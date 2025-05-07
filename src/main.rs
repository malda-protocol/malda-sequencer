use alloy::{
    network::EthereumWallet,
    providers::{
        fillers::{BlobGasFiller, ChainIdFiller, GasFiller, JoinFill, NonceFiller},
        Identity, ProviderBuilder, RootProvider,
    },
    signers::local::PrivateKeySigner,
    transports::http::reqwest::Url,
};

use eyre::Result;
use malda_rs::constants::*;
use std::time::Duration;
use tracing::{error, info, warn};

pub mod constants;
pub mod events;
pub mod types;

use crate::{constants::*, events::*};

mod event_listener;
use event_listener::{EventConfig, EventListener};

mod proof_generator;
use proof_generator::{ProofGenerator};

mod transaction_manager;
use transaction_manager::{TransactionConfig, TransactionManager};

mod batch_event_listener;
use batch_event_listener::{BatchEventConfig, BatchEventListener};

mod lane_manager;
use lane_manager::{LaneManager, LaneManagerConfig};

mod reset_tx_manager;
use reset_tx_manager::{ResetTxManager, ResetTxManagerConfig};

use alloy::primitives::TxHash;
use sequencer::database::{Database, EventStatus, EventUpdate, ChainParams};

use std::collections::HashMap;
use std::env;

mod event_proof_ready_checker;
use event_proof_ready_checker::EventProofReadyChecker;
use crate::transaction_manager::ChainConfig;

pub const UNIX_SOCKET_PATH: &str = "/tmp/sequencer.sock";

type ProviderType = alloy::providers::fillers::FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        alloy::providers::fillers::WalletFiller<EthereumWallet>,
    >,
    RootProvider<alloy::transports::http::Http<alloy::transports::http::Client>>,
    alloy::transports::http::Http<alloy::transports::http::Client>,
    alloy::network::Ethereum,
>;

