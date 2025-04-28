use alloy::{
    network::EthereumWallet,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    transports::http::reqwest::Url,
    primitives::{Address, TxHash},
};
use eyre::Result;
use futures::future::join_all;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use malda_rs::constants::*;
use malda_rs::types::IL1Block;
use crate::types::IL1Block::new;

use crate::constants::{
    ETHEREUM_SEPOLIA_CHAIN_ID, LINEA_SEPOLIA_CHAIN_ID, OPTIMISM_SEPOLIA_CHAIN_ID,
};
use sequencer::database::{Database, EventStatus, EventUpdate};

// Define a type for the block number map
type BlockNumberMap = HashMap<u64, AtomicI32>;

// Create a lazy static block number map
lazy_static! {
    static ref BLOCK_NUMBERS: Mutex<BlockNumberMap> = Mutex::new(BlockNumberMap::new());
}

// Define chain configurations
#[derive(Clone)]
struct ChainConfig {
    chain_id: u64,
    rpc_url: String,
    is_l2: bool,
}

// Define L1 block contract addresses
const L1_BLOCK_ADDRESS_OPTIMISM: &str = "0x4200000000000000000000000000000000000015";
const L1_BLOCK_ADDRESS_OPTIMISM_SEPOLIA: &str = "0x4200000000000000000000000000000000000015";

#[derive(Clone)]
pub struct EventProofReadyChecker {
    db: Database,
    poll_interval: Duration,
    block_update_interval: Duration,
    tracked_chain_ids: Vec<u64>,
}

