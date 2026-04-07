use alloy::primitives::{Address, U256};
use eyre::Result;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use tracing::info;

use crate::constants::*;
use crate::events::*;
use malda_rs::constants::*;

#[derive(Debug, Clone, PartialEq)]
pub enum Environment {
    Testnet,
    Mainnet,
}

impl Environment {
    pub fn from_env() -> Self {
        std::env::var("ENVIRONMENT")
            .unwrap_or_else(|_| "testnet".to_string())
            .to_lowercase()
            .as_str()
            .into()
    }
}

impl From<&str> for Environment {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "mainnet" => Environment::Mainnet,
            _ => Environment::Testnet,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub chain_id: u32,
    pub name: String,
    pub is_l1: bool,
    pub rpc_url: String,
    pub fallback_rpc_url: String,
    pub ws_url: String,
    pub fallback_ws_url: String,

    pub batch_submitter_address: Address,
    pub markets: Vec<Address>,
    pub events: Vec<String>,
    pub max_block_delay_secs: u64,
    pub block_delay: u64,
    pub retarded_block_delay: u64,
    pub reorg_protection: u64,
    pub transaction_config: TransactionChainConfig,
    pub lane_config: LaneChainConfig,
}

#[derive(Debug, Clone)]
pub struct TransactionChainConfig {
    pub max_retries: u128,
    pub retry_delay: Duration,
    pub submission_delay_seconds: u64,
    pub poll_interval: Duration,
    pub max_tx: i64,
    pub tx_timeout: Duration,
    pub gas_percentage_increase_per_retry: u128,
}

#[derive(Debug, Clone)]
pub struct LaneChainConfig {
    pub max_volume: i32,
    pub time_interval: Duration,
    pub block_delay: u64,
    pub reorg_protection: u64,
}

#[derive(Debug, Clone)]
pub struct SequencerConfig {
    pub environment: Environment,
    pub chains: HashMap<u32, ChainConfig>,
    pub database_url: String,
    pub fallback_database_url: Option<String>,
    pub sequencer_address: Address,
    pub sequencer_private_key: String,
    pub gas_fee_distributer_address: Address,
    pub gas_fee_distributer_private_key: String,
    pub proof_config: ProofConfig,
    pub event_listener_config: EventListenerConfig,
    pub restart_config: RestartConfig,
    pub gas_fee_distributer_config: GasFeeDistributerConfig,
    pub event_proof_ready_checker_config: EventProofReadyCheckerConfig,
    pub reset_tx_manager_config: ResetTxManagerConfig,
}

#[derive(Debug, Clone)]
pub struct ProofConfig {
    pub batch_limit: usize,
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub request_delay: Duration,
    pub dummy_mode: bool,
}

#[derive(Debug, Clone)]
pub struct EventListenerConfig {
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
    pub block_range_offset_l1_from: u64,
    pub block_range_offset_l1_to: u64,
    pub block_range_offset_l2_from: u64,
    pub block_range_offset_l2_to: u64,
    pub retarded_max_retries: u32,
    pub retarded_retry_delay_secs: u64,
    pub retarded_poll_interval_secs: u64,
    pub retarded_block_range_offset_l1_from: u64,
    pub retarded_block_range_offset_l1_to: u64,
    pub retarded_block_range_offset_l2_from: u64,
    pub retarded_block_range_offset_l2_to: u64,
}

