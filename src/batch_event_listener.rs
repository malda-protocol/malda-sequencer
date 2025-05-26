use alloy::{
    primitives::{Address, TxHash},
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::Filter,
    transports::http::reqwest::Url,
    eips::BlockNumberOrTag,
};
use chrono::{DateTime, Duration, Utc};
use eyre::{Result, WrapErr};
use futures_util::StreamExt;
use tokio::time::{interval, sleep, Duration as TokioDuration};
use tracing::{debug, error, info, warn};
use std::time::Duration as StdDuration;
use std::vec::Vec;
use std::collections::HashMap;

use crate::events::{
    parse_batch_process_failed_event, parse_batch_process_success_event, BATCH_PROCESS_FAILED_SIG, BatchProcessFailedEvent,
    BATCH_PROCESS_SUCCESS_SIG, BatchProcessSuccessEvent,
};
use sequencer::database::{Database, EventStatus, EventUpdate};

#[derive(Debug, Clone)]
pub struct BatchEventConfig {
    pub primary_ws_url: String,
    pub fallback_ws_url: String,
    pub batch_submitter: Address,
    pub chain_id: u64,
    pub block_delay: u64,  // Number of blocks to delay processing
    pub max_block_delay_secs: u64, // Maximum acceptable block delay in seconds
    pub is_retarded: bool,
}

#[derive(Clone)]
pub struct BatchEventListener {
    config: BatchEventConfig,
    db: Database,
    primary_provider: Option<alloy::providers::RootProvider<alloy::pubsub::PubSubFrontend>>,
}

impl BatchEventListener {
    pub fn new(config: BatchEventConfig, db: Database) -> Self {
        Self { 
            config, 
            db,
            primary_provider: None,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!(
            "Starting batch event listener for submitter={:?} chain={}",
            self.config.batch_submitter, self.config.chain_id
        );

        let mut retry_count = 0;
        let max_retries = 5;
        let mut retry_delay = StdDuration::from_secs(1);

        loop {
            match self.run_event_listener().await {
                Ok(_) => {
                    warn!("Batch event listener stopped, attempting to reconnect...");
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Ok(());
                    }
                    retry_count += 1;
                    retry_delay *= 2; // Exponential backoff
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    error!("Batch event listener error: {:?}", e);
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Err(e);
                    }
                    retry_count += 1;
                    retry_delay *= 2; // Exponential backoff
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
            }
        }
    }

    async fn run_event_listener(&mut self) -> Result<()> {
        // Create base filters for success and failure events
        let success_filter = Filter::new()
            .event(BATCH_PROCESS_SUCCESS_SIG)
            .address(self.config.batch_submitter);

        let failure_filter = Filter::new()
            .event(BATCH_PROCESS_FAILED_SIG)
            .address(self.config.batch_submitter);

        debug!("Setting up polling with filters");

        // Track the latest block we've processed
        let mut last_processed_block = 0u64;
        
        // Poll interval in seconds
        let poll_interval = StdDuration::from_secs(5);
        let mut interval = interval(poll_interval);

        info!("Started polling for batch events");
        
        // Flag to track if we used fallback in the previous cycle
        let mut used_fallback = false;

        loop {
            interval.tick().await;

            // Determine which provider to use for this polling cycle
            let provider = match self.get_active_provider(used_fallback).await {
                Ok((provider, is_fallback)) => {
                    used_fallback = is_fallback;
                    provider
                },
                Err(e) => {
                    error!("Failed to get active provider: {:?}", e);
                    continue;
                }
            };

            // Get current block number
            let current_block = match provider.get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    error!("Provider failed to get block number: {:?}", e);
                    used_fallback = true; // Force new primary provider on next cycle
                    continue;
                }
            };

            // For first run, start from current_block - block_delay
            if last_processed_block == 0 {
                last_processed_block = if current_block > self.config.block_delay {
                    current_block - self.config.block_delay - 2
                } else {
                    0
                };
                info!("Initializing last_processed_block to {} (current: {}, delay: {})", 
                      last_processed_block, current_block, self.config.block_delay);
                continue;
            }

            // Don't query if we're already up to date (current_block - block_delay)
            let target_block = if current_block > self.config.block_delay {
                current_block - self.config.block_delay
            } else {
                current_block
            };

