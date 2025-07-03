use alloy::{
    network::EthereumWallet,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    transports::http::reqwest::Url,
    primitives::Address,
    eips::BlockNumberOrTag,
};
use eyre::{Result, WrapErr};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, error, info, warn};

use crate::types::IL1Block::new;

use crate::constants::{
    ETHEREUM_CHAIN_ID, OPTIMISM_CHAIN_ID,
};
use sequencer::database::Database;

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
    pub max_block_delay_secs: u64, // Maximum acceptable block delay in seconds
}

// Define L1 block contract addresses
const L1_BLOCK_ADDRESS_OPTIMISM: &str = "0x4200000000000000000000000000000000000015";
const L1_BLOCK_ADDRESS_BASE: &str = "0x4200000000000000000000000000000000000015";

pub struct EventProofReadyChecker {
    db: Database,
    poll_interval: Duration,
    block_update_interval: Duration,
    task_handle: Option<tokio::task::JoinHandle<()>>,
    chain_configs: Vec<ChainConfig>,
}

// Provider type alias for readability
type ProviderType = alloy::providers::fillers::FillProvider<
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
    alloy::providers::RootProvider<alloy::network::Ethereum>
>;

// Chain provider state
struct ChainProviderState {
    primary_provider: Option<ProviderType>,
    config: ChainConfig,
    used_fallback: bool,
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
        
        // Initialize provider state for each chain
        let mut chain_providers = HashMap::new();
        for config in &self.chain_configs {
            chain_providers.insert(config.chain_id, ChainProviderState {
                primary_provider: None,
                config: config.clone(),
                used_fallback: false,
            });
        }
        
