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
pub struct ChainConfig {
    pub chain_id: u64,
    pub rpc_url: String,
    pub fallback_rpc_url: String,
    pub is_l2: bool,
}

// Define L1 block contract addresses
const L1_BLOCK_ADDRESS_OPTIMISM: &str = "0x4200000000000000000000000000000000000015";
const L1_BLOCK_ADDRESS_OPTIMISM_SEPOLIA: &str = "0x4200000000000000000000000000000000000015";

pub struct EventProofReadyChecker {
    db: Database,
    poll_interval: Duration,
    block_update_interval: Duration,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    chain_configs: Vec<ChainConfig>,
}

impl EventProofReadyChecker {
    pub fn new(
        db: Database, 
        poll_interval: Duration, 
        block_update_interval: Duration,
        chain_configs: Vec<ChainConfig>,
    ) -> Self {
        // Initialize the block number map with default values for all supported chains
        let mut block_numbers = BLOCK_NUMBERS.lock().unwrap();
        
        // Initialize block numbers for all tracked chains
        for config in &chain_configs {
            block_numbers.insert(config.chain_id, AtomicI32::new(0));
        }

        // Start the background task to update block numbers
        let chain_configs_clone = chain_configs.clone();
        let block_update_interval_clone = block_update_interval;
        tokio::spawn(async move {
            let mut interval = interval(block_update_interval_clone);
            
            // Create providers for each chain
            let mut providers = HashMap::new();
            let mut fallback_providers = HashMap::new();
            for config in &chain_configs_clone {
                // Create primary provider
                match create_provider(
                    Url::parse(&config.rpc_url).unwrap(),
                ).await {
                    Ok(provider) => {
                        providers.insert(config.chain_id, provider);
                    }
                    Err(e) => {
                        error!("Failed to create primary provider for chain {}: {}", config.chain_id, e);
                    }
                }

                // Create fallback provider
                match create_provider(
                    Url::parse(&config.fallback_rpc_url).unwrap(),
                ).await {
                    Ok(provider) => {
                        fallback_providers.insert(config.chain_id, provider);
                    }
                    Err(e) => {
                        error!("Failed to create fallback provider for chain {}: {}", config.chain_id, e);
                    }
                }
            }

            // Get Optimism provider for L1 block contract
            let optimism_provider = match providers.get(&OPTIMISM_SEPOLIA_CHAIN_ID) {
                Some(provider) => provider.clone(),
                None => {
                    error!("Optimism provider not found, cannot get L1 block numbers");
                    return;
                }
            };

            // Get Optimism fallback provider for L1 block contract
            let optimism_fallback_provider = match fallback_providers.get(&OPTIMISM_SEPOLIA_CHAIN_ID) {
                Some(provider) => provider.clone(),
                None => {
                    error!("Optimism fallback provider not found");
                    return;
                }
            };
            
            // Create L1 block contract instances
            let l1_block_contract = new(
                L1_BLOCK_ADDRESS_OPTIMISM_SEPOLIA.parse::<Address>().unwrap(), 
                optimism_provider
            );

            let l1_block_fallback_contract = new(
                L1_BLOCK_ADDRESS_OPTIMISM_SEPOLIA.parse::<Address>().unwrap(), 
                optimism_fallback_provider
            );
            
            loop {
                interval.tick().await;
                
                // Get Ethereum Sepolia block number via Optimism Sepolia with fallback
                match l1_block_contract.number().call().await {
                    Ok(number_return) => {
                        let block_number = number_return._0;
                        let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                        if let Some(atomic) = block_numbers.get(&ETHEREUM_SEPOLIA_CHAIN_ID) {
                            let block_number_i32 = block_number.try_into().unwrap_or(i32::MAX);
                            atomic.store(block_number_i32, Ordering::SeqCst);
                            debug!("Updated Ethereum Sepolia block number: {}", block_number_i32);
                        }
                    }
                    Err(e) => {
                        warn!("Primary L1 block contract failed: {:?}, trying fallback", e);
                        match l1_block_fallback_contract.number().call().await {
                            Ok(number_return) => {
                                let block_number = number_return._0;
                                let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                                if let Some(atomic) = block_numbers.get(&ETHEREUM_SEPOLIA_CHAIN_ID) {
                                    let block_number_i32 = block_number.try_into().unwrap_or(i32::MAX);
                                    atomic.store(block_number_i32, Ordering::SeqCst);
                                    debug!("Updated Ethereum Sepolia block number from fallback: {}", block_number_i32);
                                }
                            }
                            Err(e) => {
                                error!("Both L1 block contracts failed: {:?}", e);
                            }
                        }
                    }
                }
                
                // Update other chain block numbers
                for config in &chain_configs_clone {
                    // Skip Ethereum Sepolia as it's handled above
                    if config.chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
                        continue;
                    }
                    
                    // Get the providers for this chain
                    if let (Some(provider), Some(fallback_provider)) = (
                        providers.get(&config.chain_id),
                        fallback_providers.get(&config.chain_id)
                    ) {
                        // Get the current block number with fallback
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
                                warn!("Primary provider failed for chain {}: {:?}, trying fallback", config.chain_id, e);
                                match fallback_provider.get_block_number().await {
                                    Ok(block_number) => {
                                        let block_number_i32 = block_number as i32;
                                        {
                                            let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                                            if let Some(atomic) = block_numbers.get(&config.chain_id) {
                                                atomic.store(block_number_i32, Ordering::SeqCst);
                                                debug!("Updated block number for chain {} from fallback: {}", config.chain_id, block_number_i32);
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Both providers failed for chain {}: {:?}", config.chain_id, e);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        });

        Self { 
            db, 
            poll_interval, 
            block_update_interval,
            task_handle: None,
            chain_configs,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        // Cancel any existing task
        if let Some(handle) = self.task_handle.take() {
            handle.abort();
            let _ = handle.await; // Wait for the task to finish
        }

        // Spawn new task
        let db = self.db.clone();
        let poll_interval = self.poll_interval;
        let block_update_interval = self.block_update_interval;
        let chain_configs = self.chain_configs.clone();
        
        let handle = tokio::spawn(async move {
            let checker = EventProofReadyChecker {
                db,
                poll_interval,
                block_update_interval,
                task_handle: None,
                chain_configs,
            };
            
            if let Err(e) = checker.run().await {
                error!("Event proof ready checker failed: {:?}", e);
            }
        });

        self.task_handle = Some(handle);
        Ok(())
    }

    async fn run(&self) -> Result<()> {
        info!("Starting event proof ready checker with optimized DB query");
        
        let mut interval = interval(self.poll_interval);
        
        loop {
            interval.tick().await;
            
            // 1. Get current block numbers for tracked chains
            let mut current_block_map = HashMap::new();
            {
                let block_numbers_lock = BLOCK_NUMBERS.lock().unwrap();
                for config in &self.chain_configs {
                    if let Some(atomic_block) = block_numbers_lock.get(&config.chain_id) {
                        let block_num = atomic_block.load(Ordering::SeqCst);
                        if block_num > 0 { // Only include chains where we have a valid block number
                           current_block_map.insert(config.chain_id, block_num);
                        }
                    } else {
                        warn!("No block number found in map for tracked chain ID: {}", config.chain_id);
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
                }
            }
        }
    }
    
    pub fn get_block_number(&self, chain_id: u64) -> i32 {
        if let Some(atomic) = BLOCK_NUMBERS.lock().unwrap().get(&chain_id) {
            atomic.load(Ordering::SeqCst)
        } else {
            0
        }
    }
}

impl Drop for EventProofReadyChecker {
    fn drop(&mut self) {
        if let Some(handle) = self.task_handle.take() {
            info!("Cleaning up event proof ready checker task");
            handle.abort();
        }
    }
}

// Helper function to create a provider
async fn create_provider(
    rpc_url: Url,
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
    // Use a dummy private key since we only need view calls
    let dummy_key = "0000000000000000000000000000000000000000000000000000000000000001";
    let signer: PrivateKeySigner = dummy_key.parse().expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url);

    Ok(provider)
} 