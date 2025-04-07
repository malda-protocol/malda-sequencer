use alloy::{
    network::ReceiptResponse,
    primitives::{FixedBytes, TxHash, U256},
    providers::Provider,
    transports::http::reqwest::Url,
};
use eyre::Result;
use futures::future::join_all;
use sequencer::database::{Database, EventStatus, EventUpdate};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{debug, warn};
use tracing::{error, info};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

type Bytes4 = FixedBytes<4>;

use crate::{
    constants::{BATCH_SUBMITTER, SEQUENCER_ADDRESS, SEQUENCER_PRIVATE_KEY, TX_TIMEOUT},
    create_provider,
    events::{MINT_EXTERNAL_SELECTOR_FB4, OUT_HERE_SELECTOR_FB4, REPAY_EXTERNAL_SELECTOR_FB4},
    types::{BatchProcessMsg, IBatchSubmitter},
    ProviderType,
};

#[derive(Debug, Clone)]
pub struct TransactionConfig {
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub rpc_urls: Vec<(u32, String)>, // (chain_id, url)
    pub poll_interval: Duration,
}

pub struct TransactionManager {
    config: TransactionConfig,
    db: Database,
    last_confirmed_tx: Arc<Mutex<HashMap<u32, TxHash>>>,
}

impl std::fmt::Debug for TransactionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionManager")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl TransactionManager {
    pub fn new(config: TransactionConfig, db: Database) -> Self {
        Self {
            config,
            db,
            last_confirmed_tx: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting transaction manager");

        loop {
            // Get all events with ProofReceived status from the database
            match self.db.get_proven_events().await {
                Ok(events) => {
                    if !events.is_empty() {
                        info!("Found {} proven events to process", events.len());
                        self.process_events(events).await?;
                    }
                }
                Err(e) => {
                    error!("Failed to get proven events from database: {}", e);
                }
            }

            // Wait before next poll
            sleep(self.config.poll_interval).await;
        }
    }

    async fn process_events(&self, events: Vec<EventUpdate>) -> Result<()> {
        let mut current_chain_id = None;
        let mut chain_start_idx = 0;
        let mut chain_tasks = Vec::new();
        let config = self.config.clone();
        let db = self.db.clone();

        // Process all events including the last batch
        for (idx, event) in events.iter().enumerate() {
            let dst_chain_id = event.dst_chain_id.unwrap_or(0) as u32;
            
            if current_chain_id != Some(dst_chain_id) {
                // Process previous chain's batch (if any)
                if let Some(chain_id) = current_chain_id {
                    let chain_events = events[chain_start_idx..idx].to_vec();
                    let config = config.clone();
                    let db = db.clone();

                    chain_tasks.push(tokio::spawn(async move {
                        match Self::process_chain_batch(
                            &db,
                            &chain_events,
                            chain_start_idx,
                            idx,
                            chain_id,
                            &config,
                        ).await {
                            Ok(tx_hash) => {
                                info!(
                                    "Batch transaction submitted successfully for chain {}: {:?} (indices {}-{})",
                                    chain_id, tx_hash, chain_start_idx, idx
                                );
                            }
                            Err(e) => {
                                error!(
                                    "Failed to process batch for chain {}: {}",
                                    chain_id, e
                                );
                            }
                        }
                    }));
                }
                current_chain_id = Some(dst_chain_id);
                chain_start_idx = idx;
            }
        }

        // Process the final chain's batch
        if let Some(chain_id) = current_chain_id {
            let chain_events = events[chain_start_idx..].to_vec();
            let config = config.clone();
            let db = db.clone();

            chain_tasks.push(tokio::spawn(async move {
                match Self::process_chain_batch(
                    &db,
                    &chain_events,
                    chain_start_idx,
                    events.len(),
                    chain_id,
                    &config,
                ).await {
                    Ok(tx_hash) => {
                        info!(
                            "Batch transaction submitted successfully for chain {}: {:?} (indices {}-{})",
                            chain_id, tx_hash, chain_start_idx, events.len()
                        );
                    }
                    Err(e) => {
                        error!(
                            "Failed to process batch for chain {}: {}",
                            chain_id, e
                        );
                    }
                }
            }));
        }

        // Wait for all chain transactions to complete
        if !chain_tasks.is_empty() {
            join_all(chain_tasks).await;
        }

        Ok(())
    }

    async fn get_provider_for_chain(
        chain_id: u32,
        config: &TransactionConfig,
    ) -> Result<ProviderType> {
        let rpc_url = config
            .rpc_urls
            .iter()
            .find(|(id, _)| *id == chain_id)
            .map(|(_, url)| url.clone())
            .ok_or_else(|| eyre::eyre!("No RPC URL configured for chain {}", chain_id))?;

        let url = Url::parse(&rpc_url)?;
        create_provider(url, SEQUENCER_PRIVATE_KEY)
            .await
            .map_err(|e| eyre::eyre!("Failed to create provider: {}", e))
    }

    async fn process_chain_batch(
        db: &Database,
        events: &[EventUpdate],
        start_idx: usize,
        _end_idx: usize,
        chain_id: u32,
        config: &TransactionConfig,
    ) -> Result<TxHash> {
        let provider = Self::get_provider_for_chain(chain_id, config).await?;
        
        // Check for pending transactions
        let nonce = provider.get_nonce(SEQUENCER_ADDRESS).await?;
        let pending_tx = provider.get_transaction_count(SEQUENCER_ADDRESS, None).await?;
        
        if pending_tx > nonce {
            // There are pending transactions, wait for them to be confirmed
            info!("Waiting for pending transactions to be confirmed for chain {}", chain_id);
            
            // Wait for pending transactions to be confirmed
            loop {
                let current_nonce = provider.get_nonce(SEQUENCER_ADDRESS).await?;
                if current_nonce == pending_tx {
                    break;
                }
                sleep(Duration::from_secs(2)).await;
            }
        }

        let batch_submitter = IBatchSubmitter::new(BATCH_SUBMITTER, provider.clone());

        // Collect all data for the batch
        let mut receivers = Vec::new();
        let mut markets = Vec::new();
        let mut amounts = Vec::new();
        let mut selectors = Vec::new();
        let mut init_hashes = Vec::new();

        // Use the first event's journal and seal for the entire batch
        let journal_data = events[0].journal.clone().unwrap_or_default();
        let seal = events[0].seal.clone().unwrap_or_default();

        info!("Started processing chain {} batch", chain_id);

        // Collect data for the batch
        for event in events {
            receivers.push(event.msg_sender.unwrap_or_default());
            markets.push(event.market.unwrap_or_default());
            amounts.push(event.amount.unwrap_or_default());
            selectors.push(match chain_id {
                // For Linea chain, use the method-specific selector
                chain_id if chain_id == malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID as u32 => {
                    match event.target_function.as_deref().unwrap_or("outHere") {
                        "outHere" => Bytes4::from_slice(OUT_HERE_SELECTOR_FB4),
                        "mintExternal" => Bytes4::from_slice(MINT_EXTERNAL_SELECTOR_FB4),
                        "repayExternal" => Bytes4::from_slice(REPAY_EXTERNAL_SELECTOR_FB4),
                        method => {
                            error!("Invalid transaction method for Linea: {}", method);
                            return Err(eyre::eyre!("Invalid method for Linea: {}", method));
                        }
                    }
                }
                // For all other chains, always use outHere
                _ => Bytes4::from_slice(OUT_HERE_SELECTOR_FB4),
            });
            init_hashes.push(event.tx_hash.into());
        }

        let msg = BatchProcessMsg {
            receivers,
            journalData: journal_data,
            seal,
            mTokens: markets,
            amounts,
            selectors,
            initHashes: init_hashes,
            startIndex: U256::from(start_idx as u64),
        };

        info!(
            "Broadcasting batch transaction for chain {} starting at index {}: journal_size={}, seal_size={}, markets={:?}, tx_count={}",
            chain_id,
            start_idx,
            msg.journalData.len(),
            msg.seal.len(),
            msg.mTokens,
            events.len()
        );

        // Modified transaction submission logic
        let mut retry_count = 0;
        let max_retries = config.max_retries;
        let mut current_gas_multiplier = 1.1;

        loop {
            let pending_tx = batch_submitter
                .batchProcess(msg.clone())
                .gas_multiplier(current_gas_multiplier)
                .priority_fee_multiplier(1.2)
                .send()
                .await?;

            let tx_hash = pending_tx.tx_hash();

            match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
                Ok(hash) => {
                    let receipt = provider
                        .get_transaction_receipt(hash)
                        .await?
                        .ok_or_else(|| eyre::eyre!("Transaction receipt not found"))?;

                    if receipt.status() == true {
                        // Transaction successful
                        info!(
                            "Batch transaction confirmed: hash={:?}, gas_used={}",
                            receipt.transaction_hash,
                            receipt.gas_used()
                        );
                        return Ok(tx_hash);
                    } else {
                        // Transaction reverted
                        if retry_count >= max_retries {
                            error!("Max retries reached for transaction");
                            // Update events with failure status
                            for event in events {
                                let mut update = event.clone();
                                update.status = EventStatus::BatchFailed {
                                    error: "Transaction reverted after max retries".to_string(),
                                };
                                update.batch_tx_hash = Some(tx_hash.to_string());
                                if let Err(e) = db.update_event(update).await {
                                    error!("Failed to update event with failure status: {:?}", e);
                                }
                            }
                            return Err(eyre::eyre!("Transaction reverted after max retries"));
                        }
                        retry_count += 1;
                        current_gas_multiplier *= 1.2; // Increase gas price for retry
                        continue;
                    }
                }
                Err(e) => {
                    if retry_count >= max_retries {
                        error!("Max retries reached for transaction");
                        // Update events with failure status
                        for event in events {
                            let mut update = event.clone();
                            update.status = EventStatus::BatchFailed {
                                error: format!("Transaction failed after max retries: {}", e),
                            };
                            update.batch_tx_hash = Some(tx_hash.to_string());
                            if let Err(db_err) = db.update_event(update).await {
                                error!("Failed to update event with failure status: {:?}", db_err);
                            }
                        }
                        return Err(eyre::eyre!("Transaction failed after max retries: {}", e));
                    }
                    retry_count += 1;
                    current_gas_multiplier *= 1.2; // Increase gas price for retry
                    continue;
                }
            }
        }
    }
}
