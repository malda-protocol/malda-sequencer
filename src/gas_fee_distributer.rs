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

//! # Gas Fee Distributer Module
//!
//! This module provides a gas fee distributer that manages ETH balances across multiple chains,
//! automatically harvesting gas fees from markets, distributing them to sequencers, and handling
//! cross-chain rebalancing via bridge protocols like Accross.
//!
//! ## Key Features:
//! - **Multi-Chain Balance Monitoring**: Tracks ETH balances across multiple chains
//! - **Automatic Gas Fee Harvesting**: Withdraws gas fees from market contracts
//! - **Sequencer Distribution**: Sends harvested fees to sequencer addresses
//! - **Cross-Chain Rebalancing**: Bridges ETH between chains when needed
//! - **Balance Thresholds**: Configurable minimum and target balance thresholds
//! - **Bridge Integration**: Supports Accross protocol for cross-chain transfers
//!
//! ## Architecture:
//! ```
//! GasFeeDistributer::new()
//! ├── Balance Polling: Continuously monitors balances across chains
//! ├── Gas Fee Harvesting: Withdraws fees from market contracts
//! ├── Sequencer Distribution: Sends fees to sequencer addresses
//! ├── Cross-Chain Rebalancing: Bridges ETH between chains
//! ├── Bridge Integration: Uses Accross protocol for transfers
//! └── Balance Updates: Tracks all balance changes
//! ```
//!
//! ## Workflow:
//! 1. **Initialization**: Sets up balance tracking for all chains and markets
//! 2. **Balance Polling**: Continuously monitors distributor, sequencer, and market balances
//! 3. **Harvesting**: Withdraws gas fees from markets when thresholds are met
//! 4. **Distribution**: Sends harvested fees to sequencers based on target balances
//! 5. **Cross-Chain Rebalancing**: Bridges ETH between chains when needed
//! 6. **Settlement Tracking**: Monitors bridge settlement times

use crate::types::{IAccross, IMaldaMarket};
use alloy::network::EthereumWallet;
use alloy::network::TransactionBuilder;
use alloy::primitives::{Address, Bytes, U256};
use alloy::providers::Provider;
use alloy::providers::ProviderBuilder;
use alloy::rpc::types::TransactionRequest;
use alloy::signers::local::PrivateKeySigner;
use alloy::transports::http::reqwest::Url;
use malda_rs::constants::*;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;
use tokio::time;
use tokio::time::{timeout, Duration};
use tracing::{error, info};

use crate::provider_helper::{ProviderConfig, ProviderState, ProviderType};

/// Gas limit for transactions
pub const GAS_LIMIT: u64 = 100000u64;
/// Wait time for transaction confirmation (in seconds)
pub const WAIT_TIME: u64 = 24; // in seconds, two mainnet blocks

/// Gas fee distributer that manages ETH balances across multiple chains
///
/// This component handles the complete lifecycle of gas fee management:
/// - Monitors balances across multiple chains and markets
/// - Harvests gas fees from market contracts when thresholds are met
/// - Distributes harvested fees to sequencer addresses
/// - Performs cross-chain rebalancing via bridge protocols
/// - Tracks settlement times for bridge transactions
///
/// ## Balance Management
///
/// - **Market Balances**: Tracks ETH balances for each market on each chain
/// - **Distributor Balances**: Monitors distributor ETH balances
/// - **Sequencer Balances**: Tracks sequencer ETH balances
/// - **Threshold Management**: Uses configurable minimum and target balances
///
/// ## Cross-Chain Operations
///
/// - **Bridge Integration**: Uses Accross protocol for cross-chain transfers
/// - **Settlement Tracking**: Monitors bridge settlement times
/// - **Rebalancing Logic**: Automatically bridges ETH when needed
/// - **Fee Calculation**: Handles bridge fees and output amount calculations
pub struct GasFeeDistributer {
    /// List of chain IDs to monitor
    pub chains: Vec<u64>,
    /// Mapping of chain ID to list of market addresses
    pub markets_per_chain: HashMap<u64, Vec<Address>>,
    /// Mapping of chain ID to RPC URL
    pub rpc_urls: HashMap<u64, String>,
    /// Mapping of chain ID to market balances (market -> ETH balance in wei)
    pub balances: HashMap<u64, HashMap<Address, U256>>,
    /// Mapping of chain ID to distributor ETH balance in wei
    pub distributor_balances: HashMap<u64, U256>,
    /// Mapping of chain ID to sequencer ETH balance in wei
    pub sequencer_balances: HashMap<u64, U256>,
    /// Polling interval in seconds
    pub poll_interval_secs: u64,
    /// Private key for signing transactions
    pub private_key: String,
    /// Public address of the distributor
    pub public_address: Address,
    /// Sequencer address to receive distributed fees
    pub sequencer_address: Address,
    /// Minimum sequencer balance per chain
    pub minimum_sequencer_balance_per_chain: HashMap<u64, U256>,
    /// Minimum distributor balance per chain
    pub min_distributor_balance_per_chain: HashMap<u64, U256>,
    /// Target sequencer balance per chain
    pub target_sequencer_balance_per_chain: HashMap<u64, U256>,
    /// Minimum harvest balance per chain
    pub minimum_harvest_balance_per_chain: HashMap<u64, U256>,
    /// Chains that need cross-chain rebalancing
    pub cross_chain_rebalance: Vec<u64>,
    /// Bridge fee percentage for cross-chain transfers
    pub bridge_fee_percentage: u32,
    /// Minimum amount to bridge
    pub min_amount_to_bridge: U256,
    /// Accross settlement time in seconds
    pub accross_settlement_time: u64,
    /// Timer for tracking Accross settlement
    pub accross_timer: Option<std::time::Instant>,
}