            if last_processed_block >= target_block {
                debug!("No new blocks to process. Last processed: {}, Target: {}, Current: {}, Delay: {}", 
                       last_processed_block, target_block, current_block, self.config.block_delay);
                continue;
            }

            // Set the from_block to the next block after the last processed one
            let from_block = last_processed_block + 1;

            debug!(
                "Processing blocks {} to {} (current: {}, delay: {})",
                from_block, target_block, current_block, self.config.block_delay
            );

            // Update filters to include block range
            let success_range_filter = success_filter.clone()
                .from_block(from_block)
                .to_block(target_block);

            let failure_range_filter = failure_filter.clone()
                .from_block(from_block)
                .to_block(target_block);

            // Get success logs
            let success_logs = match provider.get_logs(&success_range_filter).await {
                Ok(logs) => logs,
                Err(e) => {
                    error!("Provider failed to get success logs: {:?}", e);
                    used_fallback = true; // Force new primary provider on next cycle
                    continue;
                }
            };

            // Get failure logs
            let failure_logs = match provider.get_logs(&failure_range_filter).await {
                Ok(logs) => logs,
                Err(e) => {
                    error!("Provider failed to get failure logs: {:?}", e);
                    used_fallback = true; // Force new primary provider on next cycle
                    continue;
                }
            };

            let mut block_timestamps = HashMap::new();

            // Get block timestamps
            for block_number in from_block..=target_block {
                let block = match provider.get_block_by_number(BlockNumberOrTag::Number(block_number), false.into()).await {
                    Ok(Some(block)) => block,
                    Ok(None) => return Err(eyre::eyre!("Block {} not found", block_number)),
                    Err(e) => {
                        error!("Failed to get block {}: {:?}", block_number, e);
                        used_fallback = true; // Force new primary provider on next cycle
                        continue;
                    }
                };
            
                let timestamp = block.header.inner.timestamp;
                block_timestamps.insert(block_number, timestamp);
            }

            // Process success logs
            let success_events: Vec<BatchProcessSuccessEvent> = success_logs
                .iter()
                .map(|log| parse_batch_process_success_event(log))
                .collect();
            let success_hashes: Vec<TxHash> = success_events.iter().map(|e| e.init_hash).collect();
            let success_timestamps: Vec<i64> = success_events
                .iter()
                .map(|e| *block_timestamps.get(&e.block_number).unwrap_or(&0) as i64)
                .collect();

            // Process failure logs
            let failure_events: Vec<BatchProcessFailedEvent> = failure_logs
                .iter()
                .map(|log| parse_batch_process_failed_event(log))
                .collect();
            let failure_hashes: Vec<TxHash> = failure_events.iter().map(|e| e.init_hash).collect();
            let failure_timestamps: Vec<i64> = failure_events
                .iter()
                .map(|e| *block_timestamps.get(&e.block_number).unwrap_or(&0) as i64)
                .collect();

            // Update database with success events if any
            if !success_hashes.is_empty() {
                info!(
                    "Processing batch of {} success events on chain {}",
                    success_hashes.len(),
                    self.config.chain_id
                );
                
                if let Err(e) = self.db.update_finished_events(&success_hashes, EventStatus::TxProcessSuccess, &success_timestamps, self.config.is_retarded).await {
                    error!("Failed to update success events: {:?}", e);
                } else {
                    info!("Successfully migrated {} success events to finished_events", success_hashes.len());
                }
            }

            // Update database with failure events if any
            if !failure_hashes.is_empty() {
                info!(
                    "Processing batch of {} failure events on chain {}",
                    failure_hashes.len(),
                    self.config.chain_id
                );
                
                if let Err(e) = self.db.update_finished_events(&failure_hashes, EventStatus::TxProcessFail, &failure_timestamps, self.config.is_retarded).await {
                    error!("Failed to update failure events: {:?}", e);
                } else {
                    info!("Successfully migrated {} failure events to finished_events", failure_hashes.len());
                }
            }

