use alloy::{
    primitives::{Address, keccak256},
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Log},
    transports::http::reqwest::Url,
};
use eyre::{Result, WrapErr};
use futures_util::StreamExt;
use tracing::{debug, error, info, warn};
use alloy::primitives::{ TxHash, U256};
use eyre::eyre;
use hex;
use lazy_static::lazy_static;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::task;
use tokio::time::interval;
use tokio::time::sleep;
use chrono::Utc;
use std::cmp::max;

use malda_rs::constants::*;
use crate::{
    events::{parse_supplied_event, parse_withdraw_on_extension_chain_event},
};
use crate::{HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG, HOST_BORROW_ON_EXTENSION_CHAIN_SIG};

// Import the chain ID constant from malda_rs
use malda_rs::constants::{
    ETHEREUM_SEPOLIA_CHAIN_ID, L1_BLOCK_ADDRESS_OPSTACK, LINEA_SEPOLIA_CHAIN_ID,
    rpc_url_optimism_sepolia,
};

use crate::constants::ETHEREUM_BLOCK_DELAY;
use crate::create_provider;
use crate::events::{MINT_EXTERNAL_SELECTOR, REPAY_EXTERNAL_SELECTOR};
use crate::types::IL1Block;
use sequencer::database::{Database, EventStatus, EventUpdate};
use serde::{Deserialize, Serialize};
lazy_static! {
    pub static ref ETHEREUM_BLOCK_NUMBER: AtomicU64 = AtomicU64::new(0);
}

#[derive(Debug, Clone)]
pub struct EventConfig {
    pub ws_url: String,
    pub market: Address,
    pub event_signature: String,
    pub chain_id: u64,
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
}

#[derive(Clone)]
pub struct EventListener {
    config: EventConfig,
    db: Database,
}

impl EventListener {
    pub fn new(config: EventConfig, db: Database) -> Self {
        Self {
            config,
            db,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!(
            "Starting event listener for market={:?} chain={} event={}",
            self.config.market, self.config.chain_id, self.config.event_signature
        );

        let mut retry_count = 0;
        let max_retries = self.config.max_retries;
        let retry_delay = Duration::from_secs(self.config.retry_delay_secs);

        loop {
            match self.run_event_listener().await {
                Ok(_) => {
                    warn!("Event listener stopped, attempting to reconnect...");
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Ok(());
                    }
                    retry_count += 1;
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    error!("Event listener error: {:?}", e);
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Err(e);
                    }
                    retry_count += 1;
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
            }
        }
    }

    async fn run_event_listener(&self) -> Result<()> {
        let ws_url: Url = self
            .config
            .ws_url
            .parse()
            .wrap_err_with(|| format!("Invalid WSS URL: {}", self.config.ws_url))?;

        debug!("Connecting to provider at {}", ws_url);
        let ws = WsConnect::new(ws_url);
        let provider = ProviderBuilder::new()
            .on_ws(ws)
            .await
            .wrap_err("Failed to connect to WebSocket")?;

        let filter = Filter::new()
            .event(&self.config.event_signature)
            .address(self.config.market);

        debug!("Setting up polling with filter: {:?}", filter);

        // Track the latest block we've processed
        let mut last_processed_block = 0u64;
        
        // Poll interval in seconds
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        info!("Started polling for events");

        loop {
            interval.tick().await;

            // Get current block number
            let current_block = match provider.get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    error!("Failed to get current block number: {:?}", e);
                    continue;
                }
            };

            // Don't query if we're already up to date
            if last_processed_block >= current_block {
                debug!("No new blocks since last poll (block: {})", current_block);
                continue;
            }

            // Set the from_block to the next block after the last processed one
            // If this is the first poll, start from current block
            let from_block = if last_processed_block > 0 {
                last_processed_block + 1
            } else {
                current_block
            };

            debug!(
                "Polling for logs from block {} to {}",
                from_block, current_block
            );

            // Update filter to include block range
            let range_filter = filter.clone().from_block(from_block).to_block(current_block);

            // Get logs for the block range
            let logs = match provider.get_logs(&range_filter).await {
                Ok(logs) => logs,
                Err(e) => {
                    error!("Failed to get logs: {:?}", e);
                    continue;
                }
            };

            // info!("Found {} logs in blocks {} to {}", logs.len(), from_block, current_block);

            if logs.is_empty() {
                debug!("No new events found in blocks {} to {}", from_block, current_block);
                last_processed_block = current_block;
                continue;
            }

