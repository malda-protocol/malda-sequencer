use alloy::{
    network::ReceiptResponse,
    primitives::{FixedBytes, TxHash, U256, Address},
    providers::Provider,
    transports::http::reqwest::Url,
};
use eyre::Result;
use futures::future::join_all;
use sequencer::database::{Database, EventStatus, EventUpdate};
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, warn, trace};
use tracing::{error, info};
use chrono::{DateTime, Utc};
use std::sync::Mutex;
use lazy_static::lazy_static;
use std::collections::HashMap;

type Bytes4 = FixedBytes<4>;

use malda_rs::constants::*;

use crate::{
    constants::{BATCH_SUBMITTER, SEQUENCER_ADDRESS, SEQUENCER_PRIVATE_KEY, TX_TIMEOUT},
    create_provider,
    events::{MINT_EXTERNAL_SELECTOR_FB4, OUT_HERE_SELECTOR_FB4, REPAY_EXTERNAL_SELECTOR_FB4},
    types::{BatchProcessMsg, IBatchSubmitter},
    ProviderType,
};

// Track last submission time for each chain
lazy_static! {
    static ref LAST_SUBMISSION_TIMES: Mutex<HashMap<u32, DateTime<Utc>>> = Mutex::new(HashMap::new());
    static ref PROCESSING_LOCKS: Mutex<HashMap<u32, DateTime<Utc>>> = Mutex::new(HashMap::new());
}

#[derive(Debug, Clone)]
pub struct TransactionConfig {
    pub max_retries: u128,
    pub retry_delay: Duration,
    pub rpc_urls: Vec<(u32, String, u64)>, // (chain_id, url, submission_delay_seconds)
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
        info!("Initializing TransactionManager with config: {:?}", config);
        Self {
            config,
            db,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting transaction manager with poll interval: {:?}", self.config.poll_interval);

        loop {
            let poll_start = Instant::now();
            info!("Starting new poll cycle");

            // Get all events with ProofReceived status from the database
            match self.db.get_proven_events(self.config.poll_interval.as_secs() as i64).await {
                Ok(events) => {
                    if !events.is_empty() {
                        info!("Found {} proven events to process", events.len());
                        // Log chain distribution of events
                        let chain_counts: HashMap<u32, usize> = events.iter()
                            .map(|e| (e.dst_chain_id.unwrap_or(0) as u32, 1))
                            .fold(HashMap::new(), |mut acc, (chain, count)| {
                                *acc.entry(chain).or_insert(0) += count;
                                acc
                            });
                        info!("Event distribution by chain: {:?}", chain_counts);
                        trace!("Proven events details: {:?}", events);
                        
                        // Process the events and check the result
                        match self.process_events(events).await {
                            Ok(_) => {
                                info!("Successfully processed batch of events");
                            }
                            Err(e) => {
                                error!("Failed to process events: {}", e);
                            }
                        }
                    } else {
                        debug!("No proven events found in this poll cycle");
                    }
                }
                Err(e) => {
                    error!("Failed to get proven events from database: {}", e);
                }
            }

            let poll_duration = poll_start.elapsed();
            debug!("Poll cycle completed in {:?}", poll_duration);
            
            // Wait before next poll
            info!("Waiting {:?} before next poll cycle", self.config.poll_interval);
            sleep(self.config.poll_interval).await;
        }
    }

