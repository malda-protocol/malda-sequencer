use std::time::Duration;
use eyre::Result;
use sequencer::database::{Database, ChainParams};
use std::collections::HashMap;
use alloy::primitives::{Address, address};
use tokio::time::interval;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct ResetTxManagerConfig {
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
    pub sample_size: i64,
    pub multiplier: f64,
    pub batch_limit: i64,
    pub max_retries_reset: i64,
}

pub struct ResetTxManager {
    config: ResetTxManagerConfig,
    db: Database,
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

impl ResetTxManager {
    pub fn new(config: ResetTxManagerConfig, db: Database) -> Self {
        Self {
            config,
            db,
            task_handle: None,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        // Cancel any existing task
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for the task to finish
        }

        // Spawn new task
        let config = self.config.clone();
        let db = self.db.clone();
        
        let handle = tokio::spawn(async move {
            let manager = ResetTxManager {
                config,
                db,
                task_handle: None,
            };
            
            if let Err(e) = manager.run().await {
                error!("Reset tx manager failed: {:?}", e);
            }
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    async fn run(&self) -> Result<()> {
        info!("Starting reset tx manager with sample size {} and multiplier {}", self.config.sample_size, self.config.multiplier);

        let mut retry_count = 0;
        let max_retries = self.config.max_retries;
        let retry_delay = Duration::from_secs(self.config.retry_delay_secs);
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        loop {
            match self.run_reset_tx_manager().await {
                Ok(_) => {
                    warn!("Reset tx manager stopped, attempting to reconnect...");
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Ok(());
                    }
                    retry_count += 1;
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    error!("Reset tx manager error: {:?}", e);
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

    async fn run_reset_tx_manager(&self) -> Result<()> {
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        info!("Started polling for stuck events to reset");

        let max_retries_reset = self.config.max_retries_reset;
        let db = self.db.clone();
        let failed_tx_handle = tokio::spawn(async move {
            let mut failed_interval = tokio::time::interval(poll_interval);
            loop {
                failed_interval.tick().await;
                match db.get_failed_tx(max_retries_reset).await {
                    Ok(failed_txs) => {
                        debug!("Polled failed transactions: {} groups", failed_txs.len());
                        for (_dst_chain_id, _market, _sum, tx_hashes) in failed_txs {
                            if tx_hashes.is_empty() {
                                continue;
                            }
                            match db.reset_failed_transactions(&tx_hashes).await {
                                Ok(_) => {
                                    debug!("Reset {} failed transactions", tx_hashes.len());
                                }
                                Err(e) => {
                                    error!("Failed to reset failed transactions: {:?}", e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to poll failed transactions: {:?}", e);
                    }
                }
            }
        });

        loop {
            interval.tick().await;

            match self.db.reset_stuck_events(
                self.config.sample_size,
                self.config.multiplier,
                self.config.batch_limit, // batch_limit
                self.config.max_retries_reset.clone(), // max_retries
            ).await {
                Ok(_) => {
                    debug!("Successfully reset stuck events");
                }
                Err(e) => {
                    error!("Failed to reset stuck events: {:?}", e);
                    return Err(e);
                }
            }
        }
    }
}

impl Drop for ResetTxManager {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.take() {
            info!("Cleaning up reset tx manager task");
            handle.abort();
            // Note: We don't await here as it's not possible in drop
        }
    }
}