#[derive(Debug, Clone)]
pub struct RestartConfig {
    pub listener_delay_seconds: u64,
    pub listener_interval_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct GasFeeDistributerConfig {
    pub poll_interval_secs: u64,
    pub bridge_fee_percentage: u32,
    pub min_amount_to_bridge: U256,
    pub minimum_sequencer_balance_per_chain: HashMap<u64, U256>,
    pub min_distributor_balance_per_chain: HashMap<u64, U256>,
    pub target_sequencer_balance_per_chain: HashMap<u64, U256>,
    pub minimum_harvest_balance_per_chain: HashMap<u64, U256>,
}

#[derive(Debug, Clone)]
pub struct EventProofReadyCheckerConfig {
    pub poll_interval_seconds: u64,
    pub block_update_interval_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct ResetTxManagerConfig {
    pub sample_size: i64,
    pub multiplier: f64,
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
    pub batch_limit: i64,
    pub max_retries_reset: i64,
    pub api_key: String,
    pub rebalancer_url: String,
    pub rebalance_delay: u64,
    pub minimum_usd_value: f64,
}

impl SequencerConfig {
    pub fn new() -> Result<Self> {
        let environment = Environment::from_env();
        let mut config = Self {
            environment: environment.clone(),
            chains: HashMap::new(),
            database_url: Self::get_env_var("DATABASE_URL")?,
            fallback_database_url: std::env::var("DATABASE_URL_FALLBACK").ok(),
            sequencer_address: Address::from_str(&Self::get_env_var("SEQUENCER_ADDRESS")?)?,
            sequencer_private_key: Self::get_env_var("SEQUENCER_PRIVATE_KEY")?,
            gas_fee_distributer_address: Address::from_str(&Self::get_env_var(
                "GAS_FEE_DISTRIBUTER_ADDRESS",
            )?)?,
            gas_fee_distributer_private_key: Self::get_env_var("GAS_FEE_DISTRIBUTER_PRIVATE_KEY")?,
            proof_config: ProofConfig::new()?,
            event_listener_config: EventListenerConfig::new()?,
            restart_config: RestartConfig::new()?,
            gas_fee_distributer_config: GasFeeDistributerConfig::new()?,
            event_proof_ready_checker_config: EventProofReadyCheckerConfig::new()?,
            reset_tx_manager_config: ResetTxManagerConfig::new()?,
        };

        config.init_chains()?;
        Ok(config)
    }

    fn get_env_var(key: &str) -> Result<String> {
        std::env::var(key).map_err(|_| eyre::eyre!("Environment variable {} not found", key))
    }

    fn get_env_var_with_default(key: &str, default: &str) -> Result<String> {
        match std::env::var(key) {
            Ok(value) => Ok(value),
            Err(_) => {
                info!("Environment variable {} not found, using default: {}", key, default);
                Ok(default.to_string())
            }
        }
    }

    fn init_chains(&mut self) -> Result<()> {
        match self.environment {
            Environment::Testnet => self.init_testnet_chains()?,
            Environment::Mainnet => self.init_mainnet_chains()?,
        }
        Ok(())
    }