impl EventProofReadyChecker {
    pub fn new(db: Database, poll_interval: Duration, block_update_interval: Duration) -> Self {
        // Initialize the block number map with default values for all supported chains
        let mut block_numbers = BLOCK_NUMBERS.lock().unwrap();
        
        // Mainnet chains
        block_numbers.insert(ETHEREUM_CHAIN_ID, AtomicI32::new(0));
        block_numbers.insert(OPTIMISM_CHAIN_ID, AtomicI32::new(0));
        block_numbers.insert(LINEA_CHAIN_ID, AtomicI32::new(0));
        block_numbers.insert(BASE_CHAIN_ID, AtomicI32::new(0));
        
        // Testnet chains
        block_numbers.insert(ETHEREUM_SEPOLIA_CHAIN_ID, AtomicI32::new(0));
        block_numbers.insert(OPTIMISM_SEPOLIA_CHAIN_ID, AtomicI32::new(0));
        block_numbers.insert(LINEA_SEPOLIA_CHAIN_ID, AtomicI32::new(0));
        block_numbers.insert(BASE_SEPOLIA_CHAIN_ID, AtomicI32::new(0));
        
        // Define chain configurations
        let chain_configs = vec![
            // Mainnet chains
            // ChainConfig {
            //     chain_id: ETHEREUM_CHAIN_ID,
            //     rpc_url: rpc_url_ethereum().to_string(),
            //     is_l2: false,
            // },
            // ChainConfig {
            //     chain_id: OPTIMISM_CHAIN_ID,
            //     rpc_url: rpc_url_optimism().to_string(),
            //     is_l2: true,
            // },
            // ChainConfig {
            //     chain_id: LINEA_CHAIN_ID,
            //     rpc_url: rpc_url_linea().to_string(),
            //     is_l2: true,
            // },
            // ChainConfig {
            //     chain_id: BASE_CHAIN_ID,
            //     rpc_url: rpc_url_base().to_string(),
            //     is_l2: true,
            // },
            // Testnet chains
            ChainConfig {
                chain_id: ETHEREUM_SEPOLIA_CHAIN_ID,
                rpc_url: rpc_url_ethereum_sepolia().to_string(),
                is_l2: false,
            },
            ChainConfig {
                chain_id: OPTIMISM_SEPOLIA_CHAIN_ID,
                rpc_url: rpc_url_optimism_sepolia().to_string(),
                is_l2: true,
            },
            ChainConfig {
                chain_id: LINEA_SEPOLIA_CHAIN_ID,
                rpc_url: rpc_url_linea_sepolia().to_string(),
                is_l2: true,
            },
            // ChainConfig {
            //     chain_id: BASE_SEPOLIA_CHAIN_ID,
            //     rpc_url: rpc_url_base_sepolia().to_string(),
            //     is_l2: true,
            // },
        ];
        
        // Store chain IDs for easy access later in the start loop
        let tracked_chain_ids: Vec<u64> = chain_configs.iter().map(|c| c.chain_id).collect();
        
        // Start the background task to update block numbers
        let chain_configs_clone = chain_configs.clone();
        let block_update_interval_clone = block_update_interval;
        tokio::spawn(async move {
            let mut interval = interval(block_update_interval_clone);
            
            // Create provider for Optimism Sepolia
            let provider = create_provider(
                Url::parse(&rpc_url_optimism_sepolia()).unwrap(),
                "0xbd0974bec39a17e36ba2a6b4d238ff944bacb481cbed5efcae784d7bf4a2ff80",
            )
            .await
            .map_err(|e| eyre::eyre!("Failed to create provider: {}", e))
            .unwrap();
            
            // Create L1 block contract instance
            let l1_block_contract = new(L1_BLOCK_ADDRESS_OPTIMISM_SEPOLIA.parse::<Address>().unwrap(), provider);
            
            // Create a map to store providers for each chain to avoid recreating them on every iteration
            let mut chain_providers: HashMap<u64, alloy::providers::fillers::FillProvider<
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::JoinFill<
                        alloy::providers::Identity,
                        alloy::providers::fillers::JoinFill<
                            alloy::providers::fillers::GasFiller,
                            alloy::providers::fillers::JoinFill<
                                alloy::providers::fillers::BlobGasFiller,
                                alloy::providers::fillers::JoinFill<
                                    alloy::providers::fillers::NonceFiller,
                                    alloy::providers::fillers::ChainIdFiller
                                >
                            >
                        >
                    >,
                    alloy::providers::fillers::WalletFiller<alloy::network::EthereumWallet>
                >,
                alloy::providers::RootProvider<alloy::transports::http::Http<alloy::transports::http::Client>>,
                alloy::transports::http::Http<alloy::transports::http::Client>,
                alloy::network::Ethereum
            >> = HashMap::new();
            
            loop {
                interval.tick().await;
                
                // Get Ethereum Sepolia block number via Optimism Sepolia
                match l1_block_contract.number().call().await {
                    Ok(number_return) => {
                        let block_number = number_return._0;
                        let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                        if let Some(atomic) = block_numbers.get(&ETHEREUM_SEPOLIA_CHAIN_ID) {
                            // Convert u64 to i32, handling potential overflow
                            let block_number_i32 = block_number.try_into().unwrap_or(i32::MAX);
                            atomic.store(block_number_i32, Ordering::SeqCst);
                            debug!("Updated Ethereum Sepolia block number: {}", block_number_i32);
                        }
                    }
                    Err(e) => {
                        error!("Failed to fetch Ethereum Sepolia block number: {}", e);
                    }
                }
                
                // Update other chain block numbers
                for config in &chain_configs_clone {
                    // Skip Ethereum Sepolia as it's handled above
                    if config.chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
                        continue;
                    }
                    
                    // Get or create a provider for this chain
                    let provider = if let Some(existing_provider) = chain_providers.get(&config.chain_id) {
                        existing_provider.clone()
                    } else {
                        match create_provider(
                            Url::parse(&config.rpc_url).unwrap(),
                            "0xbd0974bec39a17e36ba2a6b4d238ff944bacb481cbed5efcae784d7bf4a2ff80",
                        )
                        .await
                        {
                            Ok(p) => {
                                let provider_clone = p.clone();
                                chain_providers.insert(config.chain_id, p);
                                provider_clone
                            }
                            Err(e) => {
                                error!("Failed to create provider for chain {}: {}", config.chain_id, e);
                                continue;
                            }
                        }
                    };
                    
                    // Get the current block number
                    match provider.get_block_number().await {
                        Ok(block_number) => {
                            let block_number_i32 = block_number as i32;
                            {
                                let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                                if let Some(atomic) = block_numbers.get(&config.chain_id) {
                                    atomic.store(block_number_i32, Ordering::SeqCst);
                                    debug!("Updated block number for chain {}: {}", config.chain_id, block_number_i32);
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to fetch block number for chain {}: {}", config.chain_id, e);
                        }
                    }
                }
            }
        });
        
        Self {
            db,
            poll_interval,
            block_update_interval,
            tracked_chain_ids,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting event proof ready checker with optimized DB query");
        
        let mut interval = interval(self.poll_interval);
        
        loop {
            interval.tick().await;
            
            // 1. Get current block numbers for tracked chains
            let mut current_block_map = HashMap::new();
            {
                let block_numbers_lock = BLOCK_NUMBERS.lock().unwrap();
                for &chain_id in &self.tracked_chain_ids {
                    if let Some(atomic_block) = block_numbers_lock.get(&chain_id) {
                        let block_num = atomic_block.load(Ordering::SeqCst);
                        if block_num > 0 { // Only include chains where we have a valid block number
                           current_block_map.insert(chain_id, block_num);
                        }
                    } else {
                        warn!("No block number found in map for tracked chain ID: {}", chain_id);
                    }
                }
            } // Lock released here

            if current_block_map.is_empty() {
                debug!("No valid current block numbers available yet, skipping DB update.");
                continue;
            }
            
            // 2. Call the optimized database function
            match self.db.update_events_to_ready_status(&current_block_map).await {
                Ok(_) => { /* Success logged within the DB function */ }
                Err(e) => {
                    error!("Failed to update events to ready status: {:?}", e);
                    // Decide on error handling: continue, break, retry?
                }
            }
        }
        // Note: The loop never exits in this structure, similar to before.
        // Ok(()) // Unreachable
    }
    
    pub fn get_block_number(&self, chain_id: u64) -> i32 {
        // Synchronous version of get_current_block_number
        if let Some(atomic) = BLOCK_NUMBERS.lock().unwrap().get(&chain_id) {
            atomic.load(Ordering::SeqCst)
        } else {
            0
        }
    }
}

// Helper function to create a provider
async fn create_provider(
    rpc_url: Url,
    private_key: &str,
) -> Result<alloy::providers::fillers::FillProvider<
    alloy::providers::fillers::JoinFill<
        alloy::providers::fillers::JoinFill<
            alloy::providers::Identity,
            alloy::providers::fillers::JoinFill<
                alloy::providers::fillers::GasFiller,
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::BlobGasFiller,
                    alloy::providers::fillers::JoinFill<
                        alloy::providers::fillers::NonceFiller,
                        alloy::providers::fillers::ChainIdFiller
                    >
                >
            >
        >,
        alloy::providers::fillers::WalletFiller<alloy::network::EthereumWallet>
    >,
    alloy::providers::RootProvider<alloy::transports::http::Http<alloy::transports::http::Client>>,
    alloy::transports::http::Http<alloy::transports::http::Client>,
    alloy::network::Ethereum
>> {
    let signer: PrivateKeySigner = private_key.parse().expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url);

    Ok(provider)
} 