        // Start the block update task
        let mut interval = interval(self.block_update_interval);
        let mut iteration_count = 0;
        loop {
            interval.tick().await;
            
            // Update block numbers for all chains
            let mut current_block_map = HashMap::new();
            
            // Special handling for Ethereum via Optimism L1 block contract
            if let Some(optimism_state) = chain_providers.get_mut(&OPTIMISM_CHAIN_ID) {
                // Get active provider for Optimism
                let (optimism_provider, is_fallback) = match Self::get_active_provider(optimism_state, &self.db).await {
                    Ok((provider, is_fallback)) => {
                        optimism_state.used_fallback = is_fallback;
                        (provider, is_fallback)
                    },
                    Err(e) => {
                        error!("Failed to get Optimism provider: {:?}", e);
                        continue;
                    }
                };
                
                // Create L1 block contract
                let l1_block_contract = new(
                    L1_BLOCK_ADDRESS_OPTIMISM.parse::<Address>().unwrap(), 
                    optimism_provider
                );
                
                // Get Ethereum block number via Optimism contract
                match l1_block_contract.number().call().await {
                    Ok(number_return) => {
                        let block_number = number_return;
                        let block_number_i32 = block_number.try_into().unwrap_or(i32::MAX);
                        
                        // Update the block number in the map
                        let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                        if let Some(atomic) = block_numbers.get(&ETHEREUM_CHAIN_ID) {
                            atomic.store(block_number_i32, Ordering::SeqCst);
                            debug!("Updated Ethereum block number{}: {}", 
                                if is_fallback { " (via fallback)" } else { "" }, 
                                block_number_i32);
                            
                            current_block_map.insert(ETHEREUM_CHAIN_ID, block_number_i32);
                        }
                    },
                    Err(e) => {
                        error!("Failed to get L1 block number from Optimism: {:?}", e);
                        optimism_state.used_fallback = true; // Force new provider next time
                    }
                }
            }
            
            // Update block numbers for other chains
            for (chain_id, state) in chain_providers.iter_mut() {

                
                // Get active provider for this chain
                let (provider, is_fallback) = match Self::get_active_provider(state, &self.db).await {
                    Ok((provider, is_fallback)) => {
                        state.used_fallback = is_fallback;
                        (provider, is_fallback)
                    },
                    Err(e) => {
                        error!("Failed to get provider for chain {}: {:?}", chain_id, e);
                        continue;
                    }
                };

                // Skip Ethereum as it's handled above
                if *chain_id == ETHEREUM_CHAIN_ID {
                    continue;
                }
                
                // Get block number
                match provider.get_block_number().await {
                    Ok(block_number) => {
                        let block_number_i32 = block_number as i32;
                        // Update the block number in the map
                        let block_numbers = BLOCK_NUMBERS.lock().unwrap();
                        if let Some(atomic) = block_numbers.get(chain_id) {
                            atomic.store(block_number_i32, Ordering::SeqCst);
                            debug!("Updated block number for chain {}{}: {}", 
                                chain_id, 
                                if is_fallback { " (via fallback)" } else { "" }, 
                                block_number_i32);
                            current_block_map.insert(*chain_id, block_number_i32);
                        }
                    },
                    Err(e) => {
                        error!("Failed to get block number for chain {}: {:?}", chain_id, e);
                        state.used_fallback = true; // Force new provider next time
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
            match self.db.update_events_to_ready_status(&current_block_map).await {
                Ok(_) => { /* Success logged within the DB function */ }
                Err(e) => {
                    error!("Failed to update events to ready status: {:?}", e);
                }
            }
            
            // Sleep for poll interval before checking for events again
            tokio::time::sleep(self.poll_interval).await;
            iteration_count += 1;
        }
    }
    
    // Helper method to get an active provider for a chain
    async fn get_active_provider(state: &mut ChainProviderState, db: &Database) -> Result<(ProviderType, bool)> {
        // If we have a primary provider and don't need to force a new one, check if it's still valid
        if !state.used_fallback && state.primary_provider.is_some() {
            let provider = state.primary_provider.as_ref().unwrap();
            
            // Check if the primary provider is fresh enough
            match provider.get_block_by_number(BlockNumberOrTag::Latest).await {
                Ok(Some(block)) => {
                    // Check block timestamp
                    let block_timestamp: u64 = block.header.inner.timestamp.into();
                    let current_time = chrono::Utc::now().timestamp() as u64;
                    let max_delay = state.config.max_block_delay_secs;

                    if current_time > block_timestamp && (current_time - block_timestamp) > max_delay {
                        let reason = format!(
                            "Primary provider's latest block is too old: block_time={}, current_time={}, diff={}s, max_delay={}s",
                            block_timestamp, current_time, current_time - block_timestamp, max_delay
                        );
                        
                        warn!(
                            "{} for chain {}, trying fallback",
                            reason, state.config.chain_id
                        );
                        
                        // Record the failure in the database
                        if let Err(e) = db.add_to_node_status(
                            state.config.chain_id,
                            &state.config.rpc_url,
                            &state.config.fallback_rpc_url,
                            &reason
                        ).await {
                            error!("Failed to record node status: {:?}", e);
                        }
                        
                        // Use fallback and mark primary as invalid
                        state.primary_provider = None;
                        return Self::create_fallback_provider(&state.config, db).await;
                    }
                    
                    // Primary provider is good, use it
                    debug!("Reusing existing primary provider for chain {}", state.config.chain_id);
                    return Ok((provider.clone(), false));
                },
                Ok(None) => {
                    let reason = format!("Existing primary provider returned no latest block");
                    warn!("{} for chain {}, creating new one", reason, state.config.chain_id);
                    
                    // Record the failure in the database
                    if let Err(e) = db.add_to_node_status(
                        state.config.chain_id,
                        &state.config.rpc_url,
                        &state.config.fallback_rpc_url,
                        &reason
                    ).await {
                        error!("Failed to record node status: {:?}", e);
                    }
                    
                    // Primary provider failed, clear it and try creating a new one
                    state.primary_provider = None;
                }
                Err(e) => {
                    let reason = format!("Existing primary provider failed to get latest block: {:?}", e);
                    warn!("{} for chain {}, creating new one", reason, state.config.chain_id);
                    
                    // Record the failure in the database
                    if let Err(e) = db.add_to_node_status(
                        state.config.chain_id,
                        &state.config.rpc_url,
                        &state.config.fallback_rpc_url,
                        &reason
                    ).await {
                        error!("Failed to record node status: {:?}", e);
                    }
                    
                    // Primary provider failed, clear it and try creating a new one
                    state.primary_provider = None;
                }
            }
        }

        // Create a new primary provider
        let rpc_url = Url::parse(&state.config.rpc_url)
            .wrap_err_with(|| format!("Invalid RPC URL: {}", state.config.rpc_url))?;

        debug!("Connecting to primary provider for chain {} at {}", state.config.chain_id, rpc_url);
        let primary_provider = match create_provider(rpc_url).await {
            Ok(provider) => provider,
            Err(e) => {
                let reason = format!("Failed to create primary provider: {:?}", e);
                warn!("{} for chain {}, trying fallback", reason, state.config.chain_id);
                
                // Record the failure in the database
                if let Err(db_err) = db.add_to_node_status(
                    state.config.chain_id,
                    &state.config.rpc_url,
                    &state.config.fallback_rpc_url,
                    &reason
                ).await {
                    error!("Failed to record node status: {:?}", db_err);
                }
                
                return Self::create_fallback_provider(&state.config, db).await;
            }
        };

        // Check if the primary provider is fresh enough
        let block = match primary_provider.get_block_by_number(BlockNumberOrTag::Latest).await {
            Ok(Some(block)) => block,
            Ok(None) => {
                let reason = format!("Primary provider returned no latest block");
                warn!("{} for chain {}, trying fallback", reason, state.config.chain_id);
                
                // Record the failure in the database
                if let Err(e) = db.add_to_node_status(
                    state.config.chain_id,
                    &state.config.rpc_url,
                    &state.config.fallback_rpc_url,
                    &reason
                ).await {
                    error!("Failed to record node status: {:?}", e);
                }
                
                return Self::create_fallback_provider(&state.config, db).await;
            }
            Err(e) => {
                let reason = format!("Primary provider failed to get latest block: {:?}", e);
                warn!("{} for chain {}, trying fallback", reason, state.config.chain_id);
                
                // Record the failure in the database
                if let Err(db_err) = db.add_to_node_status(
                    state.config.chain_id,
                    &state.config.rpc_url,
                    &state.config.fallback_rpc_url,
                    &reason
                ).await {
                    error!("Failed to record node status: {:?}", db_err);
                }
                
                return Self::create_fallback_provider(&state.config, db).await;
            }
        };

        // Check if the block is fresh enough
        let block_timestamp: u64 = block.header.inner.timestamp.into();
        let current_time = chrono::Utc::now().timestamp() as u64;
        let max_delay = state.config.max_block_delay_secs;

        if current_time > block_timestamp && (current_time - block_timestamp) > max_delay {
            let reason = format!(
                "Primary provider's latest block is too old: block_time={}, current_time={}, diff={}s, max_delay={}s",
                block_timestamp, current_time, current_time - block_timestamp, max_delay
            );
            
            warn!(
                "{} for chain {}, trying fallback",
                reason, state.config.chain_id
            );
            
            // Record the failure in the database
            if let Err(e) = db.add_to_node_status(
                state.config.chain_id,
                &state.config.rpc_url,
                &state.config.fallback_rpc_url,
                &reason
            ).await {
                error!("Failed to record node status: {:?}", e);
            }
            
            return Self::create_fallback_provider(&state.config, db).await;
        }

        // Store the new primary provider
        debug!("Using new primary provider for chain {}", state.config.chain_id);
        state.primary_provider = Some(primary_provider.clone());
        Ok((primary_provider, false))
    }

    // Helper method to create fallback provider
    async fn create_fallback_provider(config: &ChainConfig, _db: &Database) -> Result<(ProviderType, bool)> {
        let fallback_url = Url::parse(&config.fallback_rpc_url)
            .wrap_err_with(|| format!("Invalid fallback RPC URL: {}", config.fallback_rpc_url))?;

        info!("Connecting to fallback provider for chain {} at {}", config.chain_id, fallback_url);
        let fallback_provider = create_provider(fallback_url).await
            .wrap_err("Failed to connect to fallback provider")?;

        info!("Using fallback provider for chain {}", config.chain_id);
        Ok((fallback_provider, true))
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
) -> Result<ProviderType> {
    // Use a dummy private key since we only need view calls
    let dummy_key = "0000000000000000000000000000000000000000000000000000000000000001";
    let signer: PrivateKeySigner = dummy_key.parse().expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_http(rpc_url);

    Ok(provider)
} 