    async fn process_events(&self, events: Vec<EventUpdate>) -> Result<()> {
        let process_start = Instant::now();
        info!("Starting to process {} events", events.len());

        // Sort events by journal_index to maintain the same order as in the proof generator
        let mut sorted_events = events;
        sorted_events.sort_by(|a, b| {
            let a_journal_idx = a.journal_index.unwrap_or(0);
            let b_journal_idx = b.journal_index.unwrap_or(0);
            a_journal_idx.cmp(&b_journal_idx)
        });
        
        debug!("Events sorted by journal index");
        trace!("Sorted events: {:?}", sorted_events);
        
        // Group events by destination chain ID and track both start and end indices
        let mut chain_indices: HashMap<u32, (usize, usize)> = HashMap::new();
        let mut current_chain_id = None;
        let mut current_start_idx = 0;
        
        // Find the start and end indices for each chain
        for (idx, event) in sorted_events.iter().enumerate() {
            let dst_chain_id = event.dst_chain_id.unwrap_or(0) as u32;
            
            if current_chain_id != Some(dst_chain_id) {
                // If we've seen this chain before, update its end index
                if let Some(chain_id) = current_chain_id {
                    chain_indices.insert(chain_id, (current_start_idx, idx));
                    debug!("Chain {} events from index {} to {}", chain_id, current_start_idx, idx);
                }
                
                current_chain_id = Some(dst_chain_id);
                current_start_idx = idx;
                info!("Found new chain {} starting at index {}", dst_chain_id, idx);
            }
        }
        
        // Don't forget to add the last chain with the end index being the length of sorted_events
        if let Some(chain_id) = current_chain_id {
            chain_indices.insert(chain_id, (current_start_idx, sorted_events.len()));
            debug!("Chain {} events from index {} to {}", chain_id, current_start_idx, sorted_events.len());
        }
        
        info!("Events grouped by {} different chains", chain_indices.len());
        info!("Chain indices: {:?}", chain_indices);
        
        let mut chain_tasks = Vec::new();
        let config = self.config.clone();
        let db = self.db.clone();
        
        // Process one batch per chain
        for (chain_id, (start_idx, end_idx)) in &chain_indices {
            // Get the slice of events for this chain
            let chain_events = &sorted_events[*start_idx..*end_idx];
            
            info!("Processing chain {} with {} events (indices {} to {})", 
                  chain_id, chain_events.len(), start_idx, end_idx - 1);
            
            // Clone the values we need for the async task
            let chain_id = *chain_id;
            let start_idx = *start_idx;
            let chain_events = chain_events.to_vec(); // Clone the slice to own it
            let config = config.clone();
            let db = db.clone();
            
            chain_tasks.push(tokio::spawn(async move {
                let task_start = Instant::now();
                info!("Starting batch processing task for chain {}", chain_id);
                
                match Self::process_chain_batch(
                    &db,
                    &chain_events,
                    start_idx,
                    chain_events.len(),
                    chain_id,
                    &config,
                ).await {
                    Ok(tx_hash) => {
                        let duration = task_start.elapsed();
                        info!(
                            "Batch transaction completed for chain {}: hash={:?}, events={}, start_idx={}, duration={:?}",
                            chain_id, tx_hash, chain_events.len(), start_idx, duration
                        );
                    }
                    Err(e) => {
                        let duration = task_start.elapsed();
                        error!(
                            "Batch processing failed for chain {} after {:?}: {}",
                            chain_id, duration, e
                        );
                    }
                }
            }));
        }

        // Wait for all chain transactions to complete
        if !chain_tasks.is_empty() {
            info!("Waiting for {} chain batch tasks to complete", chain_tasks.len());
            join_all(chain_tasks).await;
            info!("All chain batch tasks completed");
        } else {
            warn!("No chain tasks were created despite having events to process");
        }

        let process_duration = process_start.elapsed();
        info!("Completed processing all events in {:?}", process_duration);

        Ok(())
    }

    async fn get_provider_for_chain(
        chain_id: u32,
        config: &TransactionConfig,
    ) -> Result<ProviderType> {
        debug!("Getting provider for chain {}", chain_id);
        let rpc_url = config
            .rpc_urls
            .iter()
            .find(|(id, _, _)| *id == chain_id)
            .map(|(_, url, _)| url.clone())
            .ok_or_else(|| eyre::eyre!("No RPC URL configured for chain {}", chain_id))?;

        debug!("Found RPC URL for chain {}: {}", chain_id, rpc_url);
        let url = Url::parse(&rpc_url)?;
        
        let provider = create_provider(url, SEQUENCER_PRIVATE_KEY)
            .await
            .map_err(|e| eyre::eyre!("Failed to create provider: {}", e))?;
            
        info!("Successfully created provider for chain {}", chain_id);
        Ok(provider)
    }

