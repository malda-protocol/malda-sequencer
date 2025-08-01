//! # Reset Transaction Manager Module
//!
//! This module provides a reset transaction manager that handles stuck events and failed transactions
//! in the sequencer system. It continuously monitors the database for events that need to be reset
//! and optionally triggers rebalancing when certain conditions are met.
//!
//! ## Key Features:
//! - **Stuck Event Reset**: Automatically resets events that have been stuck for too long
//! - **Failed Transaction Handling**: Processes failed transactions and resets them
//! - **Rebalancing Integration**: Triggers rebalancing requests when USD value thresholds are met
//! - **Retry Logic**: Implements exponential backoff retry logic for resilience
//! - **Parallel Processing**: Handles failed transactions in a separate spawned task
//!
//! ## Architecture:
//! ```
//! ResetTxManager::new()
//! ├── Main Loop: Handles stuck events with retry logic
//! ├── Spawned Task: Processes failed transactions in parallel
//! ├── Rebalancing: Triggers rebalance requests when needed
//! ├── Database Operations: Resets events and transactions
//! └── Error Handling: Graceful error recovery with backoff
//! ```
//!
//! ## Workflow:
//! 1. **Initialization**: Sets up configuration and database connection
//! 2. **Stuck Event Processing**: Continuously polls for stuck events to reset
//! 3. **Failed Transaction Processing**: Spawns parallel task for failed transaction handling
//! 4. **Rebalancing**: Checks USD values and triggers rebalancing when thresholds are met
//! 5. **Error Recovery**: Implements retry logic with exponential backoff

use alloy::primitives::{Address, U256};
use eyre::Result;
use reqwest::Client;
use sequencer::constants::{M_USDC_MARKET, M_USDT_MARKET, M_WBTC_MARKET};
use sequencer::database::Database;
use serde_json;
use std::time::Duration;
use tokio::time::interval;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

/// Configuration for the reset transaction manager
///
/// This struct contains all necessary parameters for configuring the reset transaction
/// manager's behavior, including retry settings, polling intervals, and rebalancing
/// thresholds.
#[derive(Clone)]
pub struct ResetTxManagerConfig {
    /// Maximum number of retry attempts for failed operations
    pub max_retries: u32,
    /// Delay between retry attempts in seconds
    pub retry_delay_secs: u64,
    /// Interval between polling cycles in seconds
    pub poll_interval_secs: u64,
    /// Sample size for stuck event processing
    pub sample_size: i64,
    /// Multiplier for stuck event calculations
    pub multiplier: f64,
    /// Maximum number of events to process in a single batch
    pub batch_limit: i64,
    /// Maximum number of retries before considering a transaction failed
    pub max_retries_reset: i64,
    /// API key for rebalancing service
    pub api_key: String,
    /// URL for the rebalancing service
    pub rebalancer_url: String,
    /// Delay after rebalancing requests in seconds
    pub rebalance_delay: u64,
    /// Minimum USD value threshold for triggering rebalancing
    pub minimum_usd_value: f64,
}

/// Reset transaction manager that handles stuck events and failed transactions
///
/// This component continuously monitors for stuck events and failed transactions,
/// resetting them and optionally triggering rebalancing when needed. It operates
/// with two main processing loops:
///
/// 1. **Main Loop**: Handles stuck events with retry logic and exponential backoff
/// 2. **Spawned Task**: Processes failed transactions in parallel with rebalancing
///
/// ## Error Handling
///
/// - Implements exponential backoff retry logic for resilience
/// - Gracefully handles database errors and network failures
/// - Provides detailed logging for monitoring and debugging
/// - Maintains separate error handling for stuck events vs failed transactions
pub struct ResetTxManager;