    fn init_testnet_chains(&mut self) -> Result<()> {
        let mut enabled_chains = Vec::new();

        // Try to add Linea Sepolia
        if let Ok(chain_config) = self.try_create_linea_sepolia_config() {
            self.chains
                .insert(LINEA_SEPOLIA_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Linea Sepolia");
        }

        // Try to add Optimism Sepolia
        if let Ok(chain_config) = self.try_create_optimism_sepolia_config() {
            self.chains
                .insert(OPTIMISM_SEPOLIA_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Optimism Sepolia");
        }

        // Try to add Ethereum Sepolia
        if let Ok(chain_config) = self.try_create_ethereum_sepolia_config() {
            self.chains
                .insert(ETHEREUM_SEPOLIA_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Ethereum Sepolia");
        }

        // Try to add Base Sepolia
        if let Ok(chain_config) = self.try_create_base_sepolia_config() {
            self.chains
                .insert(BASE_SEPOLIA_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Base Sepolia");
        }

        if enabled_chains.is_empty() {
            return Err(eyre::eyre!("No chains configured for testnet. Please set at least one chain's RPC_URL environment variable."));
        }

        println!("✅ Enabled testnet chains: {}", enabled_chains.join(", "));
        Ok(())
    }

    fn init_mainnet_chains(&mut self) -> Result<()> {
        let mut enabled_chains = Vec::new();

        // Try to add Linea Mainnet
        if let Ok(chain_config) = self.try_create_linea_mainnet_config() {
            self.chains.insert(LINEA_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Linea Mainnet");
        }

        // Try to add Optimism Mainnet
        if let Ok(chain_config) = self.try_create_optimism_mainnet_config() {
            self.chains.insert(OPTIMISM_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Optimism Mainnet");
        }

        // Try to add Ethereum Mainnet
        if let Ok(chain_config) = self.try_create_ethereum_mainnet_config() {
            self.chains.insert(ETHEREUM_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Ethereum Mainnet");
        }

        // Try to add Base Mainnet
        if let Ok(chain_config) = self.try_create_base_mainnet_config() {
            self.chains.insert(BASE_CHAIN_ID as u32, chain_config);
            enabled_chains.push("Base Mainnet");
        }

        if enabled_chains.is_empty() {
            return Err(eyre::eyre!("No chains configured for mainnet. Please set at least one chain's RPC_URL environment variable."));
        }

        println!("✅ Enabled mainnet chains: {}", enabled_chains.join(", "));
        Ok(())
    }

    pub fn get_chain(&self, chain_id: u32) -> Option<&ChainConfig> {
        self.chains.get(&chain_id)
    }

    pub fn get_all_chains(&self) -> Vec<&ChainConfig> {
        self.chains.values().collect()
    }

    pub fn get_l1_chains(&self) -> Vec<&ChainConfig> {
        self.chains.values().filter(|c| c.is_l1).collect()
    }

    pub fn get_l2_chains(&self) -> Vec<&ChainConfig> {
        self.chains.values().filter(|c| !c.is_l1).collect()
    }

    // Testnet chain configuration helpers
    fn try_create_linea_sepolia_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: LINEA_SEPOLIA_CHAIN_ID as u32,
            name: "Linea Sepolia".to_string(),
            is_l1: false,
            rpc_url: Self::get_env_var("RPC_URL_LINEA_SEPOLIA")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_LINEA_SEPOLIA_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_LINEA_SEPOLIA")?,
            fallback_ws_url: Self::get_env_var("WS_URL_LINEA_SEPOLIA_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![M_USDC_MOCK_MARKET, MWST_ETH_MOCK_MARKET],
            events: vec![
                HOST_BORROW_ON_EXTENSION_CHAIN_SIG.to_string(),
                HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG.to_string(),
            ],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2", "300")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2",
            )?
            .parse()?,
            reorg_protection: 2,
            transaction_config: TransactionChainConfig::new_l2()?,
            lane_config: LaneChainConfig::new_testnet()?,
        })
    }

    fn try_create_optimism_sepolia_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: OPTIMISM_SEPOLIA_CHAIN_ID as u32,
            name: "Optimism Sepolia".to_string(),
            is_l1: false,
            rpc_url: Self::get_env_var("RPC_URL_OPTIMISM_SEPOLIA")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_OPTIMISM_SEPOLIA_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_OPT_SEPOLIA")?,
            fallback_ws_url: Self::get_env_var("WS_URL_OPT_SEPOLIA_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![M_USDC_MOCK_MARKET, MWST_ETH_MOCK_MARKET],
            events: vec![EXTENSION_SUPPLIED_SIG.to_string(), EXTENSION_LIQUIDATE_SIG.to_string()],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2", "300")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2",
            )?
            .parse()?,
            reorg_protection: 2,
            transaction_config: TransactionChainConfig::new_l2()?,
            lane_config: LaneChainConfig::new_testnet()?,
        })
    }

    fn try_create_ethereum_sepolia_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: ETHEREUM_SEPOLIA_CHAIN_ID as u32,
            name: "Ethereum Sepolia".to_string(),
            is_l1: true,
            rpc_url: Self::get_env_var("RPC_URL_ETHEREUM_SEPOLIA")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_ETHEREUM_SEPOLIA_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_ETH_SEPOLIA")?,
            fallback_ws_url: Self::get_env_var("WS_URL_ETH_SEPOLIA_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![M_USDC_MOCK_MARKET, MWST_ETH_MOCK_MARKET],
            events: vec![EXTENSION_SUPPLIED_SIG.to_string(), EXTENSION_LIQUIDATE_SIG.to_string()],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1", "600")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1",
            )?
            .parse()?,
            reorg_protection: 0,
            transaction_config: TransactionChainConfig::new_l1()?,
            lane_config: LaneChainConfig::new_testnet()?,
        })
    }

