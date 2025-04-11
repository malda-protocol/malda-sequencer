use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::Filter,
    transports::http::reqwest::Url,
};
use chrono::{DateTime, Duration, Utc};
use eyre::{Result, WrapErr};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tokio::time::{sleep, timeout, Duration as TokioDuration};
use tracing::{debug, error, info};
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

        let ws_url: Url = self
            .config
            .ws_url
            .parse()
            .wrap_err_with(|| format!("Invalid WSS URL: {}", self.config.ws_url))?;

        debug!("Connecting to WebSocket at {}", ws_url);
        let ws = WsConnect::new(ws_url);
        let provider = ProviderBuilder::new()
            .on_ws(ws)
            .await
            .wrap_err("Failed to connect to WebSocket")?;

        let success_filter = Filter::new()
            .event(BATCH_PROCESS_SUCCESS_SIG)
            .address(self.config.batch_submitter);

        let failure_filter = Filter::new()
            .event(BATCH_PROCESS_FAILED_SIG)
            .address(self.config.batch_submitter);

        debug!("Subscribing to batch events");
        let success_sub = provider.subscribe_logs(&success_filter).await?;
        let failure_sub = provider.subscribe_logs(&failure_filter).await?;

        let mut success_stream = success_sub.into_stream();
        let mut failure_stream = failure_sub.into_stream();

        info!("Successfully subscribed to batch events");

        // Clone db for both tasks
        let db_success = self.db.clone();
        let db_failure = self.db.clone();
        let chain_id = self.config.chain_id;

        // Spawn success event handler
        let success_handle = tokio::spawn(async move {
            let mut success_events: Vec<EventUpdate> = Vec::new();

            loop {
                // Try to get next event with a timeout
                match timeout(TokioDuration::from_secs(1), success_stream.next()).await {
                    Ok(Some(log)) => {
                        let event = parse_batch_process_success_event(&log);
                        let mut event_update = EventUpdate::default();
                        event_update.tx_hash = event.init_hash;
                        event_update.status = EventStatus::TxProcessSuccess;
                        event_update.tx_finished_at = Some(Utc::now());
                        success_events.push(event_update);

                        info!(
                            "Collected batch process success on chain {}: init_hash={:?}",
                            chain_id, event.init_hash
                        );
                    }
                    Ok(None) => {
                        // Stream ended
                        error!("Success event stream ended");
                        break;
                    }
                    Err(_) => {
                        // Timeout occurred, check if we should process events
                        if !success_events.is_empty() {
                            info!(
                                "Waiting 5 seconds before processing {} success events on chain {}",
                                success_events.len(),
                                chain_id
                            );
                            sleep(StdDuration::from_secs(5)).await;

                            info!(
                                "Processing batch of {} success events on chain {}",
                                success_events.len(),
                                chain_id
                            );

                            if let Err(e) = db_success.update_finished_events(&success_events).await {
                                error!("Failed to update success events to finished_events: {:?}", e);
                            }
                            success_events.clear();
                        }
                    }
                }
            }
        });

        // Spawn failure event handler
        let failure_handle = tokio::spawn(async move {
            let mut failure_events: Vec<EventUpdate> = Vec::new();

            loop {
                // Try to get next event with a timeout
                match timeout(TokioDuration::from_secs(1), failure_stream.next()).await {
                    Ok(Some(log)) => {
                        let event = parse_batch_process_failed_event(&log);
                        let mut event_update = EventUpdate::default();
                        event_update.tx_hash = event.init_hash;
                        event_update.status = EventStatus::TxProcessFail;
                        event_update.tx_finished_at = Some(Utc::now());
                        failure_events.push(event_update);

                        error!(
                            "Collected batch process failed on chain {}: init_hash={:?}, reason={:?}",
                            chain_id, event.init_hash, event.reason
                        );
                    }
                    Ok(None) => {
                        // Stream ended
                        error!("Failure event stream ended");
                        break;
                    }
                    Err(_) => {
                        // Timeout occurred, check if we should process events
                        if !failure_events.is_empty() {
                            info!(
                                "Waiting 5 seconds before processing {} failure events on chain {}",
                                failure_events.len(),
                                chain_id
                            );
                            sleep(StdDuration::from_secs(5)).await;

                            info!(
                                "Processing batch of {} failure events on chain {}",
                                failure_events.len(),
                                chain_id
                            );

                            if let Err(e) = db_failure.update_finished_events(&failure_events).await {
                                error!("Failed to update failure events to finished_events: {:?}", e);
                            }
                            failure_events.clear();
                        }
                    }
                }
            }
        });

        // Wait for both tasks to complete (they should run indefinitely)
        tokio::try_join!(success_handle, failure_handle)
            .map_err(|e| eyre::eyre!("Event listener task failed: {}", e))?;

        error!("Both event streams ended unexpectedly");
        Ok(())
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
