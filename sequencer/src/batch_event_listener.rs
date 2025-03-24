use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::Filter,
    transports::http::reqwest::Url,
};
use eyre::{Result, WrapErr};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{info, error, debug};

use crate::events::{
    BATCH_PROCESS_FAILED_SIG, BATCH_PROCESS_SUCCESS_SIG,
    parse_batch_process_failed_event, parse_batch_process_success_event,
};

#[derive(Debug)]
pub struct BatchEventConfig {
    pub ws_url: String,
    pub batch_submitter: Address,
    pub chain_id: u64,
}

pub struct BatchEventListener {
    config: BatchEventConfig,
}

impl BatchEventListener {
    pub fn new(config: BatchEventConfig) -> Self {
        Self { 
            config,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!(
            "Starting batch event listener for submitter={:?} chain={}",
            self.config.batch_submitter, self.config.chain_id
        );
        
        let ws_url: Url = self.config.ws_url.parse()
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

        loop {
            tokio::select! {
                Some(log) = success_stream.next() => {
                    let event = parse_batch_process_success_event(&log);
                    info!(
                        "Batch process success on chain {}: init_hash={:?}",
                        self.config.chain_id, event.init_hash
                    );
                }
                Some(log) = failure_stream.next() => {
                    let event = parse_batch_process_failed_event(&log);
                    error!(
                        "Batch process failed on chain {}: init_hash={:?}, reason={:?}",
                        self.config.chain_id, event.init_hash, event.reason
                    );
                }
                else => break,
            }
        }

        error!("Batch event streams ended unexpectedly");
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

        let _listener = BatchEventListener::new(config);
        Ok(())
    }
} 