            // Process all logs in parallel
            let mut handles = Vec::new();
            
            for log in logs {
                let self_clone = self.clone();
                let handle = tokio::spawn(async move {
                    match self_clone.process_event(log).await {
                        Ok(event_update) => Some(event_update),
                        Err(e) => {
                            error!("Failed to process event: {:?}", e);
                            None
                        }
                    }
                });
                handles.push(handle);
            }

            // Collect results
            let mut pending_updates = Vec::new();
            for handle in handles {
                if let Ok(Some(event_update)) = handle.await {
                    pending_updates.push(event_update);
                }
            }

            // Write updates to database if any
            if !pending_updates.is_empty() {
                let count = pending_updates.len();
                // info!("Writing batch of {} events to database", count);
                
                match self.db.add_new_events(&pending_updates).await {
                    Ok(_) => info!("Successfully added batch of {} new events to database", count),
                    Err(e) => error!("Failed to write events to database: {:?}", e),
                }
            }

            // Update last processed block
            last_processed_block = current_block;
        }
    }

    async fn process_event(&self, log: Log) -> Result<EventUpdate> {
        let tx_hash = log
            .transaction_hash
            .ok_or_else(|| eyre!("No transaction hash"))?;

        // Get the current block number
        let current_block = log.block_number.unwrap_or_default() as i32;
        
        // Calculate the block number when proof should be requested
        let reorg_protection_depth = match self.config.chain_id {
            ETHEREUM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM,
            LINEA_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA,
            OPTIMISM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM,
            BASE_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE,
            ETHEREUM_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM,
            OPTIMISM_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM,
            BASE_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE,
            LINEA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA,
            _ => panic!("Unsupported chain ID: {}", self.config.chain_id),
        };

        let should_request_proof_at_block = Some(current_block + reorg_protection_depth as i32);

        // Create event update with initial data
        let mut event_update = EventUpdate {
            tx_hash,
            src_chain_id: Some(self.config.chain_id.try_into().unwrap()),
            market: Some(self.config.market),
            received_at_block: Some(current_block),
            should_request_proof_at_block,
            status: EventStatus::Received, // Set to Processed immediately
            received_at: Some(Utc::now()),
            processed_at: Some(Utc::now()),
            ..Default::default()
        };

        // Process the event based on chain ID
        if self.config.chain_id == LINEA_SEPOLIA_CHAIN_ID || self.config.chain_id == LINEA_CHAIN_ID {
            // Process host chain events
            let event = parse_withdraw_on_extension_chain_event(&log);
            
            // Get the event signature from topic 0
            let event_signature = &log.topics()[0];
            
            // Determine event type based on the event signature
            let (event_type, target_function) = if *event_signature == keccak256(HOST_BORROW_ON_EXTENSION_CHAIN_SIG.as_bytes()) {
                ("HostBorrow", "outHere")
            } else if *event_signature == keccak256(HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG.as_bytes()) {
                ("HostWithdraw", "outHere")
            } else {
                info!("Unknown event signature: {}", event_signature);
                return Err(eyre::eyre!("Unknown event signature: {}", event_signature));
                
            };
            
            event_update.msg_sender = Some(event.sender);
            event_update.dst_chain_id = Some(event.dst_chain_id);
            event_update.amount = Some(event.amount);
            event_update.target_function = Some(target_function.to_string());
            event_update.event_type = Some(event_type.to_string());
        } else {
            // Process extension chain events
            let event = parse_supplied_event(&log);

            // Validate method selector before processing
            if event.linea_method_selector != MINT_EXTERNAL_SELECTOR
                && event.linea_method_selector != REPAY_EXTERNAL_SELECTOR
            {
                return Err(eyre::eyre!(
                    "Invalid method selector: {}",
                    event.linea_method_selector
                ));
            }

            let function_name = if event.linea_method_selector == MINT_EXTERNAL_SELECTOR {
                "mintExternal"
            } else {
                "repayExternal"
            };

            event_update.msg_sender = Some(event.from);
            event_update.src_chain_id = Some(event.src_chain_id);
            event_update.dst_chain_id = Some(event.dst_chain_id);
            event_update.amount = Some(event.amount);
            event_update.target_function = Some(function_name.to_string());
            event_update.event_type = Some("ExtensionSupply".to_string());
        }

        // Don't update database here - return the event update for batch processing
        Ok(event_update)
    }
}