    async fn process_chain_batch(
        db: &Database,
        events: &Vec<EventUpdate>,
        start_idx: usize,
        _end_idx: usize,
        chain_id: u32,
        config: &TransactionConfig,
    ) -> Result<TxHash> {
        let batch_start = Instant::now();
        info!("Starting batch processing for chain {} with {} events", chain_id, events.len());

        // Get the submission delay for this chain
        let submission_delay = config
            .rpc_urls
            .iter()
            .find(|(id, _, _)| *id == chain_id)
            .map(|(_, _, delay)| *delay)
            .unwrap_or(5); // Default to 5 seconds if not configured

        debug!("Using submission delay of {} seconds for chain {}", submission_delay, chain_id);

        // Try to acquire the processing lock for this chain
        let wait_time = {
            let mut processing_locks = PROCESSING_LOCKS.lock().unwrap();
            let now = Utc::now();

            // Check if chain is currently being processed
            if let Some(lock_time) = processing_locks.get(&chain_id) {
                let elapsed = now.signed_duration_since(*lock_time);
                if elapsed.num_seconds() < submission_delay as i64 {
                    // Chain is being processed, wait another full delay
                    info!("Chain {} is currently being processed, will wait {} seconds", chain_id, submission_delay);
                    submission_delay as i64
                } else {
                    debug!("Previous lock for chain {} has expired", chain_id);
                    0
                }
            } else {
                debug!("No existing lock found for chain {}", chain_id);
                0
            }
        };

        // Wait if needed
        if wait_time > 0 {
            info!("Waiting {} seconds before submitting next batch on chain {}", wait_time, chain_id);
            sleep(Duration::from_secs(wait_time as u64)).await;
        }

        // Acquire the lock before proceeding
        {
            let mut processing_locks = PROCESSING_LOCKS.lock().unwrap();
            processing_locks.insert(chain_id, Utc::now());
            debug!("Acquired processing lock for chain {}", chain_id);
        }

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

        info!("Processing batch for chain {}: journal_size={}, seal_size={}", 
              chain_id, journal_data.len(), seal.len());

        // Collect data for the batch
        for (idx, event) in events.iter().enumerate() {
            trace!("Processing event {} for chain {}: {:?}", idx, chain_id, event);
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


        let min_amounts_out = vec![U256::from(0); amounts.len()];

        let msg = BatchProcessMsg {
            receivers,
            journalData: journal_data,
            seal,
            mTokens: markets,
            amounts,
            minAmountsOut: min_amounts_out,
            selectors,
            initHashes: init_hashes,
            startIndex: U256::from(start_idx as u64),
        };

        info!(
            "Preparing batch transaction for chain {}: start_idx={}, journal_size={}, seal_size={}, markets={:?}, tx_count={}",
            chain_id,
            start_idx,
            msg.journalData.len(),
            msg.seal.len(),
            msg.mTokens,
            events.len()
        );

        let batch_submitted_at = Utc::now();
        
        // Retry loop for transaction submission
        let mut retry_count = 0u128;
        let mut tx_hash = None;

        let percent_increase_per_retry_numerator = 20u128;
        
        while retry_count < config.max_retries {
            // Create the action for this attempt
            let action = batch_submitter.batchProcess(msg.clone()).from(SEQUENCER_ADDRESS);
            
            // Estimate gas for this attempt
            let (gas_limit, max_fee_per_gas, max_priority_fee_per_gas) = if chain_id == LINEA_SEPOLIA_CHAIN_ID as u32 || chain_id == LINEA_CHAIN_ID as u32 {
                linea_estimate_gas(&provider, action.calldata().to_vec()).await?
            } else {
                let gas_limit = action.estimate_gas().await? * 120 / 100; // 20% buffer
                let base_fee_per_gas = provider.get_gas_price().await?;
                let priority_fee_per_gas = base_fee_per_gas;
                (gas_limit, base_fee_per_gas, priority_fee_per_gas)
            };
            
            let max_fee_per_gas = max_fee_per_gas * (1 + retry_count) * (100 + percent_increase_per_retry_numerator) / 100;
            let max_priority_fee_per_gas = max_priority_fee_per_gas * (1 + retry_count) * (100 + percent_increase_per_retry_numerator) / 100;
            info!(
                "Gas estimation for chain {}: limit={}, max_fee={}, max_priority={}",
                chain_id, gas_limit, max_fee_per_gas, max_priority_fee_per_gas
            );
            
            // Send the transaction and get pending transaction
            let pending_tx = action.gas(gas_limit).max_fee_per_gas(max_fee_per_gas).max_priority_fee_per_gas(max_priority_fee_per_gas).send().await?;
            tx_hash = Some(pending_tx.tx_hash().clone());

            info!("Submitted batch transaction for chain {}: hash={:?} (attempt {}/{})", 
                  chain_id, tx_hash, retry_count + 1, config.max_retries);

            // Wait for transaction confirmation
            match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
                Ok(hash) => {
                    info!("Batch transaction confirmed for chain {}: hash={:?}", chain_id, hash);

                    let receipt = provider
                        .get_transaction_receipt(hash)
                        .await?
                        .ok_or_else(|| eyre::eyre!("Transaction receipt not found"))?;

                    // Check transaction status
                    if receipt.status() == true {
                        info!(
                            "Batch transaction successful for chain {}: hash={:?}, gas_used={}, duration={:?}",
                            chain_id,
                            receipt.transaction_hash,
                            receipt.gas_used(),
                            batch_start.elapsed()
                        );
                        
                        // Create a mutable copy of the events to update
                        let mut events_to_update = events.clone();
                        for update in &mut events_to_update {
                            update.status = EventStatus::BatchIncluded;
                            update.batch_tx_hash = Some(tx_hash.unwrap().to_string());
                            update.batch_included_at = Some(Utc::now());
                            update.batch_submitted_at = Some(batch_submitted_at);
                        }
        
                        if let Err(e) = db.update_finished_events(&events_to_update).await {
                            error!("Failed to update events for chain {}: {:?}", chain_id, e);
                        } else {
                            info!("Successfully updated {} events for chain {}", events.len(), chain_id);
                        }
                        
                        // Success - break out of retry loop
                        break;
                    } else {
                        // Transaction reverted
                        error!("Transaction reverted for chain {}: hash={:?}", chain_id, hash);

                        // Create a mutable copy of the events to update
                        let mut events_to_update = events.clone();
                        for update in &mut events_to_update {
                            update.status = EventStatus::BatchFailed {
                                error: "Transaction reverted".to_string(),
                            };
                            update.batch_tx_hash = Some(tx_hash.unwrap().to_string());
                            update.batch_submitted_at = Some(batch_submitted_at);
                        }

                        if let Err(e) = db.update_finished_events(&events_to_update).await {
                            error!("Failed to update reverted events for chain {}: {:?}", chain_id, e);
                        } else {
                            info!("Updated {} reverted events for chain {}", events.len(), chain_id);
                        }
                        
                        // Reverted - break out of retry loop
                        break;
                    }
                }
                Err(e) => {
                    // Transaction failed or timed out
                    error!("Transaction failed for chain {}: error={}, duration={:?}, hash={:?}", 
                           chain_id, e, batch_start.elapsed(), tx_hash);
                    
                    // If we've reached max retries, update the events as failed
                    if retry_count == config.max_retries - 1 {
                        // Create a mutable copy of the events to update
                        let mut events_to_update = events.clone();
                        for update in &mut events_to_update {
                            update.status = EventStatus::BatchFailed {
                                error: format!("Transaction error after {} retries: {}", config.max_retries, e),
                            };
                            update.batch_tx_hash = Some(tx_hash.unwrap().to_string());
                            update.batch_submitted_at = Some(batch_submitted_at);
                        }

                        if let Err(db_err) = db.update_finished_events(&events_to_update).await {
                            error!("Failed to update failed events for chain {}: {:?}", chain_id, db_err);
                        } else {
                            info!("Updated {} failed events for chain {}", events.len(), chain_id);
                        }
                    } else {
                        // Wait before retrying
                        info!("Waiting {:?} before retry attempt {}/{}", config.retry_delay, retry_count + 2, config.max_retries);
                        sleep(config.retry_delay).await;
                    }
                }
            }
            
            retry_count += 1;
        }

        let batch_duration = batch_start.elapsed();
        info!("Completed batch processing for chain {} in {:?}", chain_id, batch_duration);

        Ok(tx_hash.unwrap())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{providers::ProviderBuilder, primitives::U256};
    use eyre::Result;
    use url::Url;

    // Placeholder RPC URLs - Replace with actual ones

    // Placeholder RPC URLs - Replace with actual ones
    const ETH_SEPOLIA_RPC: &str = "http://localhost:8545"; // Example, replace if needed
    const OP_SEPOLIA_RPC: &str = "http://localhost:8547"; // Example, replace if needed
    const LINEA_SEPOLIA_RPC: &str = "http://localhost:8555"; // Example, replace if needed

    // Chain IDs - Replace if different ones are used in your constants
    const ETH_SEPOLIA_CHAIN_ID: u32 = 11155111;
    const OP_SEPOLIA_CHAIN_ID: u32 = 11155420;
    const LINEA_SEPOLIA_CHAIN_ID: u32 = 59141;


    #[tokio::test]
    async fn test_get_gas_prices() -> Result<()> {
        let chains = vec![
            ("ETH Sepolia", ETH_SEPOLIA_CHAIN_ID, ETH_SEPOLIA_RPC),
            ("OP Sepolia", OP_SEPOLIA_CHAIN_ID, OP_SEPOLIA_RPC),
            ("Linea Sepolia", LINEA_SEPOLIA_CHAIN_ID, LINEA_SEPOLIA_RPC),
        ];

        println!("\nFetching Gas Prices:");

        for (name, chain_id, rpc_url_str) in chains {
            println!("--- Chain: {} (ID: {}) ---", name, chain_id);
            let rpc_url = Url::parse(rpc_url_str)?;
            println!("Using RPC URL: {}", rpc_url);

            // Create a basic provider (no signer needed for get_gas_price)
            let provider = ProviderBuilder::new().on_http(rpc_url);

            match provider.get_gas_price().await {
                Ok(gas_price) => {
                    println!("Current Gas Price: {} wei", gas_price);
                }
                Err(e) => {
                    eprintln!("Failed to get gas price for {}: {}", name, e);
                    // Optionally decide if the test should fail here
                    // return Err(e.into());
                }
            }
            println!("-------------------------\n");
        }
        panic!("test");
        Ok(())
    }
 
}

