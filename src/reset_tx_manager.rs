use std::time::Duration;
use eyre::Result;
use sequencer::database::{Database, ChainParams};
use std::collections::HashMap;
use alloy::primitives::{Address, address, U256};
use tokio::time::interval;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};
use reqwest::Client;
use serde_json;
use std::str::FromStr;
use sequencer::constants::{mUSDC_market, mUSDT_market, mWBTC_market};

#[derive(Clone)]
pub struct ResetTxManagerConfig {
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
    pub sample_size: i64,
    pub multiplier: f64,
    pub batch_limit: i64,
    pub max_retries_reset: i64,
    pub api_key: String,
    pub rebalancer_url: String,
    pub rebalance_delay: u64,
    pub minimum_usd_value: f64,
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

    // Converts the U256 amount to a string with the correct decimals for the market
    fn parse_amount(market: &Address, amount: &U256) -> String {
        let decimals = if *market == mUSDC_market || *market == mUSDT_market {
            6
        } else if *market == mWBTC_market {
            8
        } else {
            18
        };
        // Convert U256 to f64, divide by 10^decimals, format as string
        let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
        let divisor = 10f64.powi(decimals);
        let value = amount_f64 / divisor;
        // Remove trailing zeros and decimal point if not needed
        let output = if value.fract() == 0.0 {
            format!("{:.0}", value)
        } else {
            format!("{:.*}", decimals as usize, value).trim_end_matches('0').trim_end_matches('.').to_string()
        };

        output
    }

    pub async fn request_rebalance(
        config: &ResetTxManagerConfig,
        market: Address,
        dst_chain_id: u32,
        amount: U256,
    ) -> Result<()> {
        let client = Client::new();
        let url = format!("{}/ondemandcompute", config.rebalancer_url.trim_end_matches('/'));
        let api_key = &config.api_key;
        let amount_str = Self::parse_amount(&market, &amount);
        let payload = serde_json::json!({
            "marketAddress": format!("{:#x}", market),
            "amountRaw": amount_str,
            "dstChainId": dst_chain_id.to_string(),
        });
        info!("Requested rebalance for market {} dst_chain_id {} amount {}", format!("{:#x}", market), dst_chain_id, amount);
        let res = client
            .post(&url)
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;
        if res.status().is_success() {
            info!("Rebalance request successful for market {} dst_chain_id {} amount {}", format!("{:#x}", market), dst_chain_id, amount);
        } else {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            error!("Rebalance request failed: status {} body {}", status, text);
            eyre::bail!("Rebalance request failed: status {} body {}", status, text)
        }
        sleep(Duration::from_secs(config.rebalance_delay)).await;
        Ok(())
    }

    async fn run_reset_tx_manager(&self) -> Result<()> {
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        info!("Started polling for stuck events to reset");

        let max_retries_reset = self.config.max_retries_reset;
        let db = self.db.clone();
        let config = self.config.clone();
        let failed_tx_handle = tokio::spawn(async move {
            let mut failed_interval = tokio::time::interval(poll_interval);
            loop {
                failed_interval.tick().await;
                
                match db.get_failed_tx(max_retries_reset).await {
                    Ok(failed_txs) => {
                        debug!("Polled failed transactions: {} groups", failed_txs.len());
                        for (dst_chain_id, market, sum, tx_hashes) in failed_txs {
                            if tx_hashes.is_empty() {
                                continue;
                            }
                            let should_rebalance = Self::check_usd_value(&config, &market, &sum);
                            if should_rebalance {
                                if let Err(e) = Self::request_rebalance(&config, market, dst_chain_id, sum).await {
                                    error!("Failed to request rebalance: {:?}", e);
                                }
                            } else {
                                info!("Skipping rebalance for market {} dst_chain_id {} amount {}: below minimum USD value", format!("{:#x}", market), dst_chain_id, sum);
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

    pub fn check_usd_value(config: &ResetTxManagerConfig, market: &Address, amount: &U256) -> bool {
        let price = Self::get_price(market);
        info!("amount {}", amount);
        let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
        let usd_value = amount_f64 * price;
        let minimum_met = usd_value >= config.minimum_usd_value;
        info!("Checking USD value for market {} amount {} price {} usd_value {} minimum_met {}", format!("{:#x}", market), amount, price, usd_value, minimum_met);
        minimum_met
    }

    

    fn get_price(market: &Address) -> f64 {
        // For now, just return 1.0 for all markets
        // TODO: Implement actual price fetching
        if *market == mUSDC_market || *market == mUSDT_market {
            return 1.0 / 1000000.0;
        } else if *market == mWBTC_market {
            return 100000.0 / 100000000.0;
        } else {
            return 2500.0 / 1000000000000000000.0;
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