type RpcUrls = HashMap<u32, String>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber for console output
    // Log level can be controlled via RUST_LOG environment variable:
    // - RUST_LOG=debug for debug logs
    // - RUST_LOG=info for info logs
    // - RUST_LOG=warn for warning logs
    // - RUST_LOG=error for error logs
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .init();

    // Load environment variables
    dotenv::dotenv().ok();

    
    // Initialize database
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://doadmin:AVNS_G5U-F8YEsMY2G4odL39@db-postgresql-lon1-66182-do-user-15988403-0.k.db.ondigitalocean.com:25060/defaultdb?sslmode=require".to_string());
    info!(
        "Using database URL: {}",
        database_url.replace("postgres://", "postgres://*****:*****@")
    );
    let db = Database::new(&database_url).await?;

    // Markets
    let markets = vec![
        USDC_MOCK_MARKET_SEPOLIA,
        WSTETH_MOCK_MARKET_SEPOLIA,
    ];
    info!("Configured markets: {:?}", markets);

    // Chain configurations
    let chain_configs = vec![
        (
            WS_URL_LINEA_SEPOLIA,
            LINEA_SEPOLIA_CHAIN_ID,
            vec![
                HOST_BORROW_ON_EXTENSION_CHAIN_SIG,
                HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG,
            ],
        ),
        (
            WS_URL_OPT_SEPOLIA,
            OPTIMISM_SEPOLIA_CHAIN_ID,
            vec![EXTENSION_SUPPLIED_SIG],
        ),
        (
            WS_URL_ETH_SEPOLIA,
            ETHEREUM_SEPOLIA_CHAIN_ID,
            vec![EXTENSION_SUPPLIED_SIG],
        ),
    ];
    info!(
        "Configured chains: {:?}",
        chain_configs
            .iter()
            .map(|(_, id, _)| id)
            .collect::<Vec<_>>()
    );

    // After initializing channels and before starting the main pipeline components
    info!("Initializing batch event listeners...");

    // Batch submitter configurations for each chain
    let batch_configs = vec![
        (WS_URL_LINEA_SEPOLIA, LINEA_SEPOLIA_CHAIN_ID),
        (WS_URL_OPT_SEPOLIA, OPTIMISM_SEPOLIA_CHAIN_ID),
        (WS_URL_ETH_SEPOLIA, ETHEREUM_SEPOLIA_CHAIN_ID),
    ];

    // Spawn batch event listeners
    let mut handles = vec![];

    for (ws_url, chain_id) in batch_configs {
        info!(
            "Starting batch event listener for chain={}, submitter={:?}",
            chain_id, BATCH_SUBMITTER
        );

        let block_delay = if ws_url == WS_URL_LINEA_SEPOLIA || ws_url == WS_URL_ETH_SEPOLIA {
            2
        } else {
            5
        };

        let config = BatchEventConfig {
            ws_url: ws_url.to_string(),
            batch_submitter: BATCH_SUBMITTER,
            chain_id,
            block_delay,  // Process events with 2 block delay
        };

        let db = db.clone();
        let handle = tokio::spawn(async move {
            let mut current_listener = None;
            loop {
                let mut new_listener = BatchEventListener::new(config.clone(), db.clone());
                info!("Starting new batch event listener instance");
                
                if let Err(e) = new_listener.start().await {
                    error!("Batch event listener failed: {:?}", e);
                }
                
                // Reduce delay to 1 second
                tokio::time::sleep(Duration::from_secs(1)).await;
                
                if let Some(listener) = current_listener.take() {
                    drop(listener);
                }
                
                current_listener = Some(new_listener);
                tokio::time::sleep(Duration::from_secs(600)).await;
            }
        });

        handles.push(handle);
        tokio::time::sleep(LISTENER_SPAWN_DELAY).await;
    }

    info!("All batch event listeners started");

    // Spawn event listeners
    let mut handles = vec![];

    for market in &markets {
        for (ws_url, chain_id, events) in chain_configs.iter() {
            for event in events {
                info!(
                    "Starting listener for market={:?}, chain={}, event={}",
                    market, chain_id, event
                );

                let config = EventConfig {
                    ws_url: ws_url.to_string(),
                    market: *market,
                    event_signature: event.to_string(),
                    chain_id: *chain_id,
                    max_retries: 10,
                    retry_delay_secs: 1,
                    poll_interval_secs: 2,
                };

                let db = db.clone();
                let handle = tokio::spawn(async move {
                    let mut current_listener = None;
                    loop {
                        let mut new_listener = EventListener::new(config.clone(), db.clone());
                        info!("Starting new event listener instance");
                        
                        if let Err(e) = new_listener.start().await {
                            error!("Event listener failed: {:?}", e);
                        }
                        
                        // Reduce delay to 1 second
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        
                        if let Some(listener) = current_listener.take() {
                            drop(listener);
                        }
                        
                        current_listener = Some(new_listener);
                        tokio::time::sleep(Duration::from_secs(600)).await;
                    }
                });

                handles.push(handle);
                tokio::time::sleep(LISTENER_SPAWN_DELAY).await;
            }
        }
    }

    info!("All event listeners started");

    let batch_limit = 200;
    // Create proof generator
    let mut proof_generator = ProofGenerator::new(
        MAX_PROOF_RETRIES,
        PROOF_RETRY_DELAY,
        db.clone(),
        batch_limit,
    );

    // Create transaction manager configuration
    let mut chain_configs = HashMap::new();
    
    // Configure each chain with its specific settings
    chain_configs.insert(
        ETHEREUM_SEPOLIA_CHAIN_ID as u32,
        ChainConfig {
            max_retries: 3,
            retry_delay: Duration::from_secs(2),
            rpc_url: rpc_url_ethereum_sepolia().to_string(),
            submission_delay_seconds: 10,
            poll_interval: Duration::from_secs(5),
            max_tx: 50,
            tx_timeout: Duration::from_secs(15),
        },
    );

    chain_configs.insert(
        OPTIMISM_SEPOLIA_CHAIN_ID as u32,
        ChainConfig {
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            rpc_url: "https://sepolia.optimism.io".to_string(),
            submission_delay_seconds: 2,
            poll_interval: Duration::from_secs(2),
            max_tx: 50,
            tx_timeout: Duration::from_secs(10),
        },
    );

    chain_configs.insert(
        LINEA_SEPOLIA_CHAIN_ID as u32,
        ChainConfig {
            max_retries: 3,
            retry_delay: Duration::from_secs(1),
            rpc_url: rpc_url_linea_sepolia().to_string(),
            submission_delay_seconds: 2,
            poll_interval: Duration::from_secs(2),
            max_tx: 50,
            tx_timeout: Duration::from_secs(10),
        },
    );

    let transaction_config = TransactionConfig {
        chain_configs: chain_configs.clone(),
    };

    // Create transaction manager
    let mut transaction_manager = TransactionManager::new(transaction_config, db.clone());

    // Create the event proof ready checker
    let event_proof_ready_checker = EventProofReadyChecker::new(
        db.clone(),
        Duration::from_secs(2), // Check every 10 seconds
        Duration::from_secs(2), // Update block numbers every 1 second
    );

    // Initialize LaneManager
    let lane_manager_config = LaneManagerConfig {
        max_retries: 5,
        retry_delay_secs: 1,
        poll_interval_secs: 2,
        chain_params: {
            let mut map = HashMap::new();
            map.insert(
                LINEA_SEPOLIA_CHAIN_ID as u32,
                ChainParams {
                    max_volume: 1000000000,
                    time_interval: Duration::from_secs(10),
                    block_delay: 10,
                    reorg_protection: REORG_PROTECTION_DEPTH_LINEA_SEPOLIA,
                }
            );
            map.insert(
                ETHEREUM_SEPOLIA_CHAIN_ID as u32,
                ChainParams {
                    max_volume: 1000000000,
                    time_interval: Duration::from_secs(10),
                    block_delay: 2,
                    reorg_protection: REORG_PROTECTION_DEPTH_ETHEREUM_SEPOLIA,
                }
            );
            map.insert(
                OPTIMISM_SEPOLIA_CHAIN_ID as u32,
                ChainParams {
                    max_volume: 1000000000,
                    time_interval: Duration::from_secs(10),
                    block_delay: 10,
                    reorg_protection: REORG_PROTECTION_DEPTH_OPTIMISM_SEPOLIA,
                }
            );
            map
        },
        market_addresses: markets.clone(),
    };

    let lane_manager = LaneManager::new(lane_manager_config, db.clone());
    let lane_manager_handle = tokio::spawn(async move {
        if let Err(e) = lane_manager.start().await {
            error!("Lane manager error: {:?}", e);
        }
    });

    let reset_tx_manager_config = ResetTxManagerConfig {
        sample_size: 10,
        multiplier: 5.0,
        max_retries: 5,
        retry_delay_secs: 1,
        poll_interval_secs: 30,
        batch_limit: 500,
    };
    
    let db_clone_reset = db.clone(); // Clone db for reset tx manager
    let reset_tx_manager_handle = tokio::spawn(async move {
        let mut current_manager = None;
        loop {
            let mut new_manager = ResetTxManager::new(reset_tx_manager_config.clone(), db_clone_reset.clone());
            info!("Starting new reset tx manager instance");
            
            if let Err(e) = new_manager.start().await {
                error!("Reset tx manager failed: {:?}", e);
            }
            
            // Reduce delay to 1 second
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            if let Some(manager) = current_manager.take() {
                drop(manager);
            }
            
            current_manager = Some(new_manager);
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    });

    // Spawn processors
    let proof_generator_handle = tokio::spawn(async move {
        if let Err(e) = proof_generator.start().await {
            error!("Proof generator failed: {:?}", e);
        }
    });
    handles.push(proof_generator_handle);

    let tx_manager_handle = tokio::spawn(async move {
        if let Err(e) = transaction_manager.start().await {
            error!("Transaction manager failed: {:?}", e);
        }
    });
    handles.push(tx_manager_handle);

    // Spawn the event proof ready checker loop
    let db_clone_checker = db.clone(); // Clone db for the checker loop task
    let event_proof_ready_checker_handle = tokio::spawn(async move {
        let mut current_checker = None;
        loop {
            let mut new_checker = EventProofReadyChecker::new(
                db_clone_checker.clone(),
                Duration::from_secs(2),
                Duration::from_secs(2),
            );
            info!("Starting new event proof ready checker instance");
            
            if let Err(e) = new_checker.start().await {
                error!("Event proof ready checker failed: {:?}", e);
            }
            
            // Reduce delay to 1 second
            tokio::time::sleep(Duration::from_secs(1)).await;
            
            if let Some(checker) = current_checker.take() {
                drop(checker);
            }
            
            current_checker = Some(new_checker);
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    });

    // Wait for all tasks to complete
    tokio::try_join!(
        event_proof_ready_checker_handle,
        lane_manager_handle,
        reset_tx_manager_handle,
    )?;

    warn!("Sequencer shutting down");
    Ok(())
}

async fn create_provider(
    rpc_url: Url,
    private_key: &str,
) -> Result<ProviderType, Box<dyn std::error::Error>> {
    let signer: PrivateKeySigner = private_key.parse().expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url);

    Ok(provider)
}