impl GasFeeDistributer {
    /// Creates and starts a new gas fee distributer
    ///
    /// This method initializes the gas fee distributer and immediately starts
    /// the balance polling and distribution process. It sets up balance tracking
    /// for all configured chains and markets, then begins the continuous
    /// monitoring and distribution loop.
    ///
    /// ## Initialization Process
    ///
    /// 1. **Balance Tracking Setup**: Initializes balance tracking for all chains
    /// 2. **Market Balance Initialization**: Sets up market balance tracking
    /// 3. **Distributor Balance Setup**: Initializes distributor balance tracking
    /// 4. **Sequencer Balance Setup**: Initializes sequencer balance tracking
    /// 5. **Configuration Loading**: Loads Accross settlement time from environment
    ///
    /// ## Continuous Operations
    ///
    /// - **Balance Polling**: Continuously monitors all balances
    /// - **Gas Fee Harvesting**: Withdraws fees when thresholds are met
    /// - **Sequencer Distribution**: Sends fees to sequencers
    /// - **Cross-Chain Rebalancing**: Bridges ETH between chains
    ///
    /// # Arguments
    /// * `chains` - List of chain IDs to monitor
    /// * `markets_per_chain` - Mapping of chain ID to market addresses
    /// * `rpc_urls` - Mapping of chain ID to RPC URL
    /// * `poll_interval_secs` - Polling interval in seconds
    /// * `private_key` - Private key for signing transactions
    /// * `public_address` - Distributor public address
    /// * `sequencer_address` - Sequencer address to receive fees
    /// * `minimum_sequencer_balance_per_chain` - Minimum sequencer balance per chain
    /// * `min_distributor_balance_per_chain` - Minimum distributor balance per chain
    /// * `target_sequencer_balance_per_chain` - Target sequencer balance per chain
    /// * `minimum_harvest_balance_per_chain` - Minimum harvest balance per chain
    /// * `bridge_fee_percentage` - Bridge fee percentage for cross-chain transfers
    /// * `min_amount_to_bridge` - Minimum amount to bridge
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let distributer = GasFeeDistributer::new(
    ///     chains,
    ///     markets_per_chain,
    ///     rpc_urls,
    ///     60, // poll every 60 seconds
    ///     private_key,
    ///     public_address,
    ///     sequencer_address,
    ///     minimum_sequencer_balance_per_chain,
    ///     min_distributor_balance_per_chain,
    ///     target_sequencer_balance_per_chain,
    ///     minimum_harvest_balance_per_chain,
    ///     5, // 5% bridge fee
    ///     min_amount_to_bridge,
    /// ).await?;
    /// ```
    #[allow(clippy::too_many_arguments)]
    pub async fn new(
        chains: Vec<u64>,
        markets_per_chain: HashMap<u64, Vec<Address>>,
        rpc_urls: HashMap<u64, String>,
        poll_interval_secs: u64,
        private_key: String,
        public_address: Address,
        sequencer_address: Address,
        minimum_sequencer_balance_per_chain: HashMap<u64, U256>,
        min_distributor_balance_per_chain: HashMap<u64, U256>,
        target_sequencer_balance_per_chain: HashMap<u64, U256>,
        minimum_harvest_balance_per_chain: HashMap<u64, U256>,
        bridge_fee_percentage: u32,
        min_amount_to_bridge: U256,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Initialize balance tracking structures
        let mut balances = HashMap::new();
        let mut distributor_balances = HashMap::new();
        let mut sequencer_balances = HashMap::new();

        for &chain_id in &chains {
            let mut market_balances = HashMap::new();
            if let Some(markets) = markets_per_chain.get(&chain_id) {
                for &market in markets {
                    market_balances.insert(market, U256::from(0u64));
                }
            }
            balances.insert(chain_id, market_balances);
            distributor_balances.insert(chain_id, U256::from(0u64));
            sequencer_balances.insert(chain_id, U256::from(0u64));
        }

        let cross_chain_rebalance = Vec::new();
        let accross_settlement_time = std::env::var("ACCROSS_SETTLEMENT_TIME")
            .expect("ACCROSS_SETTLEMENT_TIME must be set in .env")
            .parse()
            .expect("ACCROSS_SETTLEMENT_TIME must be a u64");
        let accross_timer = None;

        let mut distributer = GasFeeDistributer {
            chains,
            markets_per_chain,
            rpc_urls,
            balances,
            distributor_balances,
            sequencer_balances,
            poll_interval_secs,
            private_key,
            public_address,
            sequencer_address,
            minimum_sequencer_balance_per_chain,
            min_distributor_balance_per_chain,
            target_sequencer_balance_per_chain,
            minimum_harvest_balance_per_chain,
            cross_chain_rebalance,
            bridge_fee_percentage,
            min_amount_to_bridge,
            accross_settlement_time,
            accross_timer,
        };

        // Start the continuous polling and distribution process
        distributer.start_polling_balances().await;

        Ok(())
    }

    /// Starts the continuous polling process for balances across all configured chains
    ///
    /// This method sets up a polling interval and begins a loop that:
    /// 1. Polls balances for all configured chains
    /// 2. Updates the internal balance tracking structures
    /// 3. Distributes gas fees to sequencers when thresholds are met
    /// 4. Handles cross-chain rebalancing
    ///
    /// The polling interval is controlled by `self.poll_interval_secs`.
    pub async fn start_polling_balances(&mut self) {
        let mut interval = time::interval(std::time::Duration::from_secs(self.poll_interval_secs));

        loop {
            info!("Gas Fee Balances: {:?}", self.balances);
            interval.tick().await;

            // Poll balances for all configured chains
            let chains_to_poll = self.chains.clone();
            for &chain_id in &chains_to_poll {
                self.poll_chain_balances(chain_id).await;
            }

            // Distribute gas fees after polling all chains
            self.distribute_gas_fees().await;
        }
    }

    /// Polls balances for a specific chain
    ///
    /// This method updates distributor, sequencer, and market balances for a given chain
    /// using the provider helper for reliable connections.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to poll balances for
    async fn poll_chain_balances(&mut self, chain_id: u64) {
        let rpc_url = match self.rpc_urls.get(&chain_id) {
            Some(url) => url,
            None => {
                error!("No RPC URL for chain {}", chain_id);
                return;
            }
        };

        // Create provider state for this chain
        let provider_config = ProviderConfig {
            primary_url: rpc_url.clone(),
            fallback_url: rpc_url.clone(), // Use same URL as fallback for now
            max_block_delay_secs: 300,     // 5 minutes
            chain_id,
            use_websocket: false, // Use HTTP for balance polling
        };

        let mut provider_state = ProviderState::new(provider_config);

        // Get fresh provider with fallback logic
        let (provider, _is_fallback) = match provider_state.get_fresh_provider().await {
            Ok(result) => result,
            Err(e) => {
                error!(
                    "Failed to get fresh provider for chain {}: {:?}",
                    chain_id, e
                );
                return;
            }
        };

        // Get distributor balance
        match provider.get_balance(self.public_address).await {
            Ok(balance) => {
                self.distributor_balances.insert(chain_id, balance);
            }
            Err(e) => {
                error!(
                    "Failed to get distributor balance on chain {}: {}",
                    chain_id, e
                );
            }
        }

        // Get sequencer balance
        match provider.get_balance(self.sequencer_address).await {
            Ok(balance) => {
                self.sequencer_balances.insert(chain_id, balance);
            }
            Err(e) => {
                error!(
                    "Failed to get sequencer balance on chain {}: {}",
                    chain_id, e
                );
            }
        }

        // Get market balances
        if let Some(markets) = self.markets_per_chain.get(&chain_id) {
            for &market in markets {
                match provider.get_balance(market).await {
                    Ok(balance) => {
                        if let Some(chain_balances) = self.balances.get_mut(&chain_id) {
                            chain_balances.insert(market, balance);
                        }
                    }
                    Err(e) => {
                        error!(
                            "Failed to get balance for market {:?} on chain {}: {}",
                            market, chain_id, e
                        );
                    }
                }
            }
        }
    }

    /// Distributes gas fees across configured chains
    ///
    /// This method:
    /// 1. Identifies chains that need to withdraw gas fees (below minimum threshold)
    /// 2. For each chain, it calls `withdraw_gas_fees` to withdraw fees
    /// 3. Then, it calls `send_to_sequencer` to send fees to sequencers
    /// 4. Finally, it performs cross-chain rebalancing if needed
    pub async fn distribute_gas_fees(&mut self) {
        // Collect chain_ids to process to avoid borrow checker issues
        let to_harvest: Vec<u64> = self
            .chains
            .iter()
            .copied()
            .filter(|&chain_id| {
                let sequencer_balance = self
                    .sequencer_balances
                    .get(&chain_id)
                    .cloned()
                    .unwrap_or(U256::from(0u64));
                let min_balance = self
                    .minimum_sequencer_balance_per_chain
                    .get(&chain_id)
                    .cloned()
                    .unwrap_or(U256::from(0u64));
                sequencer_balance < min_balance
            })
            .collect();

        // Process chains that need harvesting
        for chain_id in to_harvest {
            self.withdraw_gas_fees(chain_id).await;
            self.send_to_sequencer(chain_id).await;
        }

        // Cross-chain rebalance section
        // Wait for settlement time before starting cross-chain rebalance
        tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;

        let mut rebalance_pairs = Vec::new();
        for &chain_id in &self.cross_chain_rebalance {
            for &other_chain_id in &self.chains {
                if other_chain_id == chain_id {
                    continue;
                }
                rebalance_pairs.push((other_chain_id, chain_id));
            }
        }

        // Process cross-chain rebalancing
        for (src, target) in rebalance_pairs {
            if let Some(timer) = &self.accross_timer {
                if timer.elapsed().as_secs() < self.accross_settlement_time {
                    info!("Still waiting for settlement, skipping cross-chain rebalancing");
                    self.cross_chain_rebalance.clear();
                    return;
                } else {
                    self.accross_timer = None;
                }
            }
            self.withdraw_gas_fees(src).await;
            self.send_to_sequencer(src).await;
            self.cross_chain_gas_distribution(src, target).await;
        }
    }

    /// Withdraws gas fees from markets on a given chain
    ///
    /// This method:
    /// 1. Creates a provider for the specified chain
    /// 2. Retrieves the minimum harvest balance for the chain
    /// 3. Iterates through markets on the chain, withdrawing fees if the market balance
    ///    is above the minimum harvest balance
    /// 4. For each market, it calls `IMaldaMarket::withdrawGasFees` to initiate the withdrawal
    /// 5. Sets gas parameters for the transaction
    /// 6. Sends the transaction and updates the distributor balance
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to withdraw gas fees from
    pub async fn withdraw_gas_fees(&mut self, chain_id: u64) {
        let Some(rpc_url) = self.rpc_urls.get(&chain_id) else {
            return;
        };

        // Create provider for this chain
        let provider = self.create_provider_for_chain(chain_id, rpc_url).await;
        if provider.is_none() {
            return;
        }
        let provider = provider.unwrap();

        let min_harvest = self
            .minimum_harvest_balance_per_chain
            .get(&chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));

        // Process each market on this chain
        if let Some(markets) = self.markets_per_chain.get(&chain_id) {
            for &market in markets {
                let market_balance = self
                    .balances
                    .get(&chain_id)
                    .and_then(|m| m.get(&market))
                    .cloned()
                    .unwrap_or(U256::from(0u64));

                // Skip if market balance is below minimum harvest threshold
                if market_balance < min_harvest {
                    continue;
                }

                // Create market contract instance
                let contract = IMaldaMarket::new(market, provider.clone());
                let mut call = contract.withdrawGasFees(self.public_address);

                // Set gas parameters for the transaction
                let gas_params = self.get_gas_parameters_for_chain(chain_id, &provider).await;
                if gas_params.is_none() {
                    continue;
                }
                let (base_fee_per_gas, priority_fee_per_gas) = gas_params.unwrap();

                call = call
                    .gas(GAS_LIMIT)
                    .max_fee_per_gas(base_fee_per_gas)
                    .max_priority_fee_per_gas(priority_fee_per_gas);

                // Send the withdrawal transaction
                match call.send().await {
                    Ok(pending_tx) => {
                        info!(
                            "Called withdrawGasFees on market {:?} for chain {}: tx_hash={:?}",
                            market,
                            chain_id,
                            pending_tx.tx_hash()
                        );
                    }
                    Err(e) => {
                        error!(
                            "Failed to call withdrawGasFees on market {:?} for chain {}: {}",
                            market, chain_id, e
                        );
                    }
                }
            }
        }

        // Wait for transaction confirmation
        tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;

        // Update distributor balance after harvesting
        match provider.get_balance(self.public_address).await {
            Ok(balance) => {
                if let Some(balances) = self.distributor_balances.get_mut(&chain_id) {
                    *balances = balance;
                }
            }
            Err(e) => {
                error!(
                    "Failed to update distributor balance on chain {}: {}",
                    chain_id, e
                );
            }
        }
    }

