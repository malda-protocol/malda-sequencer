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
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting transaction manager");

        loop {
            // Get all events with ProofReceived status from the database
            match self.db.get_proven_events(self.config.poll_interval.as_secs() as i64).await {
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

        let minAmountsOut = vec![U256::from(0); amounts.len()];

        let msg = BatchProcessMsg {
            receivers,
            journalData: journal_data,
            seal,
            mTokens: markets,
            amounts,
            minAmountsOut,
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

        // Submit the batch
        let action = batch_submitter.batchProcess(msg).from(SEQUENCER_ADDRESS);

        // Estimate gas with a buffer
        let estimated_gas = action.estimate_gas().await?;
        let gas_limit = estimated_gas + (estimated_gas / 2); // Add 50% buffer

        debug!(
            "Estimated gas: {}, using gas limit: {}",
            estimated_gas, gas_limit
        );

        // Update events with batch submission pending
        for event in events {
            let mut update = event.clone();
            update.status = EventStatus::BatchSubmitted;
            update.batch_submitted_at = Some(Utc::now());

            if let Err(e) = db.update_event(update).await {
                error!("Failed to update event status to BatchSubmitted: {:?}", e);
            }
        }

        // Send the transaction and get pending transaction
        let pending_tx = action.gas(gas_limit).send().await?;
        let tx_hash = pending_tx.tx_hash().clone();

        info!("Submitted batch transaction: {:?}", tx_hash);

        // Wait for transaction confirmation
        match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
            Ok(hash) => {
                info!("Batch transaction confirmed with hash {:?}", hash);

                let receipt = provider
                    .get_transaction_receipt(hash)
                    .await?
                    .ok_or_else(|| eyre::eyre!("Transaction receipt not found"))?;

                // Check transaction status
                if receipt.status() == true {
                    info!(
                        "Batch transaction mined successfully: hash={:?}, gas_used={}",
                        receipt.transaction_hash,
                        receipt.gas_used()
                    );
                } else {
                    // Transaction reverted
                    error!("Transaction reverted with hash {:?}", hash);

                    // Update all events with failure status
                    for event in events {
                        let mut update = event.clone();
                        update.status = EventStatus::BatchFailed {
                            error: "Transaction reverted".to_string(),
                        };
                        update.batch_tx_hash = Some(tx_hash.to_string());

                        if let Err(e) = db.update_event(update).await {
                            error!("Failed to update event with failure status: {:?}", e);
                        }
                    }
                }
            }
            Err(e) => {
                // Transaction failed or timed out
                error!("Transaction failed or timed out: {}", e);

                // Update all events with failure status
                for event in events {
                    let mut update = event.clone();
                    update.status = EventStatus::BatchFailed {
                        error: format!("Transaction error: {}", e),
                    };
                    update.batch_tx_hash = Some(tx_hash.to_string());

                    if let Err(db_err) = db.update_event(update).await {
                        error!("Failed to update event with failure status: {:?}", db_err);
                    }
                }
            }
        }

        Ok(tx_hash)
    }
}