    fn try_create_base_sepolia_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: BASE_SEPOLIA_CHAIN_ID as u32,
            name: "Base Sepolia".to_string(),
            is_l1: false,
            rpc_url: Self::get_env_var("RPC_URL_BASE_SEPOLIA")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_BASE_SEPOLIA_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_BASE_SEPOLIA")?,
            fallback_ws_url: Self::get_env_var("WS_URL_BASE_SEPOLIA_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![M_USDC_MOCK_MARKET, MWST_ETH_MOCK_MARKET],
            events: vec![EXTENSION_SUPPLIED_SIG.to_string(), EXTENSION_LIQUIDATE_SIG.to_string()],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2", "300")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2",
            )?
            .parse()?,
            reorg_protection: 2,
            transaction_config: TransactionChainConfig::new_l2()?,
            lane_config: LaneChainConfig::new_testnet()?,
        })
    }

    // Mainnet chain configuration helpers
    fn try_create_linea_mainnet_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: LINEA_CHAIN_ID as u32,
            name: "Linea Mainnet".to_string(),
            is_l1: false,
            rpc_url: Self::get_env_var("RPC_URL_LINEA")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_LINEA_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_LINEA")?,
            fallback_ws_url: Self::get_env_var("WS_URL_LINEA_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![
                M_USDC_MARKET,
                M_WETH_MARKET,
                M_USDT_MARKET,
                M_WBTC_MARKET,
                MWST_ETH_MARKET,
                MEZ_ETH_MARKET,
                MWE_ETH_MARKET,
                MWRS_ETH_MARKET,
            ],
            events: vec![
                HOST_BORROW_ON_EXTENSION_CHAIN_SIG.to_string(),
                HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG.to_string(),
            ],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2", "300")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2",
            )?
            .parse()?,
            reorg_protection: 2,
            transaction_config: TransactionChainConfig::new_l2()?,
            lane_config: LaneChainConfig::new_mainnet()?,
        })
    }

    fn try_create_optimism_mainnet_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: OPTIMISM_CHAIN_ID as u32,
            name: "Optimism Mainnet".to_string(),
            is_l1: false,
            rpc_url: Self::get_env_var("RPC_URL_OPTIMISM")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_OPTIMISM_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_OPT")?,
            fallback_ws_url: Self::get_env_var("WS_URL_OPT_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![
                M_USDC_MARKET,
                M_WETH_MARKET,
                M_USDT_MARKET,
                M_WBTC_MARKET,
                MWST_ETH_MARKET,
                MEZ_ETH_MARKET,
                MWE_ETH_MARKET,
                MWRS_ETH_MARKET,
            ],
            events: vec![EXTENSION_SUPPLIED_SIG.to_string(), EXTENSION_LIQUIDATE_SIG.to_string()],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2", "300")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2",
            )?
            .parse()?,
            reorg_protection: 2,
            transaction_config: TransactionChainConfig::new_l2()?,
            lane_config: LaneChainConfig::new_mainnet()?,
        })
    }

    fn try_create_ethereum_mainnet_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: ETHEREUM_CHAIN_ID as u32,
            name: "Ethereum Mainnet".to_string(),
            is_l1: true,
            rpc_url: Self::get_env_var("RPC_URL_ETHEREUM")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_ETHEREUM_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_ETH")?,
            fallback_ws_url: Self::get_env_var("WS_URL_ETH_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![
                M_USDC_MARKET,
                M_WETH_MARKET,
                M_USDT_MARKET,
                M_WBTC_MARKET,
                MWST_ETH_MARKET,
                MEZ_ETH_MARKET,
                MWE_ETH_MARKET,
                MWRS_ETH_MARKET,
            ],
            events: vec![EXTENSION_SUPPLIED_SIG.to_string(), EXTENSION_LIQUIDATE_SIG.to_string()],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1", "600")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1",
            )?
            .parse()?,
            reorg_protection: 0,
            transaction_config: TransactionChainConfig::new_l1()?,
            lane_config: LaneChainConfig::new_mainnet()?,
        })
    }

    fn try_create_base_mainnet_config(&self) -> Result<ChainConfig> {
        Ok(ChainConfig {
            chain_id: BASE_CHAIN_ID as u32,
            name: "Base Mainnet".to_string(),
            is_l1: false,
            rpc_url: Self::get_env_var("RPC_URL_BASE")?,
            fallback_rpc_url: Self::get_env_var("RPC_URL_BASE_FALLBACK")?,
            ws_url: Self::get_env_var("WS_URL_BASE")?,
            fallback_ws_url: Self::get_env_var("WS_URL_BASE_BACKUP")?,

            batch_submitter_address: Address::from_str(&Self::get_env_var(
                "BATCH_SUBMITTER_ADDRESS",
            )?)?,
            markets: vec![
                M_USDC_MARKET,
                M_WETH_MARKET,
                M_USDT_MARKET,
                M_WBTC_MARKET,
                MWST_ETH_MARKET,
                MEZ_ETH_MARKET,
                MWE_ETH_MARKET,
                MWRS_ETH_MARKET,
            ],
            events: vec![EXTENSION_SUPPLIED_SIG.to_string(), EXTENSION_LIQUIDATE_SIG.to_string()],
            max_block_delay_secs: Self::get_env_var_with_default("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2", "300")?
                .parse()?,
            block_delay: Self::get_env_var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2")?
                .parse()?,
            retarded_block_delay: Self::get_env_var(
                "RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2",
            )?
            .parse()?,
            reorg_protection: 2,
            transaction_config: TransactionChainConfig::new_l2()?,
            lane_config: LaneChainConfig::new_mainnet()?,
        })
    }
}