            // Update last processed block
            last_processed_block = target_block;
        }
    }

    // Helper method to determine which provider to use
    async fn get_active_provider(&mut self, force_new_primary: bool) -> Result<(alloy::providers::RootProvider<alloy::pubsub::PubSubFrontend>, bool)> {
        // If we have a primary provider and don't need to force a new one, check if it's still valid
        if !force_new_primary && self.primary_provider.is_some() {
            let provider = self.primary_provider.as_ref().unwrap();
            
            // Check if the primary provider is fresh enough
            match provider.get_block_by_number(BlockNumberOrTag::Latest, false.into()).await {
                Ok(Some(block)) => {
                    // Check block timestamp
                    let block_timestamp: u64 = block.header.inner.timestamp.into();
                    let current_time = chrono::Utc::now().timestamp() as u64;
                    let max_delay = self.config.max_block_delay_secs;

                    if current_time > block_timestamp && (current_time - block_timestamp) > max_delay {
                        warn!(
                            "Primary provider's latest block is too old: block_time={}, current_time={}, diff={}s, max_delay={}s, trying fallback at {}",
                            block_timestamp, current_time, current_time - block_timestamp, max_delay, self.config.fallback_ws_url
                        );
                        // Use fallback and mark primary as invalid
                        self.primary_provider = None;
                        return self.create_fallback_provider().await;
                    }
                    
                    // Primary provider is good, use it
                    debug!("Reusing existing primary provider");
                    return Ok((provider.clone(), false));
                },
                _ => {
                    // Primary provider failed, clear it and try creating a new one
                    warn!("Existing primary provider failed to get latest block, creating new one");
                    self.primary_provider = None;
                }
            }
        }

        // Create a new primary provider
        let primary_ws_url: Url = self
            .config
            .primary_ws_url
            .parse()
            .wrap_err_with(|| format!("Invalid primary WSS URL: {}", self.config.primary_ws_url))?;

        debug!("Connecting to primary provider at {}", primary_ws_url);
        let primary_ws = WsConnect::new(primary_ws_url);
        let primary_provider = match ProviderBuilder::new().on_ws(primary_ws).await {
            Ok(provider) => provider,
            Err(e) => {
                warn!("Failed to connect to primary WebSocket: {:?}, trying fallback at {}", e, self.config.fallback_ws_url);
                return self.create_fallback_provider().await;
            }
        };

        // Check if the primary provider is fresh enough
        let block = match primary_provider.get_block_by_number(BlockNumberOrTag::Latest, false.into()).await {
            Ok(Some(block)) => block,
            Ok(None) => {
                warn!("Primary provider returned no latest block, trying fallback at {}", self.config.fallback_ws_url);
                return self.create_fallback_provider().await;
            }
            Err(e) => {
                warn!("Primary provider failed to get latest block: {:?}, trying fallback at {}", e, self.config.fallback_ws_url);
                return self.create_fallback_provider().await;
            }
        };

        // Check if the block is fresh enough
        let block_timestamp: u64 = block.header.inner.timestamp.into();
        let current_time = chrono::Utc::now().timestamp() as u64;
        let max_delay = self.config.max_block_delay_secs;

        if current_time > block_timestamp && (current_time - block_timestamp) > max_delay {
            warn!(
                "Primary provider's latest block is too old: block_time={}, current_time={}, diff={}s, max_delay={}s, trying fallback at {}",
                block_timestamp, current_time, current_time - block_timestamp, max_delay, self.config.fallback_ws_url
            );
            return self.create_fallback_provider().await;
        }

        // Store the new primary provider
        debug!("Using new primary provider");
        self.primary_provider = Some(primary_provider.clone());
        Ok((primary_provider, false))
    }

    // Helper method to create fallback provider
    async fn create_fallback_provider(&self) -> Result<(alloy::providers::RootProvider<alloy::pubsub::PubSubFrontend>, bool)> {
        let fallback_ws_url: Url = self
            .config
            .fallback_ws_url
            .parse()
            .wrap_err_with(|| format!("Invalid fallback WSS URL: {}", self.config.fallback_ws_url))?;

        info!("Connecting to fallback provider at {}", fallback_ws_url);
        let fallback_ws = WsConnect::new(fallback_ws_url);
        let fallback_provider = ProviderBuilder::new()
            .on_ws(fallback_ws)
            .await
            .wrap_err("Failed to connect to fallback WebSocket")?;

        info!("Using fallback provider for this polling cycle");
        Ok((fallback_provider, true))
    }
}

impl Drop for BatchEventListener {
    fn drop(&mut self) {
        if self.primary_provider.is_some() {
            info!("Dropping primary provider connection for chain {} and batch submitter {:?}", 
                  self.config.chain_id, self.config.batch_submitter);
        }
        
        info!("Dropping batch event listener for chain {} and batch submitter {:?}", 
              self.config.chain_id, self.config.batch_submitter);
    }
}