    /// Sends ETH from the distributor to the sequencer on a given chain
    ///
    /// This method:
    /// 1. Creates a provider for the specified chain
    /// 2. Retrieves distributor and sequencer balances for the chain
    /// 3. Checks if the distributor balance is above the minimum distributor balance
    /// 4. Calculates the amount needed to reach the target sequencer balance
    /// 5. If needed, it calculates the amount to send
    /// 6. Builds the transaction with appropriate gas parameters
    /// 7. Sends the transaction and updates balances
    /// 8. If the sequencer is still below minimum, adds to cross_chain_rebalance
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to send fees to sequencer on
    pub async fn send_to_sequencer(&mut self, chain_id: u64) {
        let Some(rpc_url) = self.rpc_urls.get(&chain_id) else {
            return;
        };

        // Create provider for this chain
        let provider = self.create_provider_for_chain(chain_id, rpc_url).await;
        if provider.is_none() {
            return;
        }
        let provider = provider.unwrap();

        // Get current balances and thresholds
        let distributor_balance = self
            .distributor_balances
            .get(&chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        let sequencer_balance = self
            .sequencer_balances
            .get(&chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        let min_distributor = self
            .min_distributor_balance_per_chain
            .get(&chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        let target_sequencer = self
            .target_sequencer_balance_per_chain
            .get(&chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        let _min_sequencer = self
            .minimum_sequencer_balance_per_chain
            .get(&chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));

        // Check if distributor balance is sufficient
        if distributor_balance <= min_distributor {
            info!("Distributor balance too low to send on chain {}", chain_id);
            self.cross_chain_rebalance.push(chain_id);
            return;
        }

        // Calculate amount needed to reach target sequencer balance
        let needed = if target_sequencer > sequencer_balance {
            target_sequencer - sequencer_balance
        } else {
            U256::from(0u64)
        };

        // Skip if sequencer is already at or close to target
        if needed < _min_sequencer / U256::from(5u64) {
            info!(
                "Sequencer already at or above target (or close enough) on chain {}",
                chain_id
            );
            return;
        }

        // Calculate amount to send
        let available = distributor_balance - min_distributor;
        let to_send = if available >= needed {
            needed
        } else {
            available
        };

        if to_send.is_zero() {
            info!("Nothing to send to sequencer on chain {}", chain_id);
            return;
        }

        // Build the transaction
        let mut tx = TransactionRequest::default()
            .with_from(self.public_address)
            .with_to(self.sequencer_address)
            .with_value(to_send);

        // Set gas parameters for the transaction
        let gas_params = self.get_gas_parameters_for_chain(chain_id, &provider).await;
        if gas_params.is_none() {
            return;
        }
        let (base_fee_per_gas, priority_fee_per_gas) = gas_params.unwrap();

        tx = tx
            .gas_limit(GAS_LIMIT)
            .max_fee_per_gas(base_fee_per_gas)
            .max_priority_fee_per_gas(priority_fee_per_gas);

        // Send the transaction
        match provider.send_transaction(tx).await {
            Ok(pending_tx) => {
                info!(
                    "Sent {} wei to sequencer on chain {}: tx_hash={:?}",
                    to_send,
                    chain_id,
                    pending_tx.tx_hash()
                );
            }
            Err(e) => {
                error!("Failed to send to sequencer on chain {}: {}", chain_id, e);
                return;
            }
        }

        // Wait for transaction confirmation
        tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;

        // Update balances after sending
        self.update_balances_after_transaction(chain_id, &provider)
            .await;

        // Check if sequencer is still below minimum and add to cross-chain rebalance if needed
        let updated_seq = self
            .sequencer_balances
            .get(&chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        if updated_seq < _min_sequencer {
            if !self.cross_chain_rebalance.contains(&chain_id) {
                self.cross_chain_rebalance.push(chain_id);
            }
        }
    }

    /// Performs cross-chain gas distribution
    ///
    /// This method:
    /// 1. Retrieves distributor and sequencer balances for the source chain
    /// 2. Retrieves distributor and sequencer balances for the target chain
    /// 3. Calculates how much gas fees are available on the source chain
    /// 4. Calculates how much is needed on the target chain
    /// 5. Determines the amount to send
    /// 6. If the amount is sufficient, it calls `bridge_via_accross` to initiate the bridge
    ///
    /// # Arguments
    /// * `src_chain_id` - Source chain ID for the cross-chain transfer
    /// * `target_chain_id` - Target chain ID for the cross-chain transfer
    pub async fn cross_chain_gas_distribution(&mut self, src_chain_id: u64, target_chain_id: u64) {
        // Get distributor balance and min_distributor for src_chain_id
        let distributor_balance = self
            .distributor_balances
            .get(&src_chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        let min_distributor = self
            .min_distributor_balance_per_chain
            .get(&src_chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));

        // Get sequencer balance, min_sequencer, and target_sequencer for target_chain_id
        let sequencer_balance = self
            .sequencer_balances
            .get(&target_chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        let _min_sequencer = self
            .minimum_sequencer_balance_per_chain
            .get(&target_chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));
        let target_sequencer = self
            .target_sequencer_balance_per_chain
            .get(&target_chain_id)
            .cloned()
            .unwrap_or(U256::from(0u64));

        // Calculate how much is available to send from src
        let available = if distributor_balance > min_distributor {
            distributor_balance - min_distributor
        } else {
            U256::from(0u64)
        };

        // Calculate how much is needed on target
        let needed = if target_sequencer > sequencer_balance {
            target_sequencer - sequencer_balance
        } else {
            U256::from(0u64)
        };

        // Determine to_send as min(available, needed)
        let to_send = if available < needed {
            available
        } else {
            needed
        };

        if to_send < self.min_amount_to_bridge {
            info!(
                "Not enough gas fees to bridge on chain {} to chain {}",
                src_chain_id, target_chain_id
            );
            return;
        }

        self.bridge_via_accross(src_chain_id, target_chain_id, to_send)
            .await;
    }

    /// Bridges ETH between two chains via the Accross protocol
    ///
    /// This method:
    /// 1. Determines the spoke pool address for the source chain
    /// 2. Creates a provider for the source chain
    /// 3. Creates an `IAccross` contract instance
    /// 4. Retrieves wrapped native token addresses for input and output
    /// 5. Retrieves current time and buffer from the contract
    /// 6. Calculates output amount as input amount minus bridge fee
    /// 7. Sets up deadlines and quote timestamp
    /// 8. Calls `IAccross::depositV3` to initiate the bridge
    /// 9. Sets gas parameters for the transaction
    /// 10. Sends the transaction and watches for confirmation
    ///
    /// # Arguments
    /// * `src_chain_id` - Source chain ID for the bridge
    /// * `target_chain_id` - Target chain ID for the bridge
    /// * `amount` - Amount to bridge
    pub async fn bridge_via_accross(
        &mut self,
        src_chain_id: u64,
        target_chain_id: u64,
        amount: U256,
    ) {
        // Determine the spoke pool address for the source chain
        let spoke_pool_address = match src_chain_id {
            ETHEREUM_CHAIN_ID => env::var("ETHEREUM_SPOKE_POOL_ADDRESS")
                .expect("ETHEREUM_SPOKE_POOL_ADDRESS must be set in .env"),
            BASE_CHAIN_ID => env::var("BASE_SPOKE_POOL_ADDRESS")
                .expect("BASE_SPOKE_POOL_ADDRESS must be set in .env"),
            LINEA_CHAIN_ID => env::var("LINEA_SPOKE_POOL_ADDRESS")
                .expect("LINEA_SPOKE_POOL_ADDRESS must be set in .env"),
            _ => {
                error!("Unsupported source chain ID: {}", src_chain_id);
                return;
            }
        };

        // Get the RPC URL for the source chain
        let Some(rpc_url) = self.rpc_urls.get(&src_chain_id) else {
            return;
        };

        // Create provider for the source chain
        let provider = self
            .create_signed_provider_for_chain(src_chain_id, rpc_url)
            .await;
        if provider.is_none() {
            return;
        }
        let provider = provider.unwrap();

        // Convert addresses from String to Address
        let spoke_pool_address = match Address::from_str(&spoke_pool_address) {
            Ok(addr) => addr,
            Err(e) => {
                error!("Invalid spoke pool address: {}", e);
                return;
            }
        };

        // Create Accross contract instance
        let contract = IAccross::new(spoke_pool_address, provider.clone());

        // Get wrapped native token addresses for input and output
        let wrapped_native_token_in = self.get_wrapped_native_token_address(src_chain_id).await;
        let wrapped_native_token_out = self.get_wrapped_native_token_address(target_chain_id).await;
        if wrapped_native_token_in.is_none() || wrapped_native_token_out.is_none() {
            return;
        }
        let wrapped_native_token_in = wrapped_native_token_in.unwrap();
        let wrapped_native_token_out = wrapped_native_token_out.unwrap();

        // Get quote timestamp and buffer from contract
        let current_time = match contract.getCurrentTime().call().await {
            Ok(t) => t,
            Err(e) => {
                error!("[BRIDGE] Failed to get current time from contract: {}", e);
                return;
            }
        };

        let buffer = match contract.depositQuoteTimeBuffer().call().await {
            Ok(b) => b,
            Err(e) => {
                error!(
                    "[BRIDGE] Failed to get depositQuoteTimeBuffer from contract: {}",
                    e
                );
                return;
            }
        };

        let quote_timestamp = current_time; // Safe to use current time as quoteTimestamp
        let fill_deadline = quote_timestamp + 120;
        let exclusivity_deadline = 0u32;

        info!(
            "[BRIDGE] Using quote_timestamp={}, buffer={}, fill_deadline={}",
            quote_timestamp, buffer, fill_deadline
        );

        // Calculate output amount as amount minus the bridge fee
        let fee = amount * U256::from(self.bridge_fee_percentage) / U256::from(100u32);
        let output_amount = if amount > fee {
            amount - fee
        } else {
            U256::from(0u64)
        };

        // Convert target_chain_id to U256
        let target_chain_id_u256 = U256::from(target_chain_id);

        info!(
            "[BRIDGE] Preparing to bridge via Accross: src_chain_id={}, target_chain_id={}, amount={}, sender={:?}, spoke_pool_address={:?}, input_token={:?}, output_token={:?}, output_amount={}, fill_deadline={}, quote_timestamp={}",
            src_chain_id,
            target_chain_id,
            amount,
            self.public_address,
            spoke_pool_address,
            wrapped_native_token_in,
            wrapped_native_token_out,
            output_amount,
            fill_deadline,
            quote_timestamp
        );

        // Create the bridge transaction
        let call = contract.depositV3(
            self.public_address,      // depositor
            self.public_address,      // recipient (distributor on target chain)
            wrapped_native_token_in,  // inputToken (WETH address)
            wrapped_native_token_out, // outputToken (WETH address)
            amount,                   // inputAmount
            output_amount,            // outputAmount (amount minus fee)
            target_chain_id_u256,     // destinationChainId
            Address::ZERO,            // exclusiveRelayer
            quote_timestamp,          // quoteTimestamp
            fill_deadline,            // fillDeadline
            exclusivity_deadline,     // exclusivityDeadline
            Bytes::from_static(b""),  // message
        );

        info!(
            "[BRIDGE] Sending depositV3 transaction with msg.value={}...",
            amount
        );

        // Set gas parameters for the transaction
        let gas_params = self
            .get_gas_parameters_for_chain(src_chain_id, &provider)
            .await;
        if gas_params.is_none() {
            return;
        }
        let (base_fee_per_gas, priority_fee_per_gas) = gas_params.unwrap();

        let call = call
            .gas(GAS_LIMIT)
            .max_fee_per_gas(base_fee_per_gas)
            .max_priority_fee_per_gas(priority_fee_per_gas);

        // Send the bridge transaction
        match call.value(amount).send().await {
            Ok(pending_tx) => {
                let tx_hash = pending_tx.tx_hash().clone();
                info!("[BRIDGE] depositV3 sent: tx_hash={:?}", tx_hash);

                // Watch for confirmation with timeout
                let watch_timeout = Duration::from_secs(36);
                info!(
                    "[BRIDGE] Watching for confirmation of tx_hash={:?} (timeout: {:?})",
                    tx_hash, watch_timeout
                );

                match timeout(watch_timeout, pending_tx.watch()).await {
                    Ok(Ok(receipt)) => {
                        info!("[BRIDGE] Bridge tx confirmed: receipt={:?}", receipt);
                    }
                    Ok(Err(e)) => {
                        error!("[BRIDGE] Bridge tx dropped or errored: {}", e);
                    }
                    Err(_) => {
                        error!(
                            "[BRIDGE] Bridge tx watch timed out for tx_hash={:?}",
                            tx_hash
                        );
                    }
                }

                self.cross_chain_rebalance.clear();
                self.accross_timer = Some(std::time::Instant::now());
            }
            Err(e) => {
                error!(
                    "[BRIDGE] Failed to bridge via Accross from chain {} to chain {}: {} (params: amount={}, output_amount={}, fill_deadline={}, quote_timestamp={}, spoke_pool_address={:?}, input_token={:?}, output_token={:?})",
                    src_chain_id, target_chain_id, e, amount, output_amount, fill_deadline, quote_timestamp, spoke_pool_address, wrapped_native_token_in, wrapped_native_token_out
                );
            }
        }
    }

    /// Creates a provider for a given chain ID and RPC URL
    ///
    /// This method:
    /// 1. Parses the RPC URL to create a `Url` object.
    /// 2. Creates a `ProviderConfig` for the chain.
    /// 3. Creates a `ProviderState` with the config.
    /// 4. Attempts to get a fresh provider from `ProviderState`.
    /// 5. Returns the provider if successful, otherwise returns `None`.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to create provider for
    /// * `rpc_url` - RPC URL for the chain
    ///
    /// # Returns
    /// * `Option<ProviderType>` - A fresh provider for the chain, or `None` on error
    async fn create_provider_for_chain(
        &self,
        chain_id: u64,
        rpc_url: &str,
    ) -> Option<ProviderType> {
        let Ok(url) = Url::parse(rpc_url) else {
            error!(
                "Failed to parse RPC URL for chain {}: {}",
                chain_id, rpc_url
            );
            return None;
        };

        let provider_config = ProviderConfig {
            primary_url: url.to_string(),
            fallback_url: url.to_string(), // Use same URL as fallback for now
            max_block_delay_secs: 300,     // 5 minutes
            chain_id,
            use_websocket: false, // Use HTTP for balance polling
        };

        let mut provider_state = ProviderState::new(provider_config);

        match provider_state.get_fresh_provider().await {
            Ok(result) => {
                let (provider, _is_fallback) = result;
                Some(provider)
            }
            Err(e) => {
                error!(
                    "Failed to get fresh provider for chain {}: {:?}",
                    chain_id, e
                );
                None
            }
        }
    }

    /// Retrieves gas parameters (base fee and priority fee) for a given chain.
    ///
    /// This method:
    /// 1. Gets the current gas price from the provider.
    /// 2. Calculates base fee and priority fee based on the chain ID.
    /// 3. Returns a tuple of `(base_fee_per_gas, priority_fee_per_gas)`.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to get gas parameters for
    /// * `provider` - Provider instance for the chain
    ///
    /// # Returns
    /// * `Option<(u128, u128)>` - A tuple of base and priority fees, or `None` on error
    async fn get_gas_parameters_for_chain(
        &self,
        chain_id: u64,
        provider: &ProviderType,
    ) -> Option<(u128, u128)> {
        let base_fee_per_gas = match provider.get_gas_price().await {
            Ok(fee) => fee,
            Err(e) => {
                error!(
                    "[GET_GAS_PARAMETERS_FOR_CHAIN] Failed to get gas price: {}",
                    e
                );
                return None;
            }
        };

        let base_fee_per_gas = if chain_id == ETHEREUM_CHAIN_ID {
            base_fee_per_gas * 200 / 100
        } else {
            base_fee_per_gas * 500 / 100
        };

        let priority_fee_per_gas = base_fee_per_gas;

        Some((base_fee_per_gas, priority_fee_per_gas))
    }

    /// Updates the distributor and sequencer balances for a given chain after a transaction.
    ///
    /// This method:
    /// 1. Creates a provider for the chain.
    /// 2. Retrieves the current balances for distributor and sequencer.
    /// 3. Updates the internal balance tracking structures.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to update balances for
    /// * `provider` - Provider instance for the chain
    async fn update_balances_after_transaction(&mut self, chain_id: u64, provider: &ProviderType) {
        match provider.get_balance(self.public_address).await {
            Ok(balance) => {
                if let Some(balances) = self.distributor_balances.get_mut(&chain_id) {
                    *balances = balance;
                }
            }
            Err(e) => {
                error!(
                    "Failed to update distributor balance on chain {}: {}",
                    chain_id, e
                );
            }
        }
        match provider.get_balance(self.sequencer_address).await {
            Ok(balance) => {
                if let Some(balances) = self.sequencer_balances.get_mut(&chain_id) {
                    *balances = balance;
                }
            }
            Err(e) => {
                error!(
                    "Failed to update sequencer balance on chain {}: {}",
                    chain_id, e
                );
            }
        }
    }

    /// Creates a signed provider for a given chain ID and RPC URL
    ///
    /// This method creates a provider with signing capabilities for transaction submission.
    /// It uses the private key from the distributer configuration.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to create provider for
    /// * `rpc_url` - RPC URL for the chain
    ///
    /// # Returns
    /// * `Option<ProviderType>` - A signed provider for the chain, or `None` on error
    async fn create_signed_provider_for_chain(
        &self,
        chain_id: u64,
        rpc_url: &str,
    ) -> Option<ProviderType> {
        let Ok(url) = Url::parse(rpc_url) else {
            error!(
                "Failed to parse RPC URL for chain {}: {}",
                chain_id, rpc_url
            );
            return None;
        };

        let signer: PrivateKeySigner = match self.private_key.parse() {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to parse private key: {}", e);
                return None;
            }
        };

        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(url);

        Some(provider)
    }

    /// Gets the wrapped native token address for a given chain ID
    ///
    /// This method retrieves the wrapped native token (WETH) address for a given chain
    /// from environment variables.
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to get wrapped native token address for
    ///
    /// # Returns
    /// * `Option<Address>` - Wrapped native token address, or `None` on error
    async fn get_wrapped_native_token_address(&self, chain_id: u64) -> Option<Address> {
        let wrapped_native_token = match chain_id {
            ETHEREUM_CHAIN_ID => env::var("ETHEREUM_WETH_ADDRESS")
                .expect("ETHEREUM_WETH_ADDRESS must be set in .env"),
            BASE_CHAIN_ID => {
                env::var("BASE_WETH_ADDRESS").expect("BASE_WETH_ADDRESS must be set in .env")
            }
            LINEA_CHAIN_ID => {
                env::var("LINEA_WETH_ADDRESS").expect("LINEA_WETH_ADDRESS must be set in .env")
            }
            _ => {
                error!("Unsupported chain ID for WETH: {}", chain_id);
                return None;
            }
        };

        match Address::from_str(&wrapped_native_token) {
            Ok(addr) => Some(addr),
            Err(e) => {
                error!("Invalid wrapped native token address: {}", e);
                None
            }
        }
    }
}
