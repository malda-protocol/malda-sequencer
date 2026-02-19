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

//! # Event Proof Ready Checker Module
//!
//! This module provides a proof readiness checker that monitors block numbers
//! across multiple chains and determines when events are ready for proof generation.
//! It handles special cases like Ethereum block numbers via Optimism L1 block contracts.
//!
//! ## Key Features:
//! - **Multi-Chain Block Monitoring**: Tracks block numbers across multiple chains
//! - **Proof Readiness Detection**: Determines when events are ready for proof generation
//! - **Special Chain Handling**: Handles L1/L2 chain differences and Optimism L1 blocks
//! - **Provider Fallback**: Uses provider helper for reliable blockchain connections
//! - **Database Integration**: Updates event status in the database
//! - **Concurrent Processing**: Handles multiple chains simultaneously
//!
//! ## Architecture:
//! ```
//! EventProofReadyChecker::new()
//! ├── Block Number Monitoring: Tracks current block numbers per chain
//! ├── Proof Readiness Check: Determines when events are ready for proofs
//! ├── Database Updates: Updates event status in the database
//! ├── Special Chain Handling: Handles L1/L2 differences
//! ├── Provider Management: Uses provider helper for connections
//! └── Concurrent Processing: Multiple chains processed in parallel
//! ```
//!
//! ## Workflow:
//! 1. **Initialization**: Sets up block number tracking for all configured chains
//! 2. **Block Monitoring**: Continuously polls block numbers from each chain
//! 3. **Readiness Detection**: Checks if events are ready for proof generation
//! 4. **Database Updates**: Updates event status when ready
//! 5. **Special Handling**: Manages L1/L2 chain differences and Optimism L1 blocks
//! 6. **Error Recovery**: Handles connection failures and provider fallbacks

use alloy::{primitives::Address, providers::Provider};
use eyre::Result;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info};

use crate::provider_helper::{ProviderConfig, ProviderState};
use crate::types::IL1Block::new;
use malda_rs::constants::{
    ETHEREUM_CHAIN_ID, ETHEREUM_SEPOLIA_CHAIN_ID, OPTIMISM_CHAIN_ID, OPTIMISM_SEPOLIA_CHAIN_ID,
};
use sequencer::database::Database;

// Define a type for the block number map
type BlockNumberMap = HashMap<u64, AtomicI32>;

// Create a lazy static block number map
lazy_static! {
    static ref BLOCK_NUMBERS: Mutex<BlockNumberMap> = Mutex::new(BlockNumberMap::new());
}

/// Configuration for a single chain in the event proof ready checker
///
/// This struct contains all necessary parameters for monitoring
/// block numbers on a specific blockchain network, including
/// RPC endpoints, chain type, and timing parameters.
#[derive(Clone)]
pub struct ChainConfig {
    /// Unique identifier for the blockchain network
    pub chain_id: u64,
    /// Primary RPC URL for blockchain connection
    pub rpc_url: String,
    /// Fallback RPC URL in case primary fails
    pub fallback_rpc_url: String,
    /// Maximum allowed delay for block freshness in seconds
    pub max_block_delay_secs: u64,
}

/// L1 block contract address for Optimism
const L1_BLOCK_ADDRESS_OPTIMISM: &str = "0x4200000000000000000000000000000000000015";

/// Manages block number tracking and event proof readiness across multiple chains
///
/// The event proof ready checker continuously monitors block numbers on different
/// chains and updates the database when events are ready for proof generation.
/// It handles special cases like Ethereum block numbers via Optimism L1 block contracts.
///
/// ## Key Responsibilities:
///
/// - **Block Number Monitoring**: Tracks current block numbers for all configured chains
/// - **Proof Readiness Detection**: Determines when events are ready for proof generation
/// - **Database Integration**: Updates event status in the database
/// - **Special Chain Handling**: Manages L1/L2 chain differences and Optimism L1 blocks
/// - **Provider Management**: Uses provider helper for reliable blockchain connections
/// - **Concurrent Processing**: Handles multiple chains simultaneously
///
/// ## Block Number Management:
///
/// - **Thread-Safe Storage**: Uses atomic integers for thread-safe block number storage
/// - **Global Access**: Provides global access to block numbers via lazy static
/// - **Concurrent Updates**: Supports concurrent updates from multiple chains
/// - **Fallback Support**: Uses provider helper for reliable connections with fallback
pub struct EventProofReadyChecker;

