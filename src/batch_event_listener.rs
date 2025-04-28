use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::Filter,
    transports::http::reqwest::Url,
};
use chrono::{DateTime, Duration, Utc};
use eyre::{Result, WrapErr};
use futures_util::StreamExt;
use tokio::time::{interval, sleep, Duration as TokioDuration};
use tracing::{debug, error, info, warn};
use std::time::Duration as StdDuration;
use std::vec::Vec;

use crate::events::{
    parse_batch_process_failed_event, parse_batch_process_success_event, BATCH_PROCESS_FAILED_SIG,
    BATCH_PROCESS_SUCCESS_SIG,
};
use sequencer::database::{Database, EventStatus, EventUpdate};

#[derive(Debug, Clone)]
pub struct BatchEventConfig {
    pub ws_url: String,
    pub batch_submitter: Address,
    pub chain_id: u64,
}

#[derive(Clone)]
pub struct BatchEventListener {
    config: BatchEventConfig,
    db: Database,
}

impl BatchEventListener {
    pub fn new(config: BatchEventConfig, db: Database) -> Self {
        Self { config, db }
    }

    pub async fn start(&self) -> Result<()> {
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
                "Polling for batch events from block {} to {}",
                from_block, current_block
            );

            // Update filters to include block range
            let success_range_filter = success_filter.clone()
                .from_block(from_block)
                .to_block(current_block);

            let failure_range_filter = failure_filter.clone()
                .from_block(from_block)
                .to_block(current_block);

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

            // info!(
            //     "Found {} success and {} failure logs in blocks {} to {}", 
            //     success_logs.len(),
            //     failure_logs.len(),
            //     from_block, 
            //     current_block
            // );

            let mut success_events: Vec<EventUpdate> = Vec::new();
            let mut failure_events: Vec<EventUpdate> = Vec::new();

            // Process success logs
            for log in success_logs {
                let event = parse_batch_process_success_event(&log);
                let mut event_update = EventUpdate::default();
                event_update.tx_hash = event.init_hash;
                event_update.status = EventStatus::TxProcessSuccess;
                event_update.tx_finished_at = Some(Utc::now());
                success_events.push(event_update);

                // info!(
                //     "Found batch process success on chain {}: init_hash={:?}",
                //     self.config.chain_id, event.init_hash
                // );
            }

            // Process failure logs
            for log in failure_logs {
                let event = parse_batch_process_failed_event(&log);
                let mut event_update = EventUpdate::default();
                event_update.tx_hash = event.init_hash;
                event_update.status = EventStatus::TxProcessFail;
                event_update.tx_finished_at = Some(Utc::now());
                failure_events.push(event_update);

                error!(
                    "Found batch process failed on chain {}: init_hash={:?}, reason={:?}",
                    self.config.chain_id, event.init_hash, event.reason
                );
            }

            // Update database with success events if any
            if !success_events.is_empty() {
                // info!(
                //     "Processing batch of {} success events on chain {}",
                //     success_events.len(),
                //     self.config.chain_id
                // );
                
                match self.db.update_finished_events(&success_events).await {
                    Ok(_) => info!("Successfully wrote {} success events to database", success_events.len()),
                    Err(e) => error!("Failed to update success events: {:?}", e),
                }
            }

            // Update database with failure events if any
            if !failure_events.is_empty() {
                info!(
                    "Processing batch of {} failure events on chain {}",
                    failure_events.len(),
                    self.config.chain_id
                );
                
                match self.db.update_finished_events(&failure_events).await {
                    Ok(_) => info!("Successfully wrote {} failure events to database", failure_events.len()),
                    Err(e) => error!("Failed to update failure events: {:?}", e),
                }
            }

            // Update last processed block
            last_processed_block = current_block;
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
        };

        let db = Database::new("postgresql://localhost:5432/test").await?;
        let _listener = BatchEventListener::new(config, db);
        Ok(())
    }
}
