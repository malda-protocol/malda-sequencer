use std::time::Duration;
use eyre::Result;
use sequencer::database::{Database, ChainParams};
use std::collections::HashMap;
use alloy::primitives::{Address, address};
use tokio::time::interval;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

#[derive(Clone)]
pub struct LaneManagerConfig {
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
    pub chain_params: HashMap<u32, ChainParams>,
    pub market_addresses: Vec<Address>,
}

#[derive(Clone)]
pub struct LaneManager {
    config: LaneManagerConfig,
    db: Database,
}

impl LaneManager {
    pub fn new(config: LaneManagerConfig, db: Database) -> Self {
        Self {
            config,
            db,
        }
    }

    fn get_price(&self, market: &Address) -> f64 {
        // For now, just return 1.0 for all markets
        // TODO: Implement actual price fetching
        if *market == address!("c15EF00790b987ce4B82eB9e25e1233a89589510") {
            return 1.0;
        }
        else {
            return 1.0;
        }
        
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting lane manager with {} chains configured", self.config.chain_params.len());

        let mut retry_count = 0;
        let max_retries = self.config.max_retries;
        let retry_delay = Duration::from_secs(self.config.retry_delay_secs);
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        loop {
            match self.run_lane_manager().await {
                Ok(_) => {
                    warn!("Lane manager stopped, attempting to reconnect...");
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Ok(());
                    }
                    retry_count += 1;
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    error!("Lane manager error: {:?}", e);
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

    async fn run_lane_manager(&self) -> Result<()> {
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        info!("Started polling for events to process");

        loop {
            interval.tick().await;

            // Create market prices map using get_price function
            let market_prices: HashMap<Address, f64> = self.config.market_addresses
                .iter()
                .map(|market| (*market, self.get_price(market)))
                .collect();

            match self.db.update_lane_status(&self.config.chain_params, &market_prices).await {
                Ok(_) => {
                    debug!("Successfully processed events");
                }
                Err(e) => {
                    error!("Failed to process events: {:?}", e);
                    return Err(e);
                }
            }
        }
    }
}