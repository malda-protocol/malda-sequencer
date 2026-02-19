// Copyright (c) 2026 Merge Layers Inc.
//
// This source code is licensed under the Business Source License 1.1
// (the "License"); you may not use this file except in compliance with the
// License. You may obtain a copy of the License at
//
//     https://github.com/malda-protocol/malda-sequencer/blob/main/LICENSE-BSL.md
//
// See the License for the specific language governing permissions and
// limitations under the License.

//! # Transaction Manager Module
//!
//! ## Overview
//!
//! The Transaction Manager processes proven events and submits them as batch transactions
//! to the blockchain. It handles transaction submission, gas estimation, retry logic,
//! and status tracking for each supported chain.
//!
//! ## Architecture
//!
//! ```
//! TransactionManager::new()
//! ├── Parallel chain processing
//! │   ├── Process each chain independently
//! │   ├── Poll for proven events
//! │   ├── Create batch transactions
//! │   └── Submit with retry logic
//! ├── Gas estimation
//! │   ├── Standard gas estimation for most chains
//! │   └── Linea-specific gas estimation
//! ├── Transaction submission
//! │   ├── Retry with exponential backoff
//! │   ├── Gas price adjustment
//! │   └── Transaction confirmation
//! └── Status updates
//!     ├── BatchIncluded on success
//!     └── BatchFailed on error
//! ```
//!
//! ## Key Features
//!
//! - **Multi-Chain Support**: Processes transactions for multiple chains in parallel
//! - **Batch Processing**: Groups events into efficient batch transactions
//! - **Gas Optimization**: Dynamic gas estimation and price adjustment
//! - **Retry Logic**: Exponential backoff with configurable retries
//! - **Status Tracking**: Updates event status based on transaction results
//! - **Provider Integration**: Uses shared provider helper for reliability
//!
//! ## Transaction Flow
//!
//! 1. **Event Retrieval**: Polls database for proven events ready for submission
//! 2. **Batch Creation**: Groups events by chain and creates batch transaction
//! 3. **Gas Estimation**: Estimates gas requirements with chain-specific logic
//! 4. **Transaction Submission**: Submits transaction with retry logic
//! 5. **Confirmation**: Waits for transaction confirmation and updates status
//!
//! ## Configuration
//!
//! Each chain has its own configuration:
//! - `max_retries`: Maximum retry attempts for failed transactions
//! - `retry_delay`: Delay between retry attempts
//! - `rpc_url`: RPC endpoint for the chain
//! - `submission_delay_seconds`: Minimum delay before processing events
//! - `poll_interval`: How often to poll for new events
//! - `max_tx`: Maximum transactions per batch
//! - `tx_timeout`: Transaction confirmation timeout
//! - `gas_percentage_increase_per_retry`: Gas price increase per retry

use alloy::{
    network::ReceiptResponse,
    primitives::{Address, FixedBytes, TxHash, U256},
    providers::Provider,
};

use eyre::Result;

use chrono::{DateTime, Utc};
use lazy_static::lazy_static;
use sequencer::database::{Database, EventStatus, EventUpdate};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::debug;
use tracing::{error, info};

use crate::provider_helper::ProviderType;
use malda_rs::constants::*;

use crate::{
    events::{MINT_EXTERNAL_SELECTOR_FB4, OUT_HERE_SELECTOR_FB4, REPAY_EXTERNAL_SELECTOR_FB4, LIQUIDATE_EXTERNAL_SELECTOR_FB4},
    types::{BatchProcessMsg, IBatchSubmitter},
};

// Track last submission time for each chain
lazy_static! {
    static ref LAST_SUBMISSION_TIMES: Mutex<HashMap<u32, DateTime<Utc>>> =
        Mutex::new(HashMap::new());
    static ref PROCESSING_LOCKS: Mutex<HashMap<u32, DateTime<Utc>>> = Mutex::new(HashMap::new());
}