impl EventProofReadyChecker {
    /// Creates and starts a new event proof ready checker
    ///
    /// This method initializes the checker and immediately starts
    /// the polling loop. It runs indefinitely until an error occurs.
    ///
    /// ## Initialization Process:
    ///
    /// 1. **Block Number Setup**: Initializes block number tracking for all chains
    /// 2. **Provider Configuration**: Sets up provider states for each chain
    /// 3. **Polling Loop**: Starts continuous block number monitoring
    /// 4. **Database Integration**: Connects to database for event status updates
    ///
    /// ## Concurrent Operations:
    ///
    /// - **Block Monitoring**: Continuously polls block numbers from each chain
    /// - **Readiness Detection**: Checks if events are ready for proof generation
    /// - **Database Updates**: Updates event status when ready
    /// - **Special Handling**: Manages L1/L2 chain differences and Optimism L1 blocks
    ///
    /// # Arguments
    /// * `db` - Database connection for event status updates
    /// * `poll_interval` - Interval between event readiness checks
    /// * `block_update_interval` - Interval between block number updates
    /// * `chain_configs` - Vector of chain configurations to monitor
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let db = Database::new("connection_string").await?;
    /// let chain_configs = vec![chain1_config, chain2_config];
    /// EventProofReadyChecker::new(db, poll_interval, block_update_interval, chain_configs).await?;
    /// ```
    pub async fn new(
        db: Database,
        poll_interval: Duration,
        block_update_interval: Duration,
        chain_configs: Vec<ChainConfig>,
    ) -> Result<()> {
        info!(
            "Starting event proof ready checker with {} chains",
            chain_configs.len()
        );

        // Initialize the block number map with default values for all supported chains
        {
            let mut block_numbers = BLOCK_NUMBERS.lock().unwrap();
            for config in &chain_configs {
                block_numbers.insert(config.chain_id, AtomicI32::new(0));
            }
        } // Release lock early

        // Initialize provider states for each chain
        let mut provider_states = HashMap::new();
        for config in chain_configs {
            let provider_config = ProviderConfig {
                primary_url: config.rpc_url.clone(),
                fallback_url: config.fallback_rpc_url.clone(),
                max_block_delay_secs: config.max_block_delay_secs,
                chain_id: config.chain_id,
                use_websocket: false, // Use HTTP for block number polling
            };
            provider_states.insert(config.chain_id, ProviderState::new(provider_config));
        }

        info!("Started polling for block number updates");

        // Main processing loop
        let mut interval = interval(block_update_interval);
        let mut iteration_count = 0;

        loop {
            interval.tick().await;

            // Update block numbers for all chains
            let mut current_block_map = HashMap::new();

            // Special handling for Ethereum via Optimism L1 block contract
            let optimism_chain_id = if std::env::var("ENVIRONMENT")
                .unwrap_or_else(|_| "testnet".to_string())
                == "mainnet"
            {
                OPTIMISM_CHAIN_ID
            } else {
                OPTIMISM_SEPOLIA_CHAIN_ID
            };

            // Get Ethereum block number via Optimism contract
            if let Some(optimism_state) = provider_states.get_mut(&optimism_chain_id) {
                if let Ok((optimism_provider, _)) = optimism_state.get_fresh_provider().await {
                    let l1_block_contract = new(
                        L1_BLOCK_ADDRESS_OPTIMISM.parse::<Address>().unwrap(),
                        optimism_provider,
                    );

                    if let Ok(block_number) = l1_block_contract.number().call().await {
                        let block_number_i32 = block_number.try_into().unwrap_or(i32::MAX);
                        let ethereum_chain_id = if std::env::var("ENVIRONMENT")
                            .unwrap_or_else(|_| "testnet".to_string())
                            == "mainnet"
                        {
                            ETHEREUM_CHAIN_ID
                        } else {
                            ETHEREUM_SEPOLIA_CHAIN_ID
                        };

                        // Update the block number in the map
                        {
                            let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                            if let Some(atomic) = block_numbers.get(&ethereum_chain_id) {
                                atomic.store(block_number_i32, Ordering::SeqCst);
                                debug!("Updated Ethereum block number: {}", block_number_i32);
                                current_block_map.insert(ethereum_chain_id, block_number_i32);
                            }
                        }
                    }
                }
            }

            // Update block numbers for other chains
            for (chain_id, state) in provider_states.iter_mut() {
                // Skip Ethereum as it's handled above
                let ethereum_chain_id = if std::env::var("ENVIRONMENT")
                    .unwrap_or_else(|_| "testnet".to_string())
                    == "mainnet"
                {
                    ETHEREUM_CHAIN_ID
                } else {
                    ETHEREUM_SEPOLIA_CHAIN_ID
                };

                if *chain_id == ethereum_chain_id {
                    continue;
                }

                // Get block number for this chain
                if let Ok((provider, _)) = state.get_fresh_provider().await {
                    if let Ok(block_number) = provider.get_block_number().await {
                        let block_number_i32 = block_number as i32;

                        // Update the block number in the map
                        {
                            let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                            if let Some(atomic) = block_numbers.get(chain_id) {
                                atomic.store(block_number_i32, Ordering::SeqCst);
                                debug!(
                                    "Updated block number for chain {}: {}",
                                    chain_id, block_number_i32
                                );
                                current_block_map.insert(*chain_id, block_number_i32);
                            }
                        }
                    }
                }
            }

            if current_block_map.is_empty() {
                debug!("No valid current block numbers available yet, skipping DB update.");
                continue;
            }

            if iteration_count % 60 == 0 {
                info!("Current block map: {:?}", current_block_map);
            }

            // Update database with new block numbers
            match db.update_events_to_ready_status(&current_block_map).await {
                Ok(_) => { /* Success logged within the DB function */ }
                Err(e) => {
                    error!("Failed to update events to ready status: {:?}", e);
                }
            }

            // Sleep for poll interval before checking for events again
            tokio::time::sleep(poll_interval).await;
            iteration_count += 1;
        }
    }
}
