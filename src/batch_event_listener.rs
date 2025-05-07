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
    pub ws_url: String,
    pub batch_submitter: Address,
    pub chain_id: u64,
    pub block_delay: u64,  // Number of blocks to delay processing
}

#[derive(Clone)]
pub struct BatchEventListener {
    config: BatchEventConfig,
    db: Database,
    ws_connection: Option<alloy::providers::RootProvider<alloy::pubsub::PubSubFrontend>>,
}

impl BatchEventListener {
    pub fn new(config: BatchEventConfig, db: Database) -> Self {
        Self { 
            config, 
            db,
            ws_connection: None,
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

        // Store the connection
        self.ws_connection = Some(provider.clone());

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

            // For first run, start from current_block - block_delay
            if last_processed_block == 0 {
                last_processed_block = if current_block > self.config.block_delay {
                    current_block - self.config.block_delay
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

            // Get success logs for the block range
            let success_logs = match provider.get_logs(&success_range_filter).await {
                Ok(logs) => logs,
                Err(e) => {
                    error!("Failed to get success logs: {:?}", e);
                    continue;
                }
            };

            // Get failure logs for the block range
            let failure_logs = match provider.get_logs(&failure_range_filter).await {
                Ok(logs) => logs,
                Err(e) => {
                    error!("Failed to get failure logs: {:?}", e);
                    continue;
                }
            };

            let mut block_timestamps = HashMap::new();

            for block_number in from_block..=target_block {
                let block = provider.get_block_by_number(BlockNumberOrTag::Number(block_number), false.into()).await?
                    .ok_or_else(|| eyre::eyre!("Block {} not found", block_number))?;
            
                let timestamp = block.header.inner.timestamp;
                block_timestamps.insert(block_number, timestamp);
            }

            // info!(
            //     "Found {} success and {} failure logs in blocks {} to {}", 
            //     success_logs.len(),
            //     failure_logs.len(),
            //     from_block, 
            //     target_block
            // );

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
                
                if let Err(e) = self.db.update_finished_events(&success_hashes, EventStatus::TxProcessSuccess, &success_timestamps).await {
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
                
                if let Err(e) = self.db.update_finished_events(&failure_hashes, EventStatus::TxProcessFail, &failure_timestamps).await {
                    error!("Failed to update failure events: {:?}", e);
                } else {
                    info!("Successfully migrated {} failure events to finished_events", failure_hashes.len());
                }
            }

            // Update last processed block
            last_processed_block = target_block;
            // info!("Updated last_processed_block to {}", last_processed_block);
        }
    }
}

impl Drop for BatchEventListener {
    fn drop(&mut self) {
        if let Some(_provider) = self.ws_connection.take() {
            info!("Cleaning up WebSocket connection for chain {}", self.config.chain_id);
            // The provider will be dropped here, which should close the WebSocket connection
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{test::*, EVENT_CHANNEL_CAPACITY};

    #[tokio::test]
    async fn test_batch_event_listener_creation() -> Result<()> {
        let config = BatchEventConfig {
            ws_url: TEST_WS_URL.to_string(),
            batch_submitter: Address::ZERO,
            chain_id: TEST_CHAIN_ID,
            block_delay: 2,  // Default test delay
        };

        let db = Database::new("postgresql://localhost:5432/test").await?;
        let _listener = BatchEventListener::new(config, db);
        Ok(())
    }
}
