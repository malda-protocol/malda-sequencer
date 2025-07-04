use alloy::{
    network::ReceiptResponse,
    primitives::{Address, FixedBytes, TxHash, U256},
    providers::Provider,
    transports::http::reqwest::Url,
};
use chrono::{DateTime, Utc};
use eyre::Result;
use futures::future::join_all;
use lazy_static::lazy_static;
use sequencer::database::{Database, EventStatus, EventUpdate};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use tracing::{debug, trace, warn};
use tracing::{error, info};

// ProviderType alias matching the actual provider returned by create_provider
use alloy::network::Ethereum;
use alloy::network::EthereumWallet;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy::providers::RootProvider;

type ProviderType = alloy::providers::fillers::FillProvider<
    JoinFill<
        JoinFill<
            alloy::providers::Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider<Ethereum>,
>;

type Bytes4 = FixedBytes<4>;

use malda_rs::constants::*;

use crate::{
    create_provider,
    events::{MINT_EXTERNAL_SELECTOR_FB4, OUT_HERE_SELECTOR_FB4, REPAY_EXTERNAL_SELECTOR_FB4},
    types::{BatchProcessMsg, IBatchSubmitter},
};

// Track last submission time for each chain
lazy_static! {
    static ref LAST_SUBMISSION_TIMES: Mutex<HashMap<u32, DateTime<Utc>>> =
        Mutex::new(HashMap::new());
    static ref PROCESSING_LOCKS: Mutex<HashMap<u32, DateTime<Utc>>> = Mutex::new(HashMap::new());
}

#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub max_retries: u128,
    pub retry_delay: Duration,
    pub rpc_url: String,
    pub submission_delay_seconds: u64,
    pub poll_interval: Duration,
    pub max_tx: i64,          // Maximum number of transactions to process per batch
    pub tx_timeout: Duration, // Chain-specific transaction timeout
    pub gas_percentage_increase_per_retry: u128,
}

#[derive(Debug, Clone)]
pub struct TransactionConfig {
    pub chain_configs: HashMap<u32, ChainConfig>, // Map of chain_id to its config
}

pub struct TransactionManager {
    config: TransactionConfig,
    db: Database,
}