/// Configuration for a single chain's transaction processing
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// Maximum number of retry attempts for failed transactions
    pub max_retries: u128,
    /// Delay between retry attempts
    pub retry_delay: Duration,
    /// RPC URL for the chain
    pub rpc_url: String,
    /// Minimum delay before processing events (in seconds)
    pub submission_delay_seconds: u64,
    /// How often to poll for new events
    pub poll_interval: Duration,
    /// Maximum number of transactions to process per batch
    pub max_tx: i64,
    /// Chain-specific transaction timeout
    pub tx_timeout: Duration,
    /// Gas price increase percentage per retry
    pub gas_percentage_increase_per_retry: u128,
}

/// Configuration for transaction manager across all chains
#[derive(Debug, Clone)]
pub struct TransactionConfig {
    /// Map of chain_id to its configuration
    pub chain_configs: HashMap<u32, ChainConfig>,
}

/// Main transaction manager that processes proven events and submits batch transactions
pub struct TransactionManager;

impl TransactionManager {
    /// Creates and starts a new transaction manager
    ///
    /// This method initializes the transaction manager and immediately starts processing
    /// proven events for all configured chains in parallel.
    ///
    /// ## Workflow
    ///
    /// 1. **Initialization**: Sets up logging and configuration
    /// 2. **Parallel Processing**: Creates a task for each chain
    /// 3. **Event Polling**: Continuously polls for proven events
    /// 4. **Batch Submission**: Submits batch transactions with retry logic
    /// 5. **Status Updates**: Updates event status based on results
    ///
    /// ## Error Handling
    ///
    /// - Individual chain failures don't stop other chains
    /// - Failed transactions are marked as BatchFailed
    /// - Successful transactions are marked as BatchIncluded
    /// - Database errors are logged but don't stop processing
    ///
    /// # Arguments
    /// * `config` - Transaction configuration for all chains
    /// * `db` - Database connection for event retrieval and updates
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status (errors are logged but don't stop processing)
    pub async fn new(config: TransactionConfig, db: Database) -> Result<()> {
        info!(
            "Starting transaction manager with {} chains",
            config.chain_configs.len()
        );

        // Create a task for each chain to process independently
        let mut chain_tasks = Vec::new();
        for (chain_id, _chain_config) in &config.chain_configs {
            let config_clone = config.clone();
            let db_clone = db.clone();
            let chain_id = *chain_id;

            chain_tasks.push(tokio::spawn(async move {
                info!("Starting chain processor for chain {}", chain_id);
                Self::process_chain(chain_id, &config_clone, &db_clone).await
            }));
        }

        // Wait for all chain processors to complete (they shouldn't unless there's an error)
        futures::future::join_all(chain_tasks).await;
        Ok(())
    }

    /// Processes events for a specific chain
    ///
    /// This method continuously polls for proven events for the given chain and
    /// processes them in batches. It handles the complete workflow from event
    /// retrieval to transaction submission and status updates.
    ///
    /// ## Processing Steps
    ///
    /// 1. **Event Retrieval**: Gets proven events ready for submission
    /// 2. **Batch Processing**: Creates and submits batch transactions
    /// 3. **Status Updates**: Updates event status based on results
    /// 4. **Error Handling**: Handles failures gracefully
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to process events for
    /// * `config` - Transaction configuration
    /// * `db` - Database connection
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn process_chain(chain_id: u32, config: &TransactionConfig, db: &Database) -> Result<()> {
        let chain_config = config
            .chain_configs
            .get(&chain_id)
            .ok_or_else(|| eyre::eyre!("No config found for chain {}", chain_id))?;

        loop {
            let poll_start = Instant::now();

            // Get events for this specific chain
            match Self::get_proven_events_for_chain(db, chain_config, chain_id).await {
                Ok(events) => {
                    if !events.is_empty() {
                        info!(
                            "Found {} proven events to process for chain {}",
                            events.len(),
                            chain_id
                        );

                        // Process the batch
                        match Self::process_chain_batch(chain_config, db, &events, chain_id).await {
                            Ok(tx_hash) => {
                                info!(
                                    "Successfully processed batch for chain {}: hash={:?}",
                                    chain_id, tx_hash
                                );
                                Self::update_batch_status_success(db, &events, tx_hash).await;
                            }
                            Err(e) => {
                                error!("Failed to process batch for chain {}: {}", chain_id, e);
                                Self::update_batch_status_failure(db, &events, &e.to_string())
                                    .await;
                            }
                        }
                    } else {
                        debug!(
                            "No proven events found for chain {} in this poll cycle",
                            chain_id
                        );
                        sleep(chain_config.poll_interval).await;
                    }
                }
                Err(e) => {
                    error!("Failed to get proven events for chain {}: {}", chain_id, e);
                }
            }

            let poll_duration = poll_start.elapsed();
            debug!(
                "Poll cycle completed for chain {} in {:?}",
                chain_id, poll_duration
            );
        }
    }

    /// Retrieves proven events ready for processing for a specific chain
    ///
    /// # Arguments
    /// * `db` - Database connection
    /// * `chain_config` - Chain-specific configuration
    /// * `chain_id` - Chain ID to get events for
    ///
    /// # Returns
    /// * `Result<Vec<EventUpdate>>` - Events ready for processing
    async fn get_proven_events_for_chain(
        db: &Database,
        chain_config: &ChainConfig,
        chain_id: u32,
    ) -> Result<Vec<EventUpdate>> {
        db.get_proven_events_for_chain(
            chain_config.submission_delay_seconds as i64,
            chain_id,
            chain_config.max_tx,
        )
        .await
    }

    /// Updates batch status to success (BatchIncluded)
    ///
    /// # Arguments
    /// * `db` - Database connection
    /// * `events` - Events to update
    /// * `tx_hash` - Transaction hash of successful submission
    async fn update_batch_status_success(db: &Database, events: &[EventUpdate], tx_hash: TxHash) {
        if let Err(e) = db
            .update_batch_status(
                &events.iter().map(|e| e.tx_hash).collect(),
                Some(tx_hash.to_string()),
                Some(Utc::now()),
                None,
                EventStatus::BatchIncluded,
            )
            .await
        {
            error!("Failed to update batch status to BatchIncluded: {}", e);
        }
    }

    /// Updates batch status to failure (BatchFailed)
    ///
    /// # Arguments
    /// * `db` - Database connection
    /// * `events` - Events to update
    /// * `error_msg` - Error message describing the failure
    async fn update_batch_status_failure(db: &Database, events: &[EventUpdate], error_msg: &str) {
        if let Err(e) = db
            .update_batch_status(
                &events.iter().map(|e| e.tx_hash).collect(),
                None,
                None,
                Some(error_msg.to_string()),
                EventStatus::BatchFailed {
                    error: error_msg.to_string(),
                },
            )
            .await
        {
            error!("Failed to update batch status to BatchFailed: {}", e);
        }
    }

    /// Processes a batch of events for a specific chain
    ///
    /// This method handles the complete batch processing workflow including
    /// batch creation, gas estimation, transaction submission, and confirmation.
    ///
    /// ## Processing Steps
    ///
    /// 1. **Batch Creation**: Creates batch message from events
    /// 2. **Gas Estimation**: Estimates gas requirements with chain-specific logic
    /// 3. **Transaction Submission**: Submits transaction with retry logic
    /// 4. **Confirmation**: Waits for transaction confirmation
    ///
    /// # Arguments
    /// * `chain_config` - Chain-specific configuration
    /// * `db` - Database connection
    /// * `events` - Events to process
    /// * `chain_id` - Chain ID being processed
    ///
    /// # Returns
    /// * `Result<TxHash>` - Transaction hash of successful submission
    async fn process_chain_batch(
        chain_config: &ChainConfig,
        _db: &Database,
        events: &[EventUpdate],
        chain_id: u32,
    ) -> Result<TxHash> {
        let batch_start = Instant::now();
        info!(
            "Starting batch processing for chain {} with {} events",
            chain_id,
            events.len()
        );

        // Get addresses from environment
        let batch_submitter = Self::get_batch_submitter_address();
        let sequencer = Self::get_sequencer_address();

        // Create batch message from events
        let msg = Self::create_batch_message(events, chain_id, batch_submitter)?;

        info!(
            "Preparing batch transaction for chain {}: start_idx={}, journal_size={}, seal_size={}, markets={:?}, tx_count={}",
            chain_id,
            msg.startIndex,
            msg.journalData.len(),
            msg.seal.len(),
            msg.mTokens,
            events.len()
        );

        // Submit transaction with retry logic
        let tx_hash = Self::submit_transaction_with_retry(
            chain_config,
            &msg,
            sequencer,
            batch_submitter,
            chain_id,
        )
        .await?;

        let batch_duration = batch_start.elapsed();
        info!(
            "Completed batch processing for chain {} in {:?}",
            chain_id, batch_duration
        );

        Ok(tx_hash)
    }

    /// Creates a batch message from events
    ///
    /// # Arguments
    /// * `events` - Events to include in the batch
    /// * `chain_id` - Chain ID for selector determination
    /// * `batch_submitter` - Batch submitter address
    ///
    /// # Returns
    /// * `Result<BatchProcessMsg>` - Created batch message
    fn create_batch_message(
        events: &[EventUpdate],
        chain_id: u32,
        _batch_submitter: Address,
    ) -> Result<BatchProcessMsg> {
        // Use the first event's journal and seal for the entire batch
        let journal_data = events[0].journal.clone().unwrap_or_default();
        let seal = events[0].seal.clone().unwrap_or_default();
        let start_idx = events[0].journal_index.unwrap_or(0) as u64;

        // Collect data for the batch
        let mut receivers = Vec::new();
        let mut markets = Vec::new();
        let mut amounts = Vec::new();
        let mut selectors = Vec::new();
        let mut init_hashes = Vec::new();
        let mut user_to_liquidate = Vec::new();
        let mut collateral = Vec::new();

        for event in events {
            receivers.push(event.msg_sender.unwrap_or_default());
            markets.push(event.market.unwrap_or_default());
            amounts.push(event.amount.unwrap_or_default());
            selectors.push(Self::get_selector_for_chain(chain_id, event)?);
            init_hashes.push(event.tx_hash.into());
            
            // For liquidate events, use the actual liquidatee and collateral
            // For non-liquidate events, use zero address (will be ignored by contract)
            user_to_liquidate.push(event.liquidatee.unwrap_or_default());
            collateral.push(event.collateral.unwrap_or_default());
        }

        let min_amounts_out = vec![U256::from(0); amounts.len()];

        Ok(BatchProcessMsg {
            receivers,
            journalData: journal_data,
            seal,
            mTokens: markets,
            amounts,
            minAmountsOut: min_amounts_out,
            selectors,
            initHashes: init_hashes,
            startIndex: U256::from(start_idx),
            userToLiquidate: user_to_liquidate,
            collateral,
        })
    }

    /// Gets the appropriate selector for a chain and event
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID
    /// * `event` - Event to get selector for
    ///
    /// # Returns
    /// * `Result<FixedBytes<4>>` - Function selector
    fn get_selector_for_chain(chain_id: u32, event: &EventUpdate) -> Result<FixedBytes<4>> {
        match chain_id {
            chain_id
                if chain_id == LINEA_CHAIN_ID as u32
                    || chain_id == LINEA_SEPOLIA_CHAIN_ID as u32 =>
            {
                match event.target_function.as_deref().unwrap_or("outHere") {
                    "outHere" => Ok(FixedBytes::from_slice(OUT_HERE_SELECTOR_FB4)),
                    "mintExternal" => Ok(FixedBytes::from_slice(MINT_EXTERNAL_SELECTOR_FB4)),
                    "repayExternal" => Ok(FixedBytes::from_slice(REPAY_EXTERNAL_SELECTOR_FB4)),
                    "liquidateExternal" => Ok(FixedBytes::from_slice(LIQUIDATE_EXTERNAL_SELECTOR_FB4)),
                    method => {
                        error!("Invalid transaction method for Linea: {}", method);
                        Err(eyre::eyre!("Invalid method for Linea: {}", method))
                    }
                }
            }
            _ => Ok(FixedBytes::from_slice(OUT_HERE_SELECTOR_FB4)),
        }
    }

    /// Submits transaction with retry logic
    ///
    /// # Arguments
    /// * `chain_config` - Chain-specific configuration
    /// * `msg` - Batch message to submit
    /// * `sequencer` - Sequencer address
    /// * `batch_submitter` - Batch submitter address
    /// * `chain_id` - Chain ID
    ///
    /// # Returns
    /// * `Result<TxHash>` - Transaction hash
    async fn submit_transaction_with_retry(
        chain_config: &ChainConfig,
        msg: &BatchProcessMsg,
        sequencer: Address,
        batch_submitter: Address,
        chain_id: u32,
    ) -> Result<TxHash> {
        let mut retry_count = 0u128;
        let mut tx_hash = None;
        let mut last_error = None;

        // Get provider for this chain
        let provider = Self::get_provider_for_chain(chain_id, chain_config).await?;
        let nonce = provider.get_transaction_count(sequencer).await.unwrap();
        let batch_submitter_contract = IBatchSubmitter::new(batch_submitter, provider.clone());

        while retry_count < chain_config.max_retries {
            info!(
                "Starting retry attempt {}/{} for chain {}",
                retry_count + 1,
                chain_config.max_retries,
                chain_id
            );

            // Create the action for this attempt
            let action = batch_submitter_contract
                .batchProcess(msg.clone())
                .from(sequencer);

            // Estimate gas for this attempt with proper error handling
            let gas_estimation_result =
                if chain_id == LINEA_SEPOLIA_CHAIN_ID as u32 || chain_id == LINEA_CHAIN_ID as u32 {
                    linea_estimate_gas(&provider, action.calldata().to_vec()).await
                } else {
                    // For non-Linea chains, estimate gas and get base fee
                    let gas_result = action.estimate_gas().await;
                    let base_fee_result = provider.get_gas_price().await;

                    // Combine results with proper error handling
                    match (gas_result, base_fee_result) {
                        (Ok(gas_limit), Ok(base_fee_per_gas)) => {
                            let gas_limit = gas_limit * 120 / 100; // 20% buffer
                            let priority_fee_per_gas = base_fee_per_gas;
                            Ok((gas_limit, base_fee_per_gas, priority_fee_per_gas))
                        }
                        (Err(e), _) => Err(eyre::eyre!("Failed to estimate gas: {}", e)),
                        (_, Err(e)) => Err(eyre::eyre!("Failed to get gas price: {}", e)),
                    }
                };

            let (gas_limit, max_fee_per_gas, max_priority_fee_per_gas) = match gas_estimation_result
            {
                Ok(result) => result,
                Err(e) => {
                    error!("Gas estimation failed for chain {}: {}", chain_id, e);
                    last_error = Some(e.to_string());
                    sleep(chain_config.retry_delay).await;
                    retry_count += 1;
                    continue;
                }
            };

            let adjusted_max_fee = max_fee_per_gas
                * (1 + retry_count)
                * (100 + chain_config.gas_percentage_increase_per_retry)
                / 100;
            let adjusted_priority_fee = max_priority_fee_per_gas
                * (1 + retry_count)
                * (100 + chain_config.gas_percentage_increase_per_retry)
                / 100;

            // Send the transaction and get pending transaction with proper error handling
            info!(
                "Attempting to send transaction for chain {} (attempt {}/{})",
                chain_id,
                retry_count + 1,
                chain_config.max_retries
            );

            let pending_tx = match action
                .gas(gas_limit)
                .max_fee_per_gas(adjusted_max_fee)
                .max_priority_fee_per_gas(adjusted_priority_fee)
                .from(sequencer)
                .nonce(nonce)
                .send()
                .await
            {
                Ok(tx) => {
                    info!("Transaction successfully sent for chain {}", chain_id);
                    tx
                }
                Err(e) => {
                    let error_str = e.to_string();
                    // Check if error is "nonce too low" which indicates previous transaction was included
                    if error_str.to_lowercase().contains("nonce too low") {
                        info!("Received 'nonce too low' error for chain {}, indicating previous transaction was included", chain_id);
                        if let Some(prev_hash) = tx_hash {
                            if let Ok(Some(receipt)) =
                                provider.get_transaction_receipt(prev_hash).await
                            {
                                if receipt.status() == true {
                                    info!("Previous transaction was successful for chain {}: hash={:?}", chain_id, receipt.transaction_hash);
                                    return Ok(prev_hash);
                                }
                            }
                        } else {
                            info!("No tx_hash available, but got 'nonce too low'. Treating as success.");
                            return Ok(TxHash::ZERO);
                        }
                    }

                    error!(
                        "Transaction submission failed for chain {}: {}",
                        chain_id, e
                    );
                    last_error = Some(error_str);
                    sleep(chain_config.retry_delay).await;
                    retry_count += 1;
                    continue;
                }
            };

            // After the first successful submission, tx_hash will always be set for subsequent retries.
            tx_hash = Some(pending_tx.tx_hash().clone());

            info!(
                "Submitted batch transaction for chain {}: hash={:?} (attempt {}/{})",
                chain_id,
                tx_hash,
                retry_count + 1,
                chain_config.max_retries
            );

            // Wait for transaction confirmation
            match tokio::time::timeout(chain_config.tx_timeout, pending_tx.watch()).await {
                Ok(Ok(hash)) => {
                    info!(
                        "Batch transaction confirmed for chain {}: hash={:?}",
                        chain_id, hash
                    );

                    let receipt = provider
                        .get_transaction_receipt(hash)
                        .await?
                        .ok_or_else(|| eyre::eyre!("Transaction receipt not found"))?;

                    // Check transaction status
                    if receipt.status() == true {
                        info!(
                            "Batch transaction successful for chain {}: hash={:?}, gas_used={}",
                            chain_id,
                            receipt.transaction_hash,
                            receipt.gas_used()
                        );
                        return Ok(hash);
                    } else {
                        // Transaction reverted
                        error!(
                            "Transaction reverted for chain {}: hash={:?}",
                            chain_id, hash
                        );
                        break;
                    }
                }
                Ok(Err(e)) => {
                    // Transaction failed
                    error!(
                        "Transaction failed for chain {}: error={}, hash={:?}",
                        chain_id, e, tx_hash
                    );
                    last_error = Some(e.to_string());
                    sleep(chain_config.retry_delay).await;
                }
                Err(_) => {
                    // Transaction timed out
                    error!(
                        "Transaction timed out for chain {}: hash={:?}",
                        chain_id, tx_hash
                    );

                    // Check if the transaction was actually included despite the timeout
                    if let Some(hash) = tx_hash {
                        if let Ok(Some(receipt)) = provider.get_transaction_receipt(hash).await {
                            info!("Transaction was actually included despite timeout for chain {}: hash={:?}", chain_id, hash);
                            if receipt.status() == true {
                                info!(
                                    "Batch transaction successful for chain {}: hash={:?}, gas_used={}",
                                    chain_id,
                                    receipt.transaction_hash,
                                    receipt.gas_used()
                                );
                                return Ok(hash);
                            } else {
                                error!(
                                    "Transaction was included but reverted for chain {}: hash={:?}",
                                    chain_id, hash
                                );
                                break;
                            }
                        }
                    }

                    // If we've reached max retries, update the events as failed
                    if retry_count == chain_config.max_retries - 1 {
                        break;
                    } else {
                        sleep(chain_config.retry_delay).await;
                    }
                }
            }

            retry_count += 1;
        }

        let error_msg = format!(
            "Failed to submit transaction for chain {} after {} retries: {}",
            chain_id,
            chain_config.max_retries,
            last_error.unwrap_or_else(|| "Unknown error".to_string())
        );
        error!("{}", error_msg);
        Err(eyre::eyre!(error_msg))
    }

    /// Gets provider for a specific chain with proper signing credentials
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID
    /// * `chain_config` - Chain-specific configuration
    ///
    /// # Returns
    /// * `Result<ProviderType>` - Provider for the chain
    async fn get_provider_for_chain(
        chain_id: u32,
        chain_config: &ChainConfig,
    ) -> Result<ProviderType> {
        use alloy::{
            network::EthereumWallet, providers::ProviderBuilder, signers::local::PrivateKeySigner,
            transports::http::reqwest::Url,
        };

        // Get the sequencer private key from environment
        let private_key = std::env::var("SEQUENCER_PRIVATE_KEY")
            .expect("SEQUENCER_PRIVATE_KEY must be set in .env");

        let signer: PrivateKeySigner = private_key
            .parse()
            .map_err(|e| eyre::eyre!("Failed to parse sequencer private key: {}", e))?;
        let wallet = EthereumWallet::from(signer);

        // Parse the RPC URL
        let rpc_url: Url = chain_config
            .rpc_url
            .parse()
            .map_err(|e| eyre::eyre!("Invalid RPC URL for chain {}: {}", chain_id, e))?;

        // Create provider with proper signing credentials
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(rpc_url);

        Ok(provider)
    }

    /// Gets batch submitter address from environment
    ///
    /// # Returns
    /// * `Address` - Batch submitter address
    fn get_batch_submitter_address() -> Address {
        Address::from_str(
            &std::env::var("BATCH_SUBMITTER_ADDRESS")
                .expect("BATCH_SUBMITTER_ADDRESS must be set in .env"),
        )
        .expect("Invalid BATCH_SUBMITTER_ADDRESS")
    }

    /// Gets sequencer address from environment
    ///
    /// # Returns
    /// * `Address` - Sequencer address
    fn get_sequencer_address() -> Address {
        Address::from_str(
            &std::env::var("SEQUENCER_ADDRESS").expect("SEQUENCER_ADDRESS must be set in .env"),
        )
        .expect("Invalid SEQUENCER_ADDRESS")
    }
}

/// Estimates gas for Linea and Linea Sepolia chains using the linea_estimateGas JSON-RPC method
/// Returns a tuple of (gas_limit, base_fee_per_gas, priority_fee_per_gas)
async fn linea_estimate_gas(provider: &ProviderType, data: Vec<u8>) -> Result<(u64, u128, u128)> {
    let batch_submitter = TransactionManager::get_batch_submitter_address();
    let sequencer = TransactionManager::get_sequencer_address();

    // Create the request parameters for linea_estimateGas
    let params = serde_json::json!({
        "from": format!("{:?}", sequencer),
        "to": format!("{:?}", batch_submitter),
        "data": format!("0x{}", hex::encode(data)),
        "value": format!("0x{:x}", U256::from(0)),
    });

    // Call the linea_estimateGas JSON-RPC method
    let response: serde_json::Value = provider
        .raw_request(
            std::borrow::Cow::Borrowed("linea_estimateGas"),
            vec![params],
        )
        .await
        .map_err(|e| eyre::eyre!("Failed to call linea_estimateGas: {}", e))?;

    // Parse the response
    let result = response
        .as_object()
        .ok_or_else(|| eyre::eyre!("Invalid response format"))?;

    let gas_limit = result
        .get("gasLimit")
        .and_then(|v| v.as_str())
        .and_then(|s| u64::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .ok_or_else(|| eyre::eyre!("Failed to parse gasLimit"))?;

    let base_fee_per_gas = result
        .get("baseFeePerGas")
        .and_then(|v| v.as_str())
        .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .ok_or_else(|| eyre::eyre!("Failed to parse baseFeePerGas"))?;

    let priority_fee_per_gas = result
        .get("priorityFeePerGas")
        .and_then(|v| v.as_str())
        .and_then(|s| u128::from_str_radix(s.trim_start_matches("0x"), 16).ok())
        .ok_or_else(|| eyre::eyre!("Failed to parse priorityFeePerGas"))?;

    let default_increase_percentage: u128 = 20;
    let increased_gas_limit =
        ((100 + default_increase_percentage) * gas_limit as u128 / 100) as u64;
    let increased_base_fee_per_gas =
        (100 + default_increase_percentage) * (base_fee_per_gas + priority_fee_per_gas) / 100;
    let increased_priority_fee_per_gas =
        (100 + default_increase_percentage) * priority_fee_per_gas / 100;

    Ok((
        increased_gas_limit,
        increased_base_fee_per_gas,
        increased_priority_fee_per_gas,
    ))
}
