use alloy::primitives::{Address, U256, Bytes};
use std::collections::HashMap;
use tracing::{info, error};
use alloy::providers::ProviderBuilder;
use alloy::transports::http::reqwest::Url;
use alloy::providers::Provider;
use tokio::time;
use crate::types::{IMaldaMarket, IAccross};
use alloy::signers::local::PrivateKeySigner;
use alloy::network::EthereumWallet;
use alloy::rpc::types::TransactionRequest;
use alloy::network::TransactionBuilder;
use std::env;
use std::str::FromStr;
use tokio::time::{timeout, Duration};
use malda_rs::constants::*;

pub const GAS_LIMIT: u64 = 100000u64;
pub const WAIT_TIME: u64 = 24; // in secnds, two mainent blocks

pub struct GasFeeDistributer {
    pub chains: Vec<u64>,
    pub markets_per_chain: HashMap<u64, Vec<Address>>,
    pub rpc_urls: HashMap<u64, String>,
    pub balances: HashMap<u64, HashMap<Address, U256>>, // chain_id -> (market -> ETH balance in wei)
    pub distributor_balances: HashMap<u64, U256>, // chain_id -> distributor ETH balance in wei
    pub sequencer_balances: HashMap<u64, U256>, // chain_id -> sequencer ETH balance in wei
    pub poll_interval_secs: u64, // polling interval in seconds
    pub private_key: String,
    pub public_address: Address,
    pub sequencer_address: Address,
    pub minimum_sequencer_balance_per_chain: HashMap<u64, U256>,
    pub min_distributor_balance_per_chain: HashMap<u64, U256>,
    pub target_sequencer_balance_per_chain: HashMap<u64, U256>,
    pub minimum_harvest_balance_per_chain: HashMap<u64, U256>,
    pub cross_chain_rebalance: Vec<u64>,
    pub bridge_fee_percentage: u32,
    pub min_amount_to_bridge: U256,
    pub accross_settlement_time: u64, // in seconds
    pub accross_timer: Option<std::time::Instant>,
}