impl TransactionChainConfig {
    fn new_l1() -> Result<Self> {
        Ok(Self {
            max_retries: SequencerConfig::get_env_var("TRANSACTION_MANAGER_MAX_RETRIES_L1")?
                .parse()?,
            retry_delay: Duration::from_secs(
                SequencerConfig::get_env_var("TRANSACTION_MANAGER_RETRY_DELAY_L1")?.parse()?,
            ),
            submission_delay_seconds: SequencerConfig::get_env_var(
                "TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L1",
            )?
            .parse()?,
            poll_interval: Duration::from_secs(
                SequencerConfig::get_env_var("TRANSACTION_MANAGER_POLL_INTERVAL_L1")?.parse()?,
            ),
            max_tx: SequencerConfig::get_env_var("TRANSACTION_MANAGER_MAX_TX_L1")?.parse()?,
            tx_timeout: Duration::from_secs(
                SequencerConfig::get_env_var("TRANSACTION_MANAGER_TX_TIMEOUT_L1")?.parse()?,
            ),
            gas_percentage_increase_per_retry: SequencerConfig::get_env_var(
                "TRANSACTION_MANAGER_GAS_PERCENTAGE_INCREASE_PER_RETRY",
            )?
            .parse()?,
        })
    }

    fn new_l2() -> Result<Self> {
        Ok(Self {
            max_retries: SequencerConfig::get_env_var("TRANSACTION_MANAGER_MAX_RETRIES_L2")?
                .parse()?,
            retry_delay: Duration::from_secs(
                SequencerConfig::get_env_var("TRANSACTION_MANAGER_RETRY_DELAY_L2")?.parse()?,
            ),
            submission_delay_seconds: SequencerConfig::get_env_var(
                "TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L2",
            )?
            .parse()?,
            poll_interval: Duration::from_secs(
                SequencerConfig::get_env_var("TRANSACTION_MANAGER_POLL_INTERVAL_L2")?.parse()?,
            ),
            max_tx: SequencerConfig::get_env_var("TRANSACTION_MANAGER_MAX_TX_L2")?.parse()?,
            tx_timeout: Duration::from_secs(
                SequencerConfig::get_env_var("TRANSACTION_MANAGER_TX_TIMEOUT_L2")?.parse()?,
            ),
            gas_percentage_increase_per_retry: SequencerConfig::get_env_var(
                "TRANSACTION_MANAGER_GAS_PERCENTAGE_INCREASE_PER_RETRY",
            )?
            .parse()?,
        })
    }
}