impl std::fmt::Debug for TransactionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionManager")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl TransactionManager {
    pub fn new(config: TransactionConfig, db: Database) -> Self {
        info!(
            "Initializing TransactionManager with config for chains: {:?}",
            config.chain_configs.keys()
        );
        Self { config, db }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!(
            "Starting transaction manager with {} chains",
            self.config.chain_configs.len()
        );

        // Create a task for each chain
        let mut chain_tasks = Vec::new();
        for (chain_id, chain_config) in &self.config.chain_configs {
            let _chain_config = chain_config;
            let config = self.config.clone();
            let db = self.db.clone();
            let chain_id = *chain_id;

            chain_tasks.push(tokio::spawn(async move {
                info!("Starting chain processor for chain {}", chain_id);
                Self::process_chain(chain_id, &config, &db).await
            }));
        }

        // Wait for all chain processors to complete (they shouldn't unless there's an error)
        futures::future::join_all(chain_tasks).await;
        Ok(())
    }

    async fn process_chain(chain_id: u32, config: &TransactionConfig, db: &Database) -> Result<()> {
        let chain_config = config
            .chain_configs
            .get(&chain_id)
            .ok_or_else(|| eyre::eyre!("No config found for chain {}", chain_id))?;

        loop {
            let poll_start = Instant::now();

            // Get events for this specific chain
            match db
                .get_proven_events_for_chain(
                    chain_config.submission_delay_seconds as i64,
                    chain_id,
                    chain_config.max_tx,
                )
                .await
            {
                Ok(events) => {
                    if !events.is_empty() {
                        info!(
                            "Found {} proven events to process for chain {}",
                            events.len(),
                            chain_id
                        );

                        // Get start index from first event
                        let start_idx = events[0].journal_index.unwrap_or(0) as u64;

                        // Process the batch
                        match Self::process_chain_batch(
                            chain_config.gas_percentage_increase_per_retry,
                            db,
                            &events,
                            start_idx,
                            chain_id,
                            chain_config,
                        )
                        .await
                        {
                            Ok(tx_hash) => {
                                info!(
                                    "Successfully processed batch for chain {}: hash={:?}",
                                    chain_id, tx_hash
                                );
                                // Update events to BatchIncluded status with the transaction hash
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
                            Err(e) => {
                                error!("Failed to process batch for chain {}: {}", chain_id, e);
                                // Update events to BatchFailed status with the error message
                                if let Err(e) = db
                                    .update_batch_status(
                                        &events.iter().map(|e| e.tx_hash).collect(),
                                        None,
                                        None,
                                        Some(e.to_string()),
                                        EventStatus::BatchFailed {
                                            error: e.to_string(),
                                        },
                                    )
                                    .await
                                {
                                    error!("Failed to update batch status to BatchFailed: {}", e);
                                }
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

            // Wait for the chain-specific poll interval
        }
    }

    async fn process_chain_batch(
        percent_increase_per_retry_numerator: u128,
        _db: &Database,
        events: &Vec<EventUpdate>,
        start_idx: u64,
        chain_id: u32,
        chain_config: &ChainConfig,
    ) -> Result<TxHash> {
        let batch_start = Instant::now();
        info!(
            "Starting batch processing for chain {} with {} events",
            chain_id,
            events.len()
        );

        let batch_submitter = Address::from_str(
            &std::env::var("BATCH_SUBMITTER_ADDRESS")
                .expect("BATCH_SUBMITTER_ADDRESS must be set in .env"),
        )
        .expect("Invalid BATCH_SUBMITTER_ADDRESS");
        let sequencer = Address::from_str(
            &std::env::var("SEQUENCER_ADDRESS").expect("SEQUENCER_ADDRESS must be set in .env"),
        )
        .expect("Invalid SEQUENCER_ADDRESS");

        // Collect all data for the batch
        let mut receivers = Vec::new();
        let mut markets = Vec::new();
        let mut amounts = Vec::new();
        let mut selectors = Vec::new();
        let mut init_hashes = Vec::new();

        // Use the first event's journal and seal for the entire batch
        let journal_data = events[0].journal.clone().unwrap_or_default();
        let seal = events[0].seal.clone().unwrap_or_default();

        // Collect data for the batch
        for event in events {
            receivers.push(event.msg_sender.unwrap_or_default());
            markets.push(event.market.unwrap_or_default());
            amounts.push(event.amount.unwrap_or_default());
            selectors.push(match chain_id {
                chain_id if chain_id == malda_rs::constants::LINEA_CHAIN_ID as u32 => {
                    match event.target_function.as_deref().unwrap_or("outHere") {
                        "outHere" => Bytes4::from_slice(OUT_HERE_SELECTOR_FB4),
                        "mintExternal" => Bytes4::from_slice(MINT_EXTERNAL_SELECTOR_FB4),
                        "repayExternal" => Bytes4::from_slice(REPAY_EXTERNAL_SELECTOR_FB4),
                        method => {
                            error!("Invalid transaction method for Linea: {}", method);
                            return Err(eyre::eyre!("Invalid method for Linea: {}", method));
                        }
                    }
                }
                _ => Bytes4::from_slice(OUT_HERE_SELECTOR_FB4),
            });
            init_hashes.push(event.tx_hash.into());
        }

        let min_amounts_out = vec![U256::from(0); amounts.len()];

        let msg = BatchProcessMsg {
            receivers,
            journalData: journal_data,
            seal,
            mTokens: markets,
            amounts,
            minAmountsOut: min_amounts_out,
            selectors: selectors.clone(),
            initHashes: init_hashes,
            startIndex: U256::from(start_idx),
        };

        info!("Selectors: {:?}", selectors.clone());

        info!(
            "Preparing batch transaction for chain {}: start_idx={}, journal_size={}, seal_size={}, markets={:?}, tx_count={}",
            chain_id,
            start_idx,
            msg.journalData.len(),
            msg.seal.len(),
            msg.mTokens,
            events.len()
        );

        // Retry loop for transaction submission
        let mut retry_count = 0u128;
        let mut tx_hash = None;
        let mut last_error = None;

        let provider = Self::get_provider_for_chain(chain_id, chain_config).await?;
        let nonce = provider.get_transaction_count(sequencer).await.unwrap();
        let batch_submitter = IBatchSubmitter::new(batch_submitter, provider.clone());

        let _batch_submitted_at = Utc::now();

        while retry_count < chain_config.max_retries {
            // Get the provider for this chain

            info!(
                "Starting retry attempt {}/{} for chain {}",
                retry_count + 1,
                chain_config.max_retries,
                chain_id
            );

            // Create the action for this attempt
            let action = batch_submitter.batchProcess(msg.clone()).from(sequencer);

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
                    // Wait before retrying
                    info!(
                        "Waiting {:?} before retry attempt {}/{}",
                        chain_config.retry_delay,
                        retry_count + 1,
                        chain_config.max_retries
                    );
                    sleep(chain_config.retry_delay).await;
                    retry_count += 1;
                    continue;
                }
            };

            let max_fee_per_gas =
                max_fee_per_gas * (1 + retry_count) * (100 + percent_increase_per_retry_numerator)
                    / 100;
            let max_priority_fee_per_gas = max_priority_fee_per_gas
                * (1 + retry_count)
                * (100 + percent_increase_per_retry_numerator)
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
                .max_fee_per_gas(max_fee_per_gas)
                .max_priority_fee_per_gas(max_priority_fee_per_gas)
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
                                    info!(
                                            "Previous transaction was successful for chain {}: hash={:?}, gas_used={}, duration={:?}",
                                            chain_id,
                                            receipt.transaction_hash,
                                            receipt.gas_used(),
                                            batch_start.elapsed()
                                        );
                                    return Ok(prev_hash);
                                }
                            }
                        } else {
                            // If we don't have a tx_hash, just treat as success and return a zero hash
                            info!("No tx_hash available, but got 'nonce too low'. Treating as success.");
                            return Ok(TxHash::ZERO);
                        }
                    }

                    error!(
                        "Transaction submission failed for chain {}: {}",
                        chain_id, e
                    );
                    last_error = Some(error_str);
                    // Wait before retrying
                    info!(
                        "Waiting {:?} before retry attempt {}/{}",
                        chain_config.retry_delay,
                        retry_count + 1,
                        chain_config.max_retries
                    );
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
            match pending_tx
                .with_timeout(Some(chain_config.tx_timeout))
                .watch()
                .await
            {
                Ok(hash) => {
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
                            "Batch transaction successful for chain {}: hash={:?}, gas_used={}, duration={:?}",
                            chain_id,
                            receipt.transaction_hash,
                            receipt.gas_used(),
                            batch_start.elapsed()
                        );

                        break;
                    } else {
                        // Transaction reverted
                        error!(
                            "Transaction reverted for chain {}: hash={:?}",
                            chain_id, hash
                        );
                        break;
                    }
                }
                Err(e) => {
                    // Transaction failed or timed out
                    error!(
                        "Transaction failed for chain {}: error={}, duration={:?}, hash={:?}",
                        chain_id,
                        e,
                        batch_start.elapsed(),
                        tx_hash
                    );

                    // Check if the transaction was actually included despite the timeout
                    if let Some(hash) = tx_hash {
                        if let Ok(Some(receipt)) = provider.get_transaction_receipt(hash).await {
                            info!("Transaction was actually included despite timeout for chain {}: hash={:?}", chain_id, hash);
                            if receipt.status() == true {
                                info!(
                                    "Batch transaction successful for chain {}: hash={:?}, gas_used={}, duration={:?}",
                                    chain_id,
                                    receipt.transaction_hash,
                                    receipt.gas_used(),
                                    batch_start.elapsed()
                                );
                                break;
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
                        // Wait before retrying
                        info!(
                            "Waiting {:?} before retry attempt {}/{}",
                            chain_config.retry_delay,
                            retry_count + 1,
                            chain_config.max_retries
                        );
                        sleep(chain_config.retry_delay).await;
                    }
                }
            }

            retry_count += 1;
        }

        let batch_duration = batch_start.elapsed();
        info!(
            "Completed batch processing for chain {} in {:?}",
            chain_id, batch_duration
        );

        match tx_hash {
            Some(hash) => Ok(hash),
            None => {
                let error_msg = format!(
                    "Failed to submit transaction for chain {} after {} retries: {}",
                    chain_id,
                    chain_config.max_retries,
                    last_error.unwrap_or_else(|| "Unknown error".to_string())
                );
                error!("{}", error_msg);
                Err(eyre::eyre!(error_msg))
            }
        }
    }

    async fn get_provider_for_chain(
        chain_id: u32,
        chain_config: &ChainConfig,
    ) -> Result<ProviderType> {
        let url = Url::parse(&chain_config.rpc_url)?;
        let private_key_string = std::env::var("SEQUENCER_PRIVATE_KEY")
            .expect("SEQUENCER_PRIVATE_KEY must be set in .env");
        let signer: alloy::signers::local::PrivateKeySigner = private_key_string
            .parse()
            .expect("should parse private key");
        let wallet = EthereumWallet::from(signer);
        let provider = alloy::providers::ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(url);
        Ok(provider)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{primitives::U256, providers::ProviderBuilder};
    use eyre::Result;
    use url::Url;

    // Placeholder RPC URLs - Replace with actual ones

    // Placeholder RPC URLs - Replace with actual ones
    const ETH_SEPOLIA_RPC: &str = "http://localhost:8545"; // Example, replace if needed
    const OP_SEPOLIA_RPC: &str = "http://localhost:8547"; // Example, replace if needed
    const LINEA_SEPOLIA_RPC: &str = "http://localhost:8555"; // Example, replace if needed

    // Chain IDs - Replace if different ones are used in your constants
    const ETH_SEPOLIA_CHAIN_ID: u32 = 11155111;
    const OP_SEPOLIA_CHAIN_ID: u32 = 11155420;
    const LINEA_SEPOLIA_CHAIN_ID: u32 = 59141;

    #[tokio::test]
    async fn test_get_gas_prices() -> Result<()> {
        let chains = vec![
            ("ETH Sepolia", ETH_SEPOLIA_CHAIN_ID, ETH_SEPOLIA_RPC),
            ("OP Sepolia", OP_SEPOLIA_CHAIN_ID, OP_SEPOLIA_RPC),
            ("Linea Sepolia", LINEA_SEPOLIA_CHAIN_ID, LINEA_SEPOLIA_RPC),
        ];

        println!("\nFetching Gas Prices:");

        for (name, chain_id, rpc_url_str) in chains {
            println!("--- Chain: {} (ID: {}) ---", name, chain_id);
            let rpc_url = Url::parse(rpc_url_str)?;
            println!("Using RPC URL: {}", rpc_url);

            // Create a basic provider (no signer needed for get_gas_price)
            let provider = ProviderBuilder::new().on_http(rpc_url);

            match provider.get_gas_price().await {
                Ok(gas_price) => {
                    println!("Current Gas Price: {} wei", gas_price);
                }
                Err(e) => {
                    eprintln!("Failed to get gas price for {}: {}", name, e);
                    // Optionally decide if the test should fail here
                    // return Err(e.into());
                }
            }
            println!("-------------------------\n");
        }
        panic!("test");
        Ok(())
    }
}

/// Estimates gas for Linea and Linea Sepolia chains using the linea_estimateGas JSON-RPC method
/// Returns a tuple of (gas_limit, base_fee_per_gas, priority_fee_per_gas)
async fn linea_estimate_gas(provider: &ProviderType, data: Vec<u8>) -> Result<(u64, u128, u128)> {
    let batch_submitter = Address::from_str(
        &std::env::var("BATCH_SUBMITTER_ADDRESS")
            .expect("BATCH_SUBMITTER_ADDRESS must be set in .env"),
    )
    .expect("Invalid BATCH_SUBMITTER_ADDRESS");
    let sequencer = Address::from_str(
        &std::env::var("SEQUENCER_ADDRESS").expect("SEQUENCER_ADDRESS must be set in .env"),
    )
    .expect("Invalid SEQUENCER_ADDRESS");
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
