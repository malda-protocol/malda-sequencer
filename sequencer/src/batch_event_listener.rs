use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::Filter,
    transports::http::reqwest::Url,
};
use chrono::Utc;
use eyre::{Result, WrapErr};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info};

use crate::events::{
    parse_batch_process_failed_event, parse_batch_process_success_event, BATCH_PROCESS_FAILED_SIG,
    BATCH_PROCESS_SUCCESS_SIG,
};
use sequencer::database::{Database, EventStatus, EventUpdate};

#[derive(Debug)]
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
            while let Some(log) = success_stream.next().await {
                let event = parse_batch_process_success_event(&log);
                info!(
                    "Batch process success on chain {}: init_hash={:?}",
                    chain_id, event.init_hash
                );

                if let Err(e) = db_success
                    .update_event(EventUpdate {
                        tx_hash: event.init_hash,
                        status: EventStatus::TxProcessSuccess,
                        ..Default::default()
                    })
                    .await
                {
                    error!("Failed to update database for success event: {:?}", e);
                }
            }
            error!("Success event stream ended");
        });

        // Spawn failure event handler
        let failure_handle = tokio::spawn(async move {
            while let Some(log) = failure_stream.next().await {
                let event = parse_batch_process_failed_event(&log);
                error!(
                    "Batch process failed on chain {}: init_hash={:?}, reason={:?}",
                    chain_id, event.init_hash, event.reason
                );

                if let Err(e) = db_failure
                    .update_event(EventUpdate {
                        tx_hash: event.init_hash,
                        status: EventStatus::TxProcessFail,
                        ..Default::default()
                    })
                    .await
                {
                    error!("Failed to update database for failure event: {:?}", e);
                }
            }
            error!("Failure event stream ended");
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