impl LaneChainConfig {
    fn new_testnet() -> Result<Self> {
        Ok(Self {
            max_volume: 4000000,
            time_interval: Duration::from_secs(3600),
            block_delay: 10,
            reorg_protection: 2,
        })
    }

    fn new_mainnet() -> Result<Self> {
        Ok(Self {
            max_volume: 10000000, // Higher volume limits for mainnet
            time_interval: Duration::from_secs(3600),
            block_delay: 10,
            reorg_protection: 2,
        })
    }
}

impl ProofConfig {
    fn new() -> Result<Self> {
        Ok(Self {
            batch_limit: SequencerConfig::get_env_var("PROOF_GENERATOR_BATCH_LIMIT")?.parse()?,
            max_retries: SequencerConfig::get_env_var("PROOF_GENERATOR_MAX_PROOF_RETRIES")?
                .parse()?,
            retry_delay: Duration::from_secs(
                SequencerConfig::get_env_var("PROOF_GENERATOR_PROOF_RETRY_DELAY")?.parse()?,
            ),
            request_delay: Duration::from_secs(
                SequencerConfig::get_env_var("PROOF_GENERATOR_PROOF_REQUEST_DELAY")?.parse()?,
            ),
            dummy_mode: SequencerConfig::get_env_var("PROOF_GENERATOR_DUMMY_MODE")?.parse()?,
        })
    }
}

impl EventListenerConfig {
    fn new() -> Result<Self> {
        Ok(Self {
            max_retries: SequencerConfig::get_env_var("EVENT_LISTENER_MAX_RETRIES")?.parse()?,
            retry_delay_secs: SequencerConfig::get_env_var("EVENT_LISTENER_RETRY_DELAY_SECS")?
                .parse()?,
            poll_interval_secs: SequencerConfig::get_env_var("EVENT_LISTENER_POLL_INTERVAL_SECS")?
                .parse()?,
            block_range_offset_l1_from: SequencerConfig::get_env_var(
                "EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM",
            )?
            .parse()?,
            block_range_offset_l1_to: SequencerConfig::get_env_var(
                "EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO",
            )?
            .parse()?,
            block_range_offset_l2_from: SequencerConfig::get_env_var(
                "EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM",
            )?
            .parse()?,
            block_range_offset_l2_to: SequencerConfig::get_env_var(
                "EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO",
            )?
            .parse()?,
            retarded_max_retries: SequencerConfig::get_env_var(
                "RETARDED_EVENT_LISTENER_MAX_RETRIES",
            )?
            .parse()?,
            retarded_retry_delay_secs: SequencerConfig::get_env_var(
                "RETARDED_EVENT_LISTENER_RETRY_DELAY_SECS",
            )?
            .parse()?,
            retarded_poll_interval_secs: SequencerConfig::get_env_var(
                "RETARDED_EVENT_LISTENER_POLL_INTERVAL_SECS",
            )?
            .parse()?,
            retarded_block_range_offset_l1_from: SequencerConfig::get_env_var(
                "RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM",
            )?
            .parse()?,
            retarded_block_range_offset_l1_to: SequencerConfig::get_env_var(
                "RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO",
            )?
            .parse()?,
            retarded_block_range_offset_l2_from: SequencerConfig::get_env_var(
                "RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM",
            )?
            .parse()?,
            retarded_block_range_offset_l2_to: SequencerConfig::get_env_var(
                "RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO",
            )?
            .parse()?,
        })
    }
}

impl RestartConfig {
    fn new() -> Result<Self> {
        Ok(Self {
            listener_delay_seconds: SequencerConfig::get_env_var("RESTART_LISTENER_DELAY_SECONDS")?
                .parse()?,
            listener_interval_seconds: SequencerConfig::get_env_var(
                "RESTART_LISTENER_INTERVAL_SECONDS",
            )?
            .parse()?,
        })
    }
}

