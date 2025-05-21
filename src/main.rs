use alloy::{
    primitives::{Address, address},
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
use event_proof_ready_checker::{EventProofReadyChecker};
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

pub const mUSDC_market: Address = address!("269C36A173D881720544Fb303E681370158FF1FD");
pub const mWETH_market: Address = address!("C7Bc6bD45Eb84D594f51cED3c5497E6812C7732f");
pub const mUSDT_market: Address = address!("DF0635c1eCfdF08146150691a97e2Ff6a8Aa1a90");
pub const mWBTC_market: Address = address!("cb4d153604a6F21Ff7625e5044E89C3b903599Bc");
pub const mwstETH_market: Address = address!("1D8e8cEFEb085f3211Ab6a443Ad9051b54D1cd1a");
pub const mezETH_market: Address = address!("0B3c6645F4F2442AD4bbee2e2273A250461cA6f8");
pub const mweETH_market: Address = address!("8BaD0c523516262a439197736fFf982F5E0987cC");
pub const mwrsETH_market: Address = address!("4DF3DD62DB219C47F6a7CB1bE02C511AFceAdf5E");

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
        .unwrap();
    info!(
        "Using database URL: {}",
        database_url.replace("postgres://", "postgres://*****:*****@")
    );
    let db = Database::new(&database_url).await?;

    db.sequencer_start_events_reset().await?;

    // Markets
    let markets = vec![
        mUSDC_market,
        mWETH_market,
        mUSDT_market,
        mWBTC_market,
        mwstETH_market,
        mezETH_market,
        mweETH_market,
        mwrsETH_market,
    ];
    info!("Configured markets: {:?}", markets);

    // Chain configurations
    let chain_configs = vec![
        (
            WS_URL_LINEA,
            WS_URL_LINEA_BACKUP,
            LINEA_CHAIN_ID,
            vec![
                HOST_BORROW_ON_EXTENSION_CHAIN_SIG,
                HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG,
            ],
        ),
        (
            WS_URL_BASE,
            WS_URL_BASE_BACKUP,
            BASE_CHAIN_ID,
            vec![EXTENSION_SUPPLIED_SIG],
        ),
        (
            WS_URL_ETH,
            WS_URL_ETH_BACKUP,
            ETHEREUM_CHAIN_ID,
            vec![EXTENSION_SUPPLIED_SIG],
        ),
    ];
    info!(
        "Configured chains: {:?}",
        chain_configs
            .iter()
            .map(|(_, _, id, _)| id)
            .collect::<Vec<_>>()
    );

    // After initializing channels and before starting the main pipeline components
    info!("Initializing batch event listeners...");

    // Batch submitter configurations for each chain
    let batch_configs = vec![
        (WS_URL_LINEA, WS_URL_LINEA_BACKUP, LINEA_CHAIN_ID),
        (WS_URL_BASE, WS_URL_BASE_BACKUP, BASE_CHAIN_ID),
        (WS_URL_ETH, WS_URL_ETH_BACKUP, ETHEREUM_CHAIN_ID),
    ];

    // Spawn batch event listeners
    let mut handles = vec![];

    for (ws_url, ws_url_backup, chain_id) in batch_configs {
        info!(
            "Starting batch event listener for chain={}, submitter={:?}",
            chain_id, BATCH_SUBMITTER
        );

        let block_delay = if ws_url == WS_URL_LINEA || ws_url == WS_URL_ETH {
            2
        } else {
            5
        };

        let max_block_delay_secs = if chain_id == ETHEREUM_CHAIN_ID || chain_id == BASE_CHAIN_ID {
            24
        } else {
            10
        };

        let config = BatchEventConfig {
            primary_ws_url: ws_url.to_string(),
            fallback_ws_url: ws_url_backup.to_string(),
            batch_submitter: BATCH_SUBMITTER,
            chain_id,
            block_delay,  // Process events with 2 block delay
            max_block_delay_secs,
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

    // Initialize chain configurations for EventProofReadyChecker
    let proof_checker_chain_configs = vec![
        event_proof_ready_checker::ChainConfig {
            chain_id: ETHEREUM_CHAIN_ID,
            rpc_url: rpc_url_ethereum().to_string(),
            fallback_rpc_url: rpc_url_ethereum_fallback().to_string(),
            is_l2: false,
            max_block_delay_secs: 24,
        },
        event_proof_ready_checker::ChainConfig {
            chain_id: BASE_CHAIN_ID,
            rpc_url: rpc_url_base().to_string(),
            fallback_rpc_url: rpc_url_base_fallback().to_string(),
            is_l2: true,
            max_block_delay_secs: 6,
        },
        event_proof_ready_checker::ChainConfig {
            chain_id: OPTIMISM_CHAIN_ID,
            rpc_url: rpc_url_optimism().to_string(),
            fallback_rpc_url: rpc_url_optimism_fallback().to_string(),
            is_l2: true,
            max_block_delay_secs: 6,
        },
        event_proof_ready_checker::ChainConfig {
            chain_id: LINEA_CHAIN_ID,
            rpc_url: rpc_url_linea().to_string(),
            fallback_rpc_url: rpc_url_linea_fallback().to_string(),
            is_l2: true,
            max_block_delay_secs: 10,
        },
    ];

    // Initialize and start EventProofReadyChecker
    let mut proof_checker = EventProofReadyChecker::new(
        db.clone(),
        Duration::from_secs(1),  // poll_interval
        Duration::from_secs(2),  // block_update_interval
        proof_checker_chain_configs,
    );

    if let Err(e) = proof_checker.start().await {
        error!("Failed to start event proof ready checker: {:?}", e);
    }

    // Spawn event listeners
    let mut handles = vec![];

    for market in &markets {
        for (ws_url, ws_url_backup, chain_id, events) in chain_configs.iter() {
            for event in events {
                info!(
                    "Starting listener for market={:?}, chain={}, event={}",
                    market, chain_id, event
                );

                let max_block_delay_secs = if *chain_id == ETHEREUM_CHAIN_ID {
                    24
                } else {
                    10
                };

                let config = EventConfig {
                    primary_ws_url: ws_url.to_string(),
                    fallback_ws_url: ws_url_backup.to_string(),
                    market: *market,
                    event_signature: event.to_string(),
                    chain_id: *chain_id,
                    max_retries: 3,
                    retry_delay_secs: 5,
                    poll_interval_secs: 1,
                    max_block_delay_secs,
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
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        
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
        24, // ethereum_max_block_delay_secs - 24 seconds for Ethereum chains
        10,  // l2_max_block_delay_secs - 6 seconds for L2 chains
    );

    // Create transaction manager configuration
    let mut chain_configs = HashMap::new();
    
    // Configure each chain with its specific settings
    chain_configs.insert(
        ETHEREUM_CHAIN_ID as u32,
        ChainConfig {
            max_retries: 3,
            retry_delay: Duration::from_secs(2),
            rpc_url: rpc_url_ethereum_fallback().to_string(),
            submission_delay_seconds: 1,
            poll_interval: Duration::from_secs(5),
            max_tx: 50,
            tx_timeout: Duration::from_secs(15),
        },
    );

    chain_configs.insert(
        BASE_CHAIN_ID as u32,
        ChainConfig {
            max_retries: 10,
            retry_delay: Duration::from_secs(1),
            rpc_url: rpc_url_base_fallback().to_string(),
            submission_delay_seconds: 1,
            poll_interval: Duration::from_secs(2),
            max_tx: 50,
            tx_timeout: Duration::from_secs(4),
        },
    );

    chain_configs.insert(
        LINEA_CHAIN_ID as u32,
        ChainConfig {
            max_retries: 10,
            retry_delay: Duration::from_secs(1),
            rpc_url: rpc_url_linea_fallback().to_string(),
            submission_delay_seconds: 1,
            poll_interval: Duration::from_secs(2),
            max_tx: 50,
            tx_timeout: Duration::from_secs(4),
        },
    );

    let transaction_config = TransactionConfig {
        chain_configs: chain_configs.clone(),
    };

    // Create transaction manager
    let mut transaction_manager = TransactionManager::new(transaction_config, db.clone());

    // Initialize LaneManager
    let lane_manager_config = LaneManagerConfig {
        max_retries: 5,
        retry_delay_secs: 1,
        poll_interval_secs: 2,
        chain_params: {
            let mut map = HashMap::new();
            map.insert(
                LINEA_CHAIN_ID as u32,
                ChainParams {
                    max_volume: 1000000000,
                    time_interval: Duration::from_secs(10),
                    block_delay: 10,
                    reorg_protection: REORG_PROTECTION_DEPTH_LINEA,
                }
            );
            map.insert(
                ETHEREUM_CHAIN_ID as u32,
                ChainParams {
                    max_volume: 1000000000,
                    time_interval: Duration::from_secs(10),
                    block_delay: 2,
                    reorg_protection: REORG_PROTECTION_DEPTH_ETHEREUM,
                }
            );
            map.insert(
                BASE_CHAIN_ID as u32,
                ChainParams {
                    max_volume: 1000000000,
                    time_interval: Duration::from_secs(10),
                    block_delay: 10,
                    reorg_protection: REORG_PROTECTION_DEPTH_BASE,
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

    // Wait for all tasks to complete
    tokio::try_join!(
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