impl GasFeeDistributer {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
    ) -> Self {
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
        let accross_settlement_time = std::env::var("ACCROSS_SETTLEMENT_TIME").expect("ACCROSS_SETTLEMENT_TIME must be set in .env").parse().expect("ACCROSS_SETTLEMENT_TIME must be a u64");
        let accross_timer = None;
        GasFeeDistributer {
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
        }
    }

    pub async fn start_polling_balances(&mut self) {
        let mut interval = time::interval(std::time::Duration::from_secs(self.poll_interval_secs));
        loop {
            info!("Gas Fee Balances: {:?}", self.balances);
            interval.tick().await;
            for &chain_id in &self.chains {
                let rpc_url = match self.rpc_urls.get(&chain_id) {
                    Some(url) => url,
                    None => {
                        error!("No RPC URL for chain {}", chain_id);
                        continue;
                    }
                };
                let url = match Url::parse(rpc_url) {
                    Ok(u) => u,
                    Err(e) => {
                        error!("Invalid RPC URL for chain {}: {}", chain_id, e);
                        continue;
                    }
                };
                let provider = ProviderBuilder::new().connect_http(url.clone());
                // Get distributor balance
                match provider.get_balance(self.public_address).await {
                    Ok(balance) => {
                        self.distributor_balances.insert(chain_id, balance);
                    }
                    Err(e) => {
                        error!("Failed to get distributor balance on chain {}: {}", chain_id, e);
                    }
                }
                // Get sequencer balance
                match provider.get_balance(self.sequencer_address).await {
                    Ok(balance) => {
                        self.sequencer_balances.insert(chain_id, balance);
                    }
                    Err(e) => {
                        error!("Failed to get sequencer balance on chain {}: {}", chain_id, e);
                    }
                }
                if let Some(markets) = self.markets_per_chain.get(&chain_id) {
                    for &market in markets {
                        match provider.get_balance(market).await {
                            Ok(balance) => {
                                if let Some(chain_balances) = self.balances.get_mut(&chain_id) {
                                    chain_balances.insert(market, balance);
                                }
                            }
                            Err(e) => {
                                error!("Failed to get balance for market {:?} on chain {}: {}", market, chain_id, e);
                            }
                        }
                    }
                }
            }
            self.distribute_gas_fees().await;
        }
    }

    pub async fn distribute_gas_fees(&mut self) {

        // Collect chain_ids to process to avoid borrow checker issues
        let to_harvest: Vec<u64> = self.chains.iter().copied().filter(|&chain_id| {
            let sequencer_balance = self.sequencer_balances.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
            let min_balance = self.minimum_sequencer_balance_per_chain.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
            sequencer_balance < min_balance
        }).collect();
        for chain_id in to_harvest {
            self.withdraw_gas_fees(chain_id).await;
            self.send_to_sequencer(chain_id).await;
        }
        // Cross-chain rebalance section
        // wait 2 seconds before starting cross-chain rebalance
        tokio::time::sleep(Duration::from_secs(24)).await;
        let mut rebalance_pairs = Vec::new();
        for &chain_id in &self.cross_chain_rebalance {
            for &other_chain_id in &self.chains {
                if other_chain_id == chain_id { continue; }
                rebalance_pairs.push((other_chain_id, chain_id));
            }
        }
        for (src, target) in rebalance_pairs {
            if let Some(timer) = &self.accross_timer {
                if timer.elapsed().as_secs() < self.accross_settlement_time {
                    info!("Still waiting for settlement, skipping cross-chain rebalance");
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

    pub async fn withdraw_gas_fees(&mut self, chain_id: u64) {
        let Some(rpc_url) = self.rpc_urls.get(&chain_id) else { return };
        let Ok(url) = Url::parse(rpc_url) else { return };
        let signer: PrivateKeySigner = match self.private_key.parse() {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to parse private key: {}", e);
                return;
            }
        };
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(url);
        let min_harvest = self.minimum_harvest_balance_per_chain.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
        if let Some(markets) = self.markets_per_chain.get(&chain_id) {
            for &market in markets {
                let market_balance = self.balances.get(&chain_id).and_then(|m| m.get(&market)).cloned().unwrap_or(U256::from(0u64));
                if market_balance < min_harvest {
                    continue;
                }
                let contract = IMaldaMarket::new(market, provider.clone());
                let mut call = contract.withdrawGasFees(self.public_address);

                // Set gas parameters as in send_to_sequencer
                let base_fee_per_gas = match provider.get_gas_price().await {
                    Ok(fee) => fee,
                    Err(e) => {
                        error!("[WITHDRAW_GAS_FEES] Failed to get gas price: {}", e);
                        continue;
                    }
                };
                let base_fee_per_gas = if chain_id == ETHEREUM_CHAIN_ID {
                    base_fee_per_gas * 200 / 100
                } else {
                    base_fee_per_gas * 500 / 100
                };
                let priority_fee_per_gas = base_fee_per_gas;
                call = call
                    .gas(GAS_LIMIT)
                    .max_fee_per_gas(base_fee_per_gas)
                    .max_priority_fee_per_gas(priority_fee_per_gas);

                match call.send().await {
                    Ok(pending_tx) => {
                        info!("Called withdrawGasFees on market {:?} for chain {}: tx_hash={:?}", market, chain_id, pending_tx.tx_hash());
                    }
                    Err(e) => {
                        error!("Failed to call withdrawGasFees on market {:?} for chain {}: {}", market, chain_id, e);
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;
        // Update distributor balance after harvesting
        match provider.get_balance(self.public_address).await {
            Ok(balance) => {
                if let Some(balances) = self.distributor_balances.get_mut(&chain_id) {
                    *balances = balance;
                }
            }
            Err(e) => {
                error!("Failed to update distributor balance on chain {}: {}", chain_id, e);
            }
        }
    }

    pub async fn send_to_sequencer(&mut self, chain_id: u64) {
        let Some(rpc_url) = self.rpc_urls.get(&chain_id) else { return };
        let Ok(url) = Url::parse(rpc_url) else { return };
        let signer: PrivateKeySigner = match self.private_key.parse() {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to parse private key: {}", e);
                return;
            }
        };
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(url);
        let distributor_balance = self.distributor_balances.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
        let sequencer_balance = self.sequencer_balances.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
        let min_distributor = self.min_distributor_balance_per_chain.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
        let target_sequencer = self.target_sequencer_balance_per_chain.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
        let _min_sequencer = self.minimum_sequencer_balance_per_chain.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
        if distributor_balance <= min_distributor {
            info!("Distributor balance too low to send on chain {}", chain_id);
            self.cross_chain_rebalance.push(chain_id);
            return;
        }
        let needed = if target_sequencer > sequencer_balance {
            target_sequencer - sequencer_balance
        } else {
            U256::from(0u64)
        };
        if needed < _min_sequencer / U256::from(5u64) {
            info!("Sequencer already at or above target (or close enough) on chain {}", chain_id);
            return;
        }
        let available = distributor_balance - min_distributor;
        let to_send = if available >= needed { needed } else { available };
        if to_send.is_zero() {
            info!("Nothing to send to sequencer on chain {}", chain_id);
            return;
        }
        // Build the transaction
        let mut tx = TransactionRequest::default()
            .with_from(self.public_address)
            .with_to(self.sequencer_address)
            .with_value(to_send);

        // Estimate gas and set explicitly

        let base_fee_per_gas = match provider.get_gas_price().await {
            Ok(fee) => fee,
            Err(e) => {
                error!("[SEND_TO_SEQUENCER] Failed to get gas price: {}", e);
                return;
            }
        };

        let base_fee_per_gas = if chain_id == ETHEREUM_CHAIN_ID {
            base_fee_per_gas * 200 / 100
        } else {
            base_fee_per_gas * 500 / 100
        };
        
        let priority_fee_per_gas = base_fee_per_gas;

        tx = tx
            .gas_limit(GAS_LIMIT)
            .max_fee_per_gas(base_fee_per_gas)
            .max_priority_fee_per_gas(priority_fee_per_gas);

        match provider.send_transaction(tx).await {
            Ok(pending_tx) => {
                info!("Sent {} wei to sequencer on chain {}: tx_hash={:?}", to_send, chain_id, pending_tx.tx_hash());
            }
            Err(e) => {
                error!("Failed to send to sequencer on chain {}: {}", chain_id, e);
                return;
            }
        }
        tokio::time::sleep(Duration::from_secs(WAIT_TIME)).await;
        // Update balances after sending
        match provider.get_balance(self.public_address).await {
            Ok(balance) => {
                self.distributor_balances.insert(chain_id, balance);
            }
            Err(e) => {
                error!("Failed to update distributor balance on chain {}: {}", chain_id, e);
            }
        }
        match provider.get_balance(self.sequencer_address).await {
            Ok(balance) => {
                self.sequencer_balances.insert(chain_id, balance);
            }
            Err(e) => {
                error!("Failed to update sequencer balance on chain {}: {}", chain_id, e);
            }
        }
        // If sequencer is still below minimum, add to cross_chain_rebalance
        let updated_seq = self.sequencer_balances.get(&chain_id).cloned().unwrap_or(U256::from(0u64));
        if updated_seq < _min_sequencer {
            if !self.cross_chain_rebalance.contains(&chain_id) {
                self.cross_chain_rebalance.push(chain_id);
            }
        }
    }

    pub async fn cross_chain_gas_distribution(&mut self, src_chain_id: u64, target_chain_id: u64) {
        // Get distributor balance and min_distributor for src_chain_id
        let distributor_balance = self.distributor_balances.get(&src_chain_id).cloned().unwrap_or(U256::from(0u64));
        let min_distributor = self.min_distributor_balance_per_chain.get(&src_chain_id).cloned().unwrap_or(U256::from(0u64));
        // Get sequencer balance, min_sequencer, and target_sequencer for target_chain_id
        let sequencer_balance = self.sequencer_balances.get(&target_chain_id).cloned().unwrap_or(U256::from(0u64));
        let _min_sequencer = self.minimum_sequencer_balance_per_chain.get(&target_chain_id).cloned().unwrap_or(U256::from(0u64));
        let target_sequencer = self.target_sequencer_balance_per_chain.get(&target_chain_id).cloned().unwrap_or(U256::from(0u64));
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
        let to_send = if available < needed { available } else { needed };
        if to_send < self.min_amount_to_bridge {
            info!("Not enough gas fees to bridge on chain {} to chain {}", src_chain_id, target_chain_id);
            return;
        }
        self.bridge_via_accross(src_chain_id, target_chain_id, to_send).await;
    }

    pub async fn bridge_via_accross(&mut self, src_chain_id: u64, target_chain_id: u64, amount: U256) {
        // Determine the spoke pool address for the source chain
        let spoke_pool_address = match src_chain_id {
            ETHEREUM_CHAIN_ID => {
                env::var("ETHEREUM_SPOKE_POOL_ADDRESS").expect("ETHEREUM_SPOKE_POOL_ADDRESS must be set in .env")
            }
            BASE_CHAIN_ID => {
                env::var("BASE_SPOKE_POOL_ADDRESS").expect("BASE_SPOKE_POOL_ADDRESS must be set in .env")
            }
            LINEA_CHAIN_ID => {
                env::var("LINEA_SPOKE_POOL_ADDRESS").expect("LINEA_SPOKE_POOL_ADDRESS must be set in .env")
            }
            _ => {
                error!("Unsupported source chain ID: {}", src_chain_id);
                return;
            }
        };

        // Get the RPC URL for the source chain
        let Some(rpc_url) = self.rpc_urls.get(&src_chain_id) else { return };
        let Ok(url) = Url::parse(rpc_url) else { return };
        let signer: PrivateKeySigner = match self.private_key.parse() {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to parse private key: {}", e);
                return;
            }
        };
        let wallet = EthereumWallet::from(signer);
        let provider = ProviderBuilder::new().wallet(wallet).connect_http(url);

        // Convert addresses from String to Address
        let spoke_pool_address = match Address::from_str(&spoke_pool_address) {
            Ok(addr) => addr,
            Err(e) => {
                error!("Invalid spoke pool address: {}", e);
                return;
            }
        };
        let contract = IAccross::new(spoke_pool_address, provider.clone());
        // Use the wrapped native token address for inputToken/outputToken (e.g., WETH)
        let wrapped_native_token_in = match src_chain_id {
            ETHEREUM_CHAIN_ID => env::var("ETHEREUM_WETH_ADDRESS").expect("ETHEREUM_WETH_ADDRESS must be set in .env"),
            BASE_CHAIN_ID => env::var("BASE_WETH_ADDRESS").expect("BASE_WETH_ADDRESS must be set in .env"),
            LINEA_CHAIN_ID => env::var("LINEA_WETH_ADDRESS").expect("LINEA_WETH_ADDRESS must be set in .env"),
            _ => {
                error!("Unsupported source chain ID for WETH: {}", src_chain_id);
                return;
            }
        };
        let wrapped_native_token_out = match target_chain_id {
            ETHEREUM_CHAIN_ID => env::var("ETHEREUM_WETH_ADDRESS").expect("ETHEREUM_WETH_ADDRESS must be set in .env"),
            BASE_CHAIN_ID => env::var("BASE_WETH_ADDRESS").expect("BASE_WETH_ADDRESS must be set in .env"),
            LINEA_CHAIN_ID => env::var("LINEA_WETH_ADDRESS").expect("LINEA_WETH_ADDRESS must be set in .env"),
            _ => {
                error!("Unsupported target chain ID for WETH: {}", target_chain_id);
                return;
            }
        };
        // Get quoteTimestamp and buffer from contract
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
                error!("[BRIDGE] Failed to get depositQuoteTimeBuffer from contract: {}", e);
                return;
            }
        };
        let quote_timestamp = current_time; // Safe to use current time as quoteTimestamp
        // Set fillDeadline and exclusivityDeadline
        let fill_deadline = quote_timestamp + 120;
        let exclusivity_deadline = 0u32;
        info!(
            "[BRIDGE] Using quote_timestamp={}, buffer={}, fill_deadline={}",
            quote_timestamp, buffer, fill_deadline
        );
        // Calculate outputAmount as amount minus the bridge fee
        let fee = amount * U256::from(self.bridge_fee_percentage) / U256::from(100u32);
        let output_amount = if amount > fee { amount - fee } else { U256::from(0u64) };
        // Convert target_chain_id to U256
        let target_chain_id_u256 = U256::from(target_chain_id);
        info!(
            "[BRIDGE] Preparing to bridge via Accross: src_chain_id={}, target_chain_id={}, amount={}, rpc_url={}, sender={:?}, spoke_pool_address={:?}, input_token={:?}, output_token={:?}, output_amount={}, fill_deadline={}, quote_timestamp={}",
            src_chain_id,
            target_chain_id,
            amount,
            rpc_url,
            self.public_address,
            spoke_pool_address,
            wrapped_native_token_in,
            wrapped_native_token_out,
            output_amount,
            fill_deadline,
            quote_timestamp
        );
        let call = contract.depositV3(
            self.public_address, // depositor
            self.public_address, // recipient (distributor on target chain)
            Address::from_str(&wrapped_native_token_in).unwrap(), // inputToken (WETH address)
            Address::from_str(&wrapped_native_token_out).unwrap(), // outputToken (WETH address)
            amount, // inputAmount
            output_amount, // outputAmount (amount minus fee)
            target_chain_id_u256, // destinationChainId
            Address::ZERO, // exclusiveRelayer
            quote_timestamp, // quoteTimestamp
            fill_deadline, // fillDeadline
            exclusivity_deadline, // exclusivityDeadline
            Bytes::from_static(b"") // message
        );
        info!("[BRIDGE] Sending depositV3 transaction with msg.value={}...", amount);

        let base_fee_per_gas = match provider.get_gas_price().await {
            Ok(fee) => fee,
            Err(e) => {
                error!("[BRIDGE] Failed to get gas price: {}", e);
                return;
            }
        };
        let base_fee_per_gas = if src_chain_id == ETHEREUM_CHAIN_ID {
            base_fee_per_gas * 200 / 100
        } else {
            base_fee_per_gas * 500 / 100
        };
        let priority_fee_per_gas = base_fee_per_gas;

        let call = call
            .gas(GAS_LIMIT)
            .max_fee_per_gas(base_fee_per_gas)
            .max_priority_fee_per_gas(priority_fee_per_gas);

        match call.value(amount).send().await {
            Ok(pending_tx) => {
                let tx_hash = pending_tx.tx_hash().clone();
                info!(
                    "[BRIDGE] depositV3 sent: tx_hash={:?}",
                    tx_hash
                );
                // Watch for confirmation with timeout
                let watch_timeout = Duration::from_secs(36);
                info!("[BRIDGE] Watching for confirmation of tx_hash={:?} (timeout: {:?})", tx_hash, watch_timeout);

                match timeout(watch_timeout, pending_tx.watch()).await {
                    Ok(Ok(receipt)) => {
                        info!("[BRIDGE] Bridge tx confirmed: receipt={:?}", receipt);
                    }
                    Ok(Err(e)) => {
                        error!("[BRIDGE] Bridge tx dropped or errored: {}", e);
                    }
                    Err(_) => {
                        error!("[BRIDGE] Bridge tx watch timed out for tx_hash={:?}", tx_hash);
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
} 