impl GasFeeDistributerConfig {
    fn new() -> Result<Self> {
        let poll_interval_secs = 60; // Default polling interval

        let bridge_fee_percentage: u32 = std::env::var("BRIDGE_FEE_PERCENTAGE")
            .expect("BRIDGE_FEE_PERCENTAGE must be set in .env")
            .parse()
            .unwrap();

        let min_amount_to_bridge = U256::from_str(
            &std::env::var("MIN_AMOUNT_TO_BRIDGE")
                .expect("MIN_AMOUNT_TO_BRIDGE must be set in .env"),
        )
        .unwrap();

        let mut minimum_sequencer_balance_per_chain = HashMap::new();
        let mut min_distributor_balance_per_chain = HashMap::new();
        let mut target_sequencer_balance_per_chain = HashMap::new();
        let mut minimum_harvest_balance_per_chain = HashMap::new();

        // Add balances for each chain if environment variables are set
        if let Ok(min_seq_eth) = std::env::var("MIN_SEQUENCER_BALANCE_ETHEREUM") {
            let balance = U256::from_str(&min_seq_eth).unwrap();
            minimum_sequencer_balance_per_chain.insert(ETHEREUM_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(min_seq_linea) = std::env::var("MIN_SEQUENCER_BALANCE_LINEA") {
            let balance = U256::from_str(&min_seq_linea).unwrap();
            minimum_sequencer_balance_per_chain.insert(LINEA_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(min_seq_opt) = std::env::var("MIN_SEQUENCER_BALANCE_BASE") {
            let balance = U256::from_str(&min_seq_opt).unwrap();
            minimum_sequencer_balance_per_chain.insert(OPTIMISM_SEPOLIA_CHAIN_ID as u64, balance);
        }

        // Add distributor balances
        if let Ok(min_dist_eth) = std::env::var("MIN_DISTRIBUTOR_BALANCE_ETHEREUM") {
            let balance = U256::from_str(&min_dist_eth).unwrap();
            min_distributor_balance_per_chain.insert(ETHEREUM_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(min_dist_linea) = std::env::var("MIN_DISTRIBUTOR_BALANCE_LINEA") {
            let balance = U256::from_str(&min_dist_linea).unwrap();
            min_distributor_balance_per_chain.insert(LINEA_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(min_dist_opt) = std::env::var("MIN_DISTRIBUTOR_BALANCE_BASE") {
            let balance = U256::from_str(&min_dist_opt).unwrap();
            min_distributor_balance_per_chain.insert(OPTIMISM_SEPOLIA_CHAIN_ID as u64, balance);
        }

        // Add target sequencer balances
        if let Ok(target_seq_eth) = std::env::var("TARGET_SEQUENCER_BALANCE_ETHEREUM") {
            let balance = U256::from_str(&target_seq_eth).unwrap();
            target_sequencer_balance_per_chain.insert(ETHEREUM_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(target_seq_linea) = std::env::var("TARGET_SEQUENCER_BALANCE_LINEA") {
            let balance = U256::from_str(&target_seq_linea).unwrap();
            target_sequencer_balance_per_chain.insert(LINEA_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(target_seq_opt) = std::env::var("TARGET_SEQUENCER_BALANCE_BASE") {
            let balance = U256::from_str(&target_seq_opt).unwrap();
            target_sequencer_balance_per_chain.insert(OPTIMISM_SEPOLIA_CHAIN_ID as u64, balance);
        }

        // Add minimum harvest balances
        if let Ok(min_harvest_eth) = std::env::var("MIN_HARVEST_BALANCE_ETHEREUM") {
            let balance = U256::from_str(&min_harvest_eth).unwrap();
            minimum_harvest_balance_per_chain.insert(ETHEREUM_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(min_harvest_linea) = std::env::var("MIN_HARVEST_BALANCE_LINEA") {
            let balance = U256::from_str(&min_harvest_linea).unwrap();
            minimum_harvest_balance_per_chain.insert(LINEA_SEPOLIA_CHAIN_ID as u64, balance);
        }
        if let Ok(min_harvest_opt) = std::env::var("MIN_HARVEST_BALANCE_BASE") {
            let balance = U256::from_str(&min_harvest_opt).unwrap();
            minimum_harvest_balance_per_chain.insert(OPTIMISM_SEPOLIA_CHAIN_ID as u64, balance);
        }

        Ok(Self {
            poll_interval_secs,
            bridge_fee_percentage,
            min_amount_to_bridge,
            minimum_sequencer_balance_per_chain,
            min_distributor_balance_per_chain,
            target_sequencer_balance_per_chain,
            minimum_harvest_balance_per_chain,
        })
    }
}

impl EventProofReadyCheckerConfig {
    fn new() -> Result<Self> {
        let poll_interval_seconds = std::env::var("PROOF_READY_CHECKER_POOL_INTERVAL_SECONDS")
            .expect("PROOF_READY_CHECKER_POOL_INTERVAL_SECONDS must be set in .env")
            .parse::<u64>()
            .unwrap();

        let block_update_interval_seconds =
            std::env::var("PROOF_READY_CHECKER_BLOCK_UPDATE_INTERVAL_SECONDS")
                .expect("PROOF_READY_CHECKER_BLOCK_UPDATE_INTERVAL_SECONDS must be set in .env")
                .parse::<u64>()
                .unwrap();

        Ok(Self {
            poll_interval_seconds,
            block_update_interval_seconds,
        })
    }
}

impl ResetTxManagerConfig {
    fn new() -> Result<Self> {
        let sample_size: i64 = std::env::var("RESET_TX_MANAGER_SAMPLE_SIZE")
            .expect("RESET_TX_MANAGER_SAMPLE_SIZE must be set in .env")
            .parse::<u64>()
            .unwrap()
            .try_into()
            .unwrap();

        let multiplier = std::env::var("RESET_TX_MANAGER_MULTIPLIER")
            .expect("RESET_TX_MANAGER_MULTIPLIER must be set in .env")
            .parse::<f64>()
            .unwrap();

        let max_retries: u32 = std::env::var("RESET_TX_MANAGER_MAX_RETRIES")
            .expect("RESET_TX_MANAGER_MAX_RETRIES must be set in .env")
            .parse::<u64>()
            .unwrap()
            .try_into()
            .unwrap();

        let retry_delay_secs = std::env::var("RESET_TX_MANAGER_RETRY_DELAY_SECS")
            .expect("RESET_TX_MANAGER_RETRY_DELAY_SECS must be set in .env")
            .parse::<u64>()
            .unwrap();

        let poll_interval_secs = std::env::var("RESET_TX_MANAGER_POLL_INTERVAL_SECS")
            .expect("RESET_TX_MANAGER_POLL_INTERVAL_SECS must be set in .env")
            .parse::<u64>()
            .unwrap();

        let batch_limit: i64 = std::env::var("RESET_TX_MANAGER_BATCH_LIMIT")
            .expect("RESET_TX_MANAGER_BATCH_LIMIT must be set in .env")
            .parse::<u64>()
            .unwrap()
            .try_into()
            .unwrap();

        let max_retries_reset: i64 = max_retries as i64;

        let api_key =
            std::env::var("REBALANCER_API_KEY").expect("REBALANCER_API_KEY must be set in .env");

        let rebalancer_url =
            std::env::var("REBALANCER_URL").expect("REBALANCER_URL must be set in .env");

        let rebalance_delay = std::env::var("REBALANCER_DELAY_SECONDS")
            .expect("REBALANCER_DELAY_SECONDS must be set in .env")
            .parse::<u64>()
            .unwrap();

        let minimum_usd_value = std::env::var("REBALANCER_MINIMUM_USD_VALUE")
            .expect("REBALANCER_MINIMUM_USD_VALUE must be set in .env")
            .parse::<f64>()
            .unwrap();

        Ok(Self {
            sample_size,
            multiplier,
            max_retries,
            retry_delay_secs,
            poll_interval_secs,
            batch_limit,
            max_retries_reset,
            api_key,
            rebalancer_url,
            rebalance_delay,
            minimum_usd_value,
        })
    }
}
