use eyre::Result;
use tracing::{info, error};
use alloy::{
    primitives::{TxHash, U256, FixedBytes},
    transports::http::reqwest::Url,
    providers::Provider,
    network::ReceiptResponse,
};
use tokio::sync::mpsc;
use tracing::{warn, debug};
use std::time::Duration;
use futures::future::join_all;
use sequencer::database::{Database, EventStatus, EventUpdate};
use crate::ProofReadyEvent;

type Bytes4 = FixedBytes<4>;

use crate::{
    // proof_generator::ProofReadyEvent,
    ProviderType,
    create_provider,
    constants::{
        TX_TIMEOUT,
        SEQUENCER_ADDRESS,
        SEQUENCER_PRIVATE_KEY,
        BATCH_SUBMITTER,
    },
    types::{IBatchSubmitter, BatchProcessMsg},
    events::{
        MINT_EXTERNAL_SELECTOR_FB4,
        REPAY_EXTERNAL_SELECTOR_FB4,
        OUT_HERE_SELECTOR_FB4,
    },
};

#[derive(Debug, Clone)]
pub struct TransactionConfig {
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub rpc_urls: Vec<(u32, String)>, // (chain_id, url)
}

pub struct TransactionManager {
    event_receiver: mpsc::Receiver<Vec<ProofReadyEvent>>,
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
    pub fn new(
        event_receiver: mpsc::Receiver<Vec<ProofReadyEvent>>,
        config: TransactionConfig,
        db: Database,
    ) -> Self {
        Self {
            event_receiver,
            config,
            db,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting transaction manager");
        
        while let Some(proof_events) = self.event_receiver.recv().await {
            let mut current_chain_id = None;
            let mut chain_start_idx = 0;
            let mut chain_tasks = Vec::new();
            let config = self.config.clone();
            let db = self.db.clone();

            // Process all events including the last batch
            for (idx, event) in proof_events.iter().enumerate() {
                if current_chain_id != Some(event.dst_chain_id) {
                    // Process previous chain's batch (if any)
                    if let Some(chain_id) = current_chain_id {
                        let chain_events = proof_events[chain_start_idx..idx].to_vec();
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
                    current_chain_id = Some(event.dst_chain_id);
                    chain_start_idx = idx;
                }
            }

            // Process the final chain's batch
            if let Some(chain_id) = current_chain_id {
                let chain_events = proof_events[chain_start_idx..].to_vec();
                let config = config.clone();
                let db = db.clone();
                
                chain_tasks.push(tokio::spawn(async move {
                    
                    match Self::process_chain_batch(
                        &db,
                        &chain_events,
                        chain_start_idx,
                        proof_events.len(),
                        chain_id,
                        &config,
                    ).await {
                        Ok(tx_hash) => {
                            info!(
                                "Batch transaction submitted successfully for chain {}: {:?} (indices {}-{})",
                                chain_id, tx_hash, chain_start_idx, proof_events.len()
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
        }

        warn!("Transaction manager channel closed");
        Ok(())
    }

    async fn get_provider_for_chain(chain_id: u32, config: &TransactionConfig) -> Result<ProviderType> {
        let rpc_url = config.rpc_urls
            .iter()
            .find(|(id, _)| *id == chain_id)
            .map(|(_, url)| url.clone())
            .ok_or_else(|| eyre::eyre!("No RPC URL configured for chain {}", chain_id))?;

        let url = Url::parse(&rpc_url)?;
        create_provider(url, SEQUENCER_PRIVATE_KEY).await
            .map_err(|e| eyre::eyre!("Failed to create provider: {}", e))
    }

    async fn process_chain_batch(
        db: &Database,
        events: &[ProofReadyEvent],
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
        let journal_data = events[0].journal.clone();
        let seal = events[0].seal.clone();

        info!("Started processing chain {} batch", chain_id);

        // Mark all events as included in batch
        for event in events {
            if let Err(e) = db.update_event(EventUpdate {
                tx_hash: event.tx_hash,
                status: EventStatus::IncludedInBatch,
                ..Default::default()
            }).await {
                error!("Failed to update event to IncludedInBatch: {:?}", e);
            }

            receivers.push(event.receiver);
            markets.push(event.market);
            amounts.extend(event.amount.clone());
            selectors.push(match event.dst_chain_id {
                // For Linea chain, use the method-specific selector
                chain_id if chain_id == malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID as u32 => {
                    match event.method.as_str() {
                        "outHere" => Bytes4::from_slice(OUT_HERE_SELECTOR_FB4),
                        "mintExternal" => Bytes4::from_slice(MINT_EXTERNAL_SELECTOR_FB4),
                        "repayExternal" => Bytes4::from_slice(REPAY_EXTERNAL_SELECTOR_FB4),
                        method => {
                            error!("Invalid transaction method for Linea: {}", method);
                            return Err(eyre::eyre!("Invalid method for Linea: {}", method));
                        }
                    }
                },
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

        // Submit the batch
        let action = batch_submitter
            .batchProcess(msg)
            .from(SEQUENCER_ADDRESS);

        // Estimate gas with a buffer
        let estimated_gas = action
            .estimate_gas()
            .await?;
        let gas_limit = estimated_gas + (estimated_gas / 2); // Add 50% buffer

        debug!("Estimated gas: {}, using gas limit: {}", estimated_gas, gas_limit);

        // Update events with batch submission pending
        for event in events {
            if let Err(e) = db.update_event(EventUpdate {
                tx_hash: event.tx_hash,
                status: EventStatus::BatchSubmitted,
                ..Default::default()
            }).await {
                error!("Failed to update event to BatchSubmitted: {:?}", e);
            }
        }

        let gas_price = provider.get_gas_price().await? * 2;

        let pending_tx = action
            .gas(gas_limit)
            .gas_price(gas_price)
            .send()
            .await?;

        let tx_hash = pending_tx.tx_hash().clone();

        info!("Submitted batch transaction: {:?}", tx_hash);

        // Update all events with the batch tx hash
        for event in events {
            if let Err(e) = db.update_event(EventUpdate {
                tx_hash: event.tx_hash,
                batch_tx_hash: Some(tx_hash.to_string()),
                ..Default::default()
            }).await {
                error!("Failed to update event with batch tx hash: {:?}", e);
            }
        }

        // Wait for transaction confirmation
        match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
            Ok(hash) => {
                info!("Batch transaction confirmed with hash {:?}", hash);
                
                let receipt = provider
                    .get_transaction_receipt(hash)
                    .await?
                    .ok_or_else(|| eyre::eyre!("Transaction receipt not found"))?;

                // Check transaction status
                let status = if receipt.status() == true {
                    EventStatus::BatchIncluded
                } else {
                    EventStatus::BatchFailed { error: "Transaction reverted".to_string() }
                };

                // Update all events with transaction status
                for event in events {
                    // Get current status before updating
                    let current_status = db.get_event_status(&event.tx_hash).await?;
                    
                    // Use current status if it's TxProcessSuccess or TxProcessFail, otherwise use new status
                    let status = if let Some(current) = current_status {
                        match current {
                            EventStatus::TxProcessSuccess | EventStatus::TxProcessFail => current,
                            _ => status.clone(),
                        }
                    } else {
                        status.clone()
                    };

                    if let Err(e) = db.update_event(EventUpdate {
                        tx_hash: event.tx_hash,
                        status,
                        ..Default::default()
                    }).await {
                        error!("Failed to update event with transaction status: {:?}", e);
                    }
                }

                info!(
                    "Batch transaction mined: hash={:?}, status={}, gas_used={}",
                    receipt.transaction_hash,
                    receipt.status(),
                    receipt.gas_used()
                );
            },
            Err(e) => {
                // Transaction failed or timed out
                error!("Transaction failed or timed out: {}", e);
                
                // Update all events with failure status
                for event in events {
                    // Get current status before updating
                    let current_status = db.get_event_status(&event.tx_hash).await?;
                    
                    // Use current status if it's TxProcessSuccess or TxProcessFail, otherwise use BatchFailed
                    let status = if let Some(current) = current_status {
                        match current {
                            EventStatus::TxProcessSuccess | EventStatus::TxProcessFail => current,
                            _ => EventStatus::BatchFailed { error: format!("Transaction error: {}", e) },
                        }
                    } else {
                        EventStatus::BatchFailed { error: format!("Transaction error: {}", e) }
                    };

                    if let Err(db_err) = db.update_event(EventUpdate {
                        tx_hash: event.tx_hash,
                        status,
                        resubmitted: Some(1),
                        ..Default::default()
                    }).await {
                        error!("Failed to update event with failure status: {:?}", db_err);
                    }
                }
            }
        }

        Ok(tx_hash)
    }

}