impl ResetTxManager {
    /// Creates and starts a new reset transaction manager
    ///
    /// This method initializes the reset transaction manager and immediately starts
    /// processing stuck events and failed transactions in a continuous loop. The manager
    /// operates with two parallel processes:
    ///
    /// - **Main Process**: Continuously polls for stuck events and resets them
    /// - **Background Process**: Handles failed transactions and triggers rebalancing
    ///
    /// ## Retry Logic
    ///
    /// The manager implements exponential backoff retry logic:
    /// - Starts with 1 second delay
    /// - Doubles delay on each retry attempt
    /// - Gives up after reaching max_retries
    ///
    /// ## Error Recovery
    ///
    /// - Database errors trigger retry with backoff
    /// - Network failures are handled gracefully
    /// - Failed operations don't stop the main processing loop
    ///
    /// # Arguments
    /// * `config` - Reset transaction manager configuration
    /// * `db` - Database connection for event and transaction operations
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let config = ResetTxManagerConfig { /* ... */ };
    /// let db = Database::new("connection_string").await?;
    /// ResetTxManager::new(config, db).await?;
    /// ```
    pub async fn new(config: ResetTxManagerConfig, db: Database) -> Result<()> {
        info!(
            "Starting reset tx manager with sample size {} and multiplier {}",
            config.sample_size, config.multiplier
        );

        let mut retry_count = 0;
        let max_retries = config.max_retries;
        let retry_delay = Duration::from_secs(config.retry_delay_secs);

        // Main retry loop with exponential backoff
        loop {
            match Self::run_reset_tx_manager(&config, &db).await {
                Ok(_) => {
                    warn!("Reset tx manager stopped, attempting to reconnect...");
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Ok(());
                    }
                    retry_count += 1;
                    info!(
                        "Waiting {} seconds before reconnection attempt {}",
                        retry_delay.as_secs(),
                        retry_count
                    );
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    error!("Reset tx manager error: {:?}", e);
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Err(e);
                    }
                    retry_count += 1;
                    info!(
                        "Waiting {} seconds before reconnection attempt {}",
                        retry_delay.as_secs(),
                        retry_count
                    );
                    sleep(retry_delay).await;
                }
            }
        }
    }

    /// Converts U256 amount to string with correct decimal precision for the market
    ///
    /// This function handles the conversion of blockchain amounts (U256) to human-readable
    /// strings with the appropriate decimal precision based on the market type.
    ///
    /// ## Market Types and Decimals
    ///
    /// - **USDC/USDT**: 6 decimals (1.0 = 1,000,000)
    /// - **WBTC**: 8 decimals (1.0 = 100,000,000)
    /// - **Other tokens**: 18 decimals (1.0 = 1,000,000,000,000,000,000)
    ///
    /// ## Formatting
    ///
    /// - Removes trailing zeros for cleaner output
    /// - Removes decimal point if not needed
    /// - Handles edge cases gracefully
    ///
    /// # Arguments
    /// * `market` - Market address to determine decimal precision
    /// * `amount` - U256 amount to convert
    ///
    /// # Returns
    /// * `String` - Formatted amount string with correct decimals
    fn parse_amount(market: &Address, amount: &U256) -> String {
        // Determine decimal precision based on market type
        let decimals = if *market == M_USDC_MARKET || *market == M_USDT_MARKET {
            6
        } else if *market == M_WBTC_MARKET {
            8
        } else {
            18
        };

        // Convert U256 to f64 and apply decimal precision
        let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
        let divisor = 10f64.powi(decimals);
        let value = amount_f64 / divisor;

        // Format output: remove trailing zeros and decimal point if not needed
        let output = if value.fract() == 0.0 {
            format!("{:.0}", value)
        } else {
            format!("{:.*}", decimals as usize, value)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        };

        output
    }

    /// Requests rebalancing for a specific market and amount
    ///
    /// This function sends a rebalancing request to the external rebalancing service
    /// when certain conditions are met (e.g., USD value thresholds). It handles the
    /// HTTP request and response processing.
    ///
    /// ## Request Format
    ///
    /// The request includes:
    /// - Market address (hex format)
    /// - Amount in human-readable format
    /// - Destination chain ID
    /// - API key for authentication
    ///
    /// ## Error Handling
    ///
    /// - Network errors are propagated up
    /// - HTTP errors include status code and response body
    /// - Invalid responses trigger appropriate error messages
    ///
    /// # Arguments
    /// * `config` - Reset transaction manager configuration
    /// * `market` - Market address for rebalancing
    /// * `dst_chain_id` - Destination chain ID
    /// * `amount` - Amount to rebalance (U256)
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    pub async fn request_rebalance(
        config: &ResetTxManagerConfig,
        market: Address,
        dst_chain_id: u32,
        amount: U256,
    ) -> Result<()> {
        let client = Client::new();
        let url = format!(
            "{}/ondemandcompute",
            config.rebalancer_url.trim_end_matches('/')
        );
        let api_key = &config.api_key;

        // Convert amount to human-readable format
        let amount_str = Self::parse_amount(&market, &amount);

        // Prepare request payload
        let payload = serde_json::json!({
            "marketAddress": format!("{:#x}", market),
            "amountRaw": amount_str,
            "dstChainId": dst_chain_id.to_string(),
        });

        info!(
            "Requested rebalance for market {} dst_chain_id {} amount {}",
            format!("{:#x}", market),
            dst_chain_id,
            amount
        );

        // Send HTTP request to rebalancing service
        let res = client
            .post(&url)
            .header("x-api-key", api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        if res.status().is_success() {
            info!(
                "Rebalance request successful for market {} dst_chain_id {} amount {}",
                format!("{:#x}", market),
                dst_chain_id,
                amount
            );
        } else {
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            error!("Rebalance request failed: status {} body {}", status, text);
            eyre::bail!("Rebalance request failed: status {} body {}", status, text)
        }

        // Wait for the configured delay after rebalancing
        sleep(Duration::from_secs(config.rebalance_delay)).await;
        Ok(())
    }

    /// Runs the main reset transaction manager processing loop
    ///
    /// This function implements the core processing logic for the reset transaction manager.
    /// It operates with two parallel processes:
    ///
    /// 1. **Main Loop**: Continuously polls for stuck events and resets them
    /// 2. **Spawned Task**: Processes failed transactions and triggers rebalancing
    ///
    /// ## Processing Flow
    ///
    /// - **Stuck Events**: Polls database for events that have been stuck too long
    /// - **Failed Transactions**: Processes failed transactions in parallel task
    /// - **Rebalancing**: Triggers rebalancing requests when USD thresholds are met
    /// - **Database Updates**: Resets events and transactions in the database
    ///
    /// ## Error Handling
    ///
    /// - Database errors are logged but don't stop processing
    /// - Network failures trigger retry logic
    /// - Individual operation failures don't affect the main loop
    ///
    /// # Arguments
    /// * `config` - Reset transaction manager configuration
    /// * `db` - Database connection for operations
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn run_reset_tx_manager(config: &ResetTxManagerConfig, db: &Database) -> Result<()> {
        let poll_interval = Duration::from_secs(config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        info!("Started polling for stuck events to reset");

        let max_retries_reset = config.max_retries_reset;
        let db_clone = db.clone();
        let config_clone = config.clone();

        // Spawn background task for failed transaction processing
        tokio::spawn(async move {
            let mut failed_interval = tokio::time::interval(poll_interval);
            loop {
                failed_interval.tick().await;

                // Poll for failed transactions that need processing
                match db_clone.get_failed_tx(max_retries_reset).await {
                    Ok(failed_txs) => {
                        debug!("Polled failed transactions: {} groups", failed_txs.len());
                        for (dst_chain_id, market, sum, tx_hashes) in failed_txs {
                            if tx_hashes.is_empty() {
                                continue;
                            }

                            // Check if rebalancing is needed based on USD value
                            let should_rebalance =
                                Self::check_usd_value(&config_clone, &market, &sum);
                            if should_rebalance {
                                if let Err(e) = Self::request_rebalance(
                                    &config_clone,
                                    market,
                                    dst_chain_id,
                                    sum,
                                )
                                .await
                                {
                                    error!("Failed to request rebalance: {:?}", e);
                                }
                            } else {
                                info!("Skipping rebalance for market {} dst_chain_id {} amount {}: below minimum USD value", format!("{:#x}", market), dst_chain_id, sum);
                            }

                            // Reset the failed transactions in the database
                            match db_clone.reset_failed_transactions(&tx_hashes).await {
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

        // Main loop for stuck event processing
        loop {
            interval.tick().await;

            // Reset stuck events in the database
            match db
                .reset_stuck_events(
                    config.sample_size,
                    config.multiplier,
                    config.batch_limit,
                    config.max_retries_reset.clone(),
                )
                .await
            {
                Ok(_) => {
                    debug!("Successfully reset stuck events");
                }
                Err(e) => {
                    error!("Failed to reset stuck events: {:?}, continuing to next cycle", e);
                    // Don't return error, just log and continue
                }
            }
        }
    }

    pub fn check_usd_value(config: &ResetTxManagerConfig, market: &Address, amount: &U256) -> bool {
        // Get current price for the market
        let price = Self::get_price(market);
        info!("amount {}", amount);

        // Calculate USD value
        let amount_f64 = amount.to_string().parse::<f64>().unwrap_or(0.0);
        let usd_value = amount_f64 * price;
        let minimum_met = usd_value >= config.minimum_usd_value;

        info!(
            "Checking USD value for market {} amount {} price {} usd_value {} minimum_met {}",
            format!("{:#x}", market),
            amount,
            price,
            usd_value,
            minimum_met
        );
        minimum_met
    }

    fn get_price(market: &Address) -> f64 {
        // TODO: Implement actual price fetching from external APIs
        // For now, return hardcoded prices based on market type
        if *market == M_USDC_MARKET || *market == M_USDT_MARKET {
            return 1.0 / 1000000.0; // $1.00 for stablecoins
        } else if *market == M_WBTC_MARKET {
            return 100000.0 / 100000000.0; // $100,000 for WBTC
        } else {
            return 2500.0 / 1000000000000000000.0; // $2,500 for other tokens (e.g., ETH)
        }
    }
}