    /// Estimates gas for Linea and Linea Sepolia chains using the linea_estimateGas JSON-RPC method
    /// Returns a tuple of (gas_limit, base_fee_per_gas, priority_fee_per_gas)
    async fn linea_estimate_gas(
        provider: &ProviderType,
        data: Vec<u8>,
    ) -> Result<(u64, u128, u128)> {
        // Create the request parameters for linea_estimateGas
        let params = serde_json::json!({
            "from": format!("{:?}", SEQUENCER_ADDRESS),
            "to": format!("{:?}", BATCH_SUBMITTER),
            "data": format!("0x{}", hex::encode(data)),
            "value": format!("0x{:x}", U256::from(0)),
        });

        // Call the linea_estimateGas JSON-RPC method
        let response: serde_json::Value = provider
            .raw_request(std::borrow::Cow::Borrowed("linea_estimateGas"), vec![params])
            .await
            .map_err(|e| eyre::eyre!("Failed to call linea_estimateGas: {}", e))?;

        // Parse the response
        let result = response.as_object().ok_or_else(|| eyre::eyre!("Invalid response format"))?;
        
        let gas_limit = result
            .get("gasLimit")
            .and_then(|v| v.as_str())
            .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .ok_or_else(|| eyre::eyre!("Failed to parse gasLimit"))?;
            
        let base_fee_per_gas = result
            .get("baseFeePerGas")
            .and_then(|v| v.as_str())
            .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .ok_or_else(|| eyre::eyre!("Failed to parse baseFeePerGas"))?;
            
        let priority_fee_per_gas = result
            .get("priorityFeePerGas")
            .and_then(|v| v.as_str())
            .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
            .ok_or_else(|| eyre::eyre!("Failed to parse priorityFeePerGas"))?;

        Ok((gas_limit, base_fee_per_gas + priority_fee_per_gas, priority_fee_per_gas))
    }
