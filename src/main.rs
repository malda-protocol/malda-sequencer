use alloy::{
    primitives::{Address, address, U256},
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

mod gas_fee_distributer;
use gas_fee_distributer::GasFeeDistributer;

use std::str::FromStr;

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

    // Parse listener restart intervals from .env
    let restart_listener_delay_seconds: u64 = std::env::var("RESTART_LISTENER_DELAY_SECONDS").expect("RESTART_LISTENER_DELAY_SECONDS must be set in .env").parse().expect("RESTART_LISTENER_DELAY_SECONDS must be a valid u64");
    let restart_listener_interval_seconds: u64 = std::env::var("RESTART_LISTENER_INTERVAL_SECONDS").expect("RESTART_LISTENER_INTERVAL_SECONDS must be set in .env").parse().expect("RESTART_LISTENER_INTERVAL_SECONDS must be a valid u64");

    // Parse WebSocket URLs from .env
    let ws_url_linea = std::env::var("WS_URL_LINEA").expect("WS_URL_LINEA must be set in .env");
    let ws_url_opt = std::env::var("WS_URL_OPT").expect("WS_URL_OPT must be set in .env");
    let ws_url_eth = std::env::var("WS_URL_ETH").expect("WS_URL_ETH must be set in .env");
    let ws_url_base = std::env::var("WS_URL_BASE").expect("WS_URL_BASE must be set in .env");
    let ws_url_linea_backup = std::env::var("WS_URL_LINEA_BACKUP").expect("WS_URL_LINEA_BACKUP must be set in .env");
    let ws_url_opt_backup = std::env::var("WS_URL_OPT_BACKUP").expect("WS_URL_OPT_BACKUP must be set in .env");
    let ws_url_eth_backup = std::env::var("WS_URL_ETH_BACKUP").expect("WS_URL_ETH_BACKUP must be set in .env");
    let ws_url_base_backup = std::env::var("WS_URL_BASE_BACKUP").expect("WS_URL_BASE_BACKUP must be set in .env");

    let batch_submitter_address: Address = Address::from_str(&std::env::var("BATCH_SUBMITTER_ADDRESS").expect("BATCH_SUBMITTER_ADDRESS must be set in .env")).expect("Invalid BATCH_SUBMITTER_ADDRESS");
    let sequencer_address: Address = Address::from_str(&std::env::var("SEQUENCER_ADDRESS").expect("SEQUENCER_ADDRESS must be set in .env")).expect("Invalid SEQUENCER_ADDRESS");
    
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
            &ws_url_linea,
            &ws_url_linea_backup,
            LINEA_CHAIN_ID,
            vec![
                HOST_BORROW_ON_EXTENSION_CHAIN_SIG,
                HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG,
            ],
        ),
        (
            &ws_url_base,
            &ws_url_base_backup,
            BASE_CHAIN_ID,
            vec![EXTENSION_SUPPLIED_SIG],
        ),
        (
            &ws_url_eth,
            &ws_url_eth_backup,
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
        (&ws_url_linea, &ws_url_linea_backup, LINEA_CHAIN_ID),
        (&ws_url_base, &ws_url_base_backup, BASE_CHAIN_ID),
        (&ws_url_eth, &ws_url_eth_backup, ETHEREUM_CHAIN_ID),
    ];

    // Spawn batch event listeners
    let mut handles = vec![];

    for (ws_url, ws_url_backup, chain_id) in batch_configs {
        info!(
            "Starting batch event listener for chain={}, submitter={:?}",
            chain_id, batch_submitter_address
        );

        let block_delay_batch_event_listener = if ws_url == &ws_url_eth {
            std::env::var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1").expect("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1 must be set in .env").parse::<u64>().unwrap()
        } else {
            std::env::var("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2").expect("BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2 must be set in .env").parse::<u64>().unwrap()
        };

        let block_delay_retarded_batch_event_listener = if ws_url == &ws_url_eth {
            std::env::var("RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1").expect("RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L1 must be set in .env").parse::<u64>().unwrap()
        } else {
            std::env::var("RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2").expect("RETARDED_BATCH_EVENT_LISTENER_REGISTRATION_BLOCK_DELAY_L2 must be set in .env").parse::<u64>().unwrap()
        };

        let max_block_delay_secs = if chain_id == ETHEREUM_CHAIN_ID {
            std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1 must be set in .env").parse::<u64>().unwrap()
        } else {
            std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2 must be set in .env").parse::<u64>().unwrap()
        };

        let config = BatchEventConfig {
            primary_ws_url: ws_url.to_string(),
            fallback_ws_url: ws_url_backup.to_string(),
            batch_submitter: batch_submitter_address,
            chain_id,
            block_delay: block_delay_batch_event_listener,  // Process events with 2 block delay
            max_block_delay_secs,
            is_retarded: false,
        };

        let config_retarded = BatchEventConfig {
            primary_ws_url: ws_url.to_string(),
            fallback_ws_url: ws_url_backup.to_string(),
            batch_submitter: batch_submitter_address,
            chain_id,
            block_delay: block_delay_retarded_batch_event_listener,  // Process events with 2 block delay
            max_block_delay_secs,
            is_retarded: true,
        };

        let db = db.clone();
        let handle = tokio::spawn(async move {
            let mut current_listener = None;
            let mut current_listener_retarded = None;
            loop {
                let mut new_listener = BatchEventListener::new(config.clone(), db.clone());
                let mut new_listener_retarded = BatchEventListener::new(config_retarded.clone(), db.clone());
                info!("Starting new batch event listener instance");
                
                if let Err(e) = new_listener.start().await {
                    error!("Batch event listener failed: {:?}", e);
                }

                if let Err(e) = new_listener_retarded.start().await {
                    error!("Retarded Batch event listener failed: {:?}", e);
                }
                
                // Reduce delay to 1 second
                tokio::time::sleep(Duration::from_secs(restart_listener_delay_seconds)).await;
                
                if let Some(listener) = current_listener.take() {
                    drop(listener);
                }

                if let Some(listener) = current_listener_retarded.take() {
                    drop(listener);
                }
                
                current_listener = Some(new_listener);
                current_listener_retarded = Some(new_listener_retarded);
                tokio::time::sleep(Duration::from_secs(restart_listener_interval_seconds)).await;
            }
        });

        handles.push(handle);
    }

    info!("All batch event listeners started");

    let max_block_delay_secs =
        std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1 must be set in .env").parse::<u64>().unwrap();


    // Initialize chain configurations for EventProofReadyChecker
    let proof_checker_chain_configs = vec![
        event_proof_ready_checker::ChainConfig {
            chain_id: ETHEREUM_CHAIN_ID,
            rpc_url: rpc_url_ethereum().to_string(),
            fallback_rpc_url: rpc_url_ethereum_fallback().to_string(),
            is_l2: false,
            max_block_delay_secs,
        },
        event_proof_ready_checker::ChainConfig {
            chain_id: BASE_CHAIN_ID,
            rpc_url: rpc_url_base().to_string(),
            fallback_rpc_url: rpc_url_base_fallback().to_string(),
            is_l2: true,
            max_block_delay_secs,
        },
        event_proof_ready_checker::ChainConfig {
            chain_id: OPTIMISM_CHAIN_ID,
            rpc_url: rpc_url_optimism().to_string(),
            fallback_rpc_url: rpc_url_optimism_fallback().to_string(),
            is_l2: true,
            max_block_delay_secs,
        },
        event_proof_ready_checker::ChainConfig {
            chain_id: LINEA_CHAIN_ID,
            rpc_url: rpc_url_linea().to_string(),
            fallback_rpc_url: rpc_url_linea_fallback().to_string(),
            is_l2: true,
            max_block_delay_secs,
        },
    ];

    let proof_ready_checker_pool_interval_seconds = std::env::var("PROOF_READY_CHECKER_POOL_INTERVAL_SECONDS").expect("PROOF_READY_CHECKER_POOL_INTERVAL_SECONDS must be set in .env").parse::<u64>().unwrap();
    let proof_ready_checker_block_update_interval_seconds = std::env::var("PROOF_READY_CHECKER_BLOCK_UPDATE_INTERVAL_SECONDS").expect("PROOF_READY_CHECKER_BLOCK_UPDATE_INTERVAL_SECONDS must be set in .env").parse::<u64>().unwrap();
    // Initialize and start EventProofReadyChecker
    let mut proof_checker = EventProofReadyChecker::new(
        db.clone(),
        Duration::from_secs(proof_ready_checker_pool_interval_seconds),  // poll_interval
        Duration::from_secs(proof_ready_checker_block_update_interval_seconds),  // block_update_interval
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
                    std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1 must be set in .env").parse::<u64>().unwrap()
                } else {
                    std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2 must be set in .env").parse::<u64>().unwrap()
                };

                let block_range_offset_from = if *chain_id == ETHEREUM_CHAIN_ID {
                    std::env::var("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM").expect("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM must be set in .env").parse::<u64>().unwrap()
                } else {
                    std::env::var("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM").expect("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM must be set in .env").parse::<u64>().unwrap()
                };

                let block_range_offset_to = if *chain_id == ETHEREUM_CHAIN_ID {
                    std::env::var("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO").expect("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO must be set in .env").parse::<u64>().unwrap()
                } else {
                    std::env::var("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO").expect("EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO must be set in .env").parse::<u64>().unwrap()
                };

                let poll_interval = std::env::var("EVENT_LISTENER_POLL_INTERVAL_SECS").expect("EVENT_LISTENER_POLL_INTERVAL_SECS must be set in .env").parse::<u64>().unwrap();
                let max_retries: u32 = 10;
                let retry_delay_secs = std::env::var("EVENT_LISTENER_RETRY_DELAY_SECS").expect("EVENT_LISTENER_RETRY_DELAY_SECS must be set in .env").parse::<u64>().unwrap();
                
                let config = EventConfig {
                    primary_ws_url: ws_url.to_string(),
                    fallback_ws_url: ws_url_backup.to_string(),
                    market: *market,
                    event_signature: event.to_string(),
                    chain_id: *chain_id,
                    max_retries,
                    retry_delay_secs,
                    poll_interval_secs: poll_interval,
                    max_block_delay_secs,
                    block_range_offset_from,
                    block_range_offset_to,
                    is_retarded: false,
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
                        tokio::time::sleep(Duration::from_secs(restart_listener_delay_seconds)).await;
                        
                        if let Some(listener) = current_listener.take() {
                            drop(listener);
                        }
                        
                        current_listener = Some(new_listener);
                        tokio::time::sleep(Duration::from_secs(restart_listener_interval_seconds)).await;
                    }
                });

                handles.push(handle);
            }
        }
    }

    for market in &markets {
        for (ws_url, ws_url_backup, chain_id, events) in chain_configs.iter() {
            for event in events {
                info!(
                    "Starting listener for market={:?}, chain={}, event={}",
                    market, chain_id, event
                );

                let max_block_delay_secs = if *chain_id == ETHEREUM_CHAIN_ID {
                    std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1 must be set in .env").parse::<u64>().unwrap()
                } else {
                    std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2 must be set in .env").parse::<u64>().unwrap()
                };

                let block_range_offset_from = if *chain_id == ETHEREUM_CHAIN_ID {
                    std::env::var("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM").expect("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_FROM must be set in .env").parse::<u64>().unwrap()
                } else {
                    std::env::var("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM").expect("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_FROM must be set in .env").parse::<u64>().unwrap()
                };

                let block_range_offset_to = if *chain_id == ETHEREUM_CHAIN_ID {
                    std::env::var("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO").expect("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L1_TO must be set in .env").parse::<u64>().unwrap()
                } else {
                    std::env::var("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO").expect("RETARDED_EVENT_LISTENER_BLOCK_RANGE_OFFSET_L2_TO must be set in .env").parse::<u64>().unwrap()
                };

                let poll_interval = std::env::var("RETARDED_EVENT_LISTENER_POLL_INTERVAL_SECS").expect("RETARDED_EVENT_LISTENER_POLL_INTERVAL_SECS must be set in .env").parse::<u64>().unwrap();
                let max_retries = std::env::var("RETARDED_EVENT_LISTENER_MAX_RETRIES").expect("RETARDED_EVENT_LISTENER_MAX_RETRIES must be set in .env").parse::<u64>().unwrap();
                let retry_delay_secs = std::env::var("RETARDED_EVENT_LISTENER_RETRY_DELAY_SECS").expect("RETARDED_EVENT_LISTENER_RETRY_DELAY_SECS must be set in .env").parse::<u64>().unwrap();
                
                let config = EventConfig {
                    primary_ws_url: ws_url.to_string(),
                    fallback_ws_url: ws_url_backup.to_string(),
                    market: *market,
                    event_signature: event.to_string(),
                    chain_id: *chain_id,
                    max_retries: max_retries.try_into().unwrap(),
                    retry_delay_secs,
                    poll_interval_secs: poll_interval,
                    max_block_delay_secs,
                    block_range_offset_from,
                    block_range_offset_to,
                    is_retarded: true,
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
                        tokio::time::sleep(Duration::from_secs(restart_listener_delay_seconds)).await;
                        
                        if let Some(listener) = current_listener.take() {
                            drop(listener);
                        }
                        
                        current_listener = Some(new_listener);
                        tokio::time::sleep(Duration::from_secs(restart_listener_interval_seconds)).await;
                    }
                });

                handles.push(handle);
            }
        }
    }

    info!("All event listeners started");

    let batch_limit = std::env::var("PROOF_GENERATOR_BATCH_LIMIT").expect("PROOF_GENERATOR_BATCH_LIMIT must be set in .env").parse::<u64>().unwrap();
    let ethereum_max_block_delay_secs = std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L1 must be set in .env").parse::<u64>().unwrap();
    let l2_max_block_delay_secs = std::env::var("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2").expect("NODE_MAX_SECONDS_DELAY_BEFORE_FALLBACK_L2 must be set in .env").parse::<u64>().unwrap();
    let max_proof_retries = std::env::var("PROOF_GENERATOR_MAX_PROOF_RETRIES").expect("PROOF_GENERATOR_MAX_PROOF_RETRIES must be set in .env").parse::<u64>().unwrap();
    let proof_retry_delay = std::env::var("PROOF_GENERATOR_PROOF_RETRY_DELAY").expect("PROOF_GENERATOR_PROOF_RETRY_DELAY must be set in .env").parse::<u64>().unwrap();
    // Create proof generator
    let mut proof_generator = ProofGenerator::new(
        max_proof_retries.try_into().unwrap(),
        Duration::from_secs(proof_retry_delay),
        db.clone(),
        batch_limit.try_into().unwrap(),
        ethereum_max_block_delay_secs,
        l2_max_block_delay_secs,
    );

    // Create transaction manager configuration
    let mut chain_configs = HashMap::new();

    let ethereum_max_retries = std::env::var("TRANSACTION_MANAGER_MAX_RETRIES_L1").expect("TRANSACTION_MANAGER_MAX_RETRIES_L1 must be set in .env").parse::<u64>().unwrap();
    let ethereum_retry_delay = std::env::var("TRANSACTION_MANAGER_RETRY_DELAY_L1").expect("TRANSACTION_MANAGER_RETRY_DELAY_L1 must be set in .env").parse::<u64>().unwrap();
    let ethereum_submission_delay_seconds = std::env::var("TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L1").expect("TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L1 must be set in .env").parse::<u64>().unwrap();
    let ethereum_poll_interval = std::env::var("TRANSACTION_MANAGER_POLL_INTERVAL_L1").expect("TRANSACTION_MANAGER_POLL_INTERVAL_L1 must be set in .env").parse::<u64>().unwrap();
    let ethereum_max_tx = std::env::var("TRANSACTION_MANAGER_MAX_TX_L1").expect("TRANSACTION_MANAGER_MAX_TX_L1 must be set in .env").parse::<u64>().unwrap();
    let ethereum_tx_timeout = std::env::var("TRANSACTION_MANAGER_TX_TIMEOUT_L1").expect("TRANSACTION_MANAGER_TX_TIMEOUT_L1 must be set in .env").parse::<u64>().unwrap();

    let l2_max_retries = std::env::var("TRANSACTION_MANAGER_MAX_RETRIES_L2").expect("TRANSACTION_MANAGER_MAX_RETRIES_L2 must be set in .env").parse::<u64>().unwrap();
    let l2_retry_delay = std::env::var("TRANSACTION_MANAGER_RETRY_DELAY_L2").expect("TRANSACTION_MANAGER_RETRY_DELAY_L2 must be set in .env").parse::<u64>().unwrap();
    let l2_submission_delay_seconds = std::env::var("TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L2").expect("TRANSACTION_MANAGER_SUBMISSION_DELAY_SECONDS_L2 must be set in .env").parse::<u64>().unwrap();
    let l2_poll_interval = std::env::var("TRANSACTION_MANAGER_POLL_INTERVAL_L2").expect("TRANSACTION_MANAGER_POLL_INTERVAL_L2 must be set in .env").parse::<u64>().unwrap();
    let l2_max_tx = std::env::var("TRANSACTION_MANAGER_MAX_TX_L2").expect("TRANSACTION_MANAGER_MAX_TX_L2 must be set in .env").parse::<u64>().unwrap();
    let l2_tx_timeout = std::env::var("TRANSACTION_MANAGER_TX_TIMEOUT_L2").expect("TRANSACTION_MANAGER_TX_TIMEOUT_L2 must be set in .env").parse::<u64>().unwrap();
    let gas_percentage_increase_per_retry = std::env::var("TRANSACTION_MANAGER_GAS_PERCENTAGE_INCREASE_PER_RETRY").expect("TRANSACTION_MANAGER_GAS_PERCENTAGE_INCREASE_PER_RETRY must be set in .env").parse::<u64>().unwrap();

    let rpc_url_linea_transaction = std::env::var("RPC_URL_LINEA_TRANSACTION").expect("RPC_URL_LINEA_TRANSACTION must be set in .env");
    // Configure each chain with its specific settings
    chain_configs.insert(
        ETHEREUM_CHAIN_ID as u32,
        ChainConfig {
            max_retries: ethereum_max_retries as u128,
            retry_delay: Duration::from_secs(ethereum_retry_delay),
            rpc_url: rpc_url_ethereum_fallback().to_string(),
            submission_delay_seconds: ethereum_submission_delay_seconds,
            poll_interval: Duration::from_secs(ethereum_poll_interval),
            max_tx: ethereum_max_tx as i64,
            tx_timeout: Duration::from_secs(ethereum_tx_timeout),
            gas_percentage_increase_per_retry: gas_percentage_increase_per_retry.try_into().unwrap(),
        },
    );

    chain_configs.insert(
        BASE_CHAIN_ID as u32,
        ChainConfig {
            max_retries: l2_max_retries as u128,
            retry_delay: Duration::from_secs(l2_retry_delay),
            rpc_url: rpc_url_base_fallback().to_string(),
            submission_delay_seconds: l2_submission_delay_seconds,
            poll_interval: Duration::from_secs(l2_poll_interval),
            max_tx: l2_max_tx as i64,
            tx_timeout: Duration::from_secs(l2_tx_timeout),
            gas_percentage_increase_per_retry: gas_percentage_increase_per_retry.try_into().unwrap(),
        },
    );

    chain_configs.insert(
        LINEA_CHAIN_ID as u32,
        ChainConfig {
            max_retries: l2_max_retries as u128,
            retry_delay: Duration::from_secs(l2_retry_delay),
            rpc_url: rpc_url_linea_transaction.to_string(),
            submission_delay_seconds: l2_submission_delay_seconds,
            poll_interval: Duration::from_secs(l2_poll_interval),
            max_tx: l2_max_tx as i64,
            tx_timeout: Duration::from_secs(l2_tx_timeout),
            gas_percentage_increase_per_retry: gas_percentage_increase_per_retry.try_into().unwrap(),
        },
    );

    let transaction_config = TransactionConfig {
        chain_configs: chain_configs.clone(),
    };

    // Create transaction manager
    let mut transaction_manager = TransactionManager::new(transaction_config, db.clone());

    let max_retries: u32 = std::env::var("LANE_MANAGER_MAX_RETRIES").expect("LANE_MANAGER_MAX_RETRIES must be set in .env").parse::<u64>().unwrap().try_into().unwrap();
    let retry_delay_secs = std::env::var("LANE_MANAGER_RETRY_DELAY_SECS").expect("LANE_MANAGER_RETRY_DELAY_SECS must be set in .env").parse::<u64>().unwrap();
    let poll_interval_secs = std::env::var("LANE_MANAGER_POLL_INTERVAL_SECS").expect("LANE_MANAGER_POLL_INTERVAL_SECS must be set in .env").parse::<u64>().unwrap();
    let max_volume_linea: i32 = std::env::var("LANE_MANAGER_MAX_VOLUME_LINEA").expect("LANE_MANAGER_MAX_VOLUME_LINEA must be set in .env").parse::<u64>().unwrap().try_into().unwrap();
    let max_volume_ethereum: i32 = std::env::var("LANE_MANAGER_MAX_VOLUME_ETHEREUM").expect("LANE_MANAGER_MAX_VOLUME_ETHEREUM must be set in .env").parse::<u64>().unwrap().try_into().unwrap();
    let max_volume_base: i32 = std::env::var("LANE_MANAGER_MAX_VOLUME_BASE").expect("LANE_MANAGER_MAX_VOLUME_BASE must be set in .env").parse::<u64>().unwrap().try_into().unwrap();
    let time_interval_linea = std::env::var("LANE_MANAGER_INTERVAL_SECS_LINEA").expect("LANE_MANAGER_INTERVAL_SECS_LINEA must be set in .env").parse::<u64>().unwrap();
    let time_interval_ethereum = std::env::var("LANE_MANAGER_INTERVAL_SECS_ETHEREUM").expect("LANE_MANAGER_INTERVAL_SECS_ETHEREUM must be set in .env").parse::<u64>().unwrap();
    let time_interval_base = std::env::var("LANE_MANAGER_INTERVAL_SECS_BASE").expect("LANE_MANAGER_INTERVAL_SECS_BASE must be set in .env").parse::<u64>().unwrap();
    let block_delay_linea = std::env::var("LANE_MANAGER_BLOCK_DELAY_LINEA").expect("LANE_MANAGER_BLOCK_DELAY_LINEA must be set in .env").parse::<u64>().unwrap();
    let block_delay_ethereum = std::env::var("LANE_MANAGER_BLOCK_DELAY_ETHEREUM").expect("LANE_MANAGER_BLOCK_DELAY_ETHEREUM must be set in .env").parse::<u64>().unwrap();
    let block_delay_base = std::env::var("LANE_MANAGER_BLOCK_DELAY_BASE").expect("LANE_MANAGER_BLOCK_DELAY_BASE must be set in .env").parse::<u64>().unwrap();
    // Initialize LaneManager
    let lane_manager_config = LaneManagerConfig {
        max_retries: max_retries,
        retry_delay_secs: retry_delay_secs,
        poll_interval_secs: poll_interval_secs,
        chain_params: {
            let mut map = HashMap::new();
            map.insert(
                LINEA_CHAIN_ID as u32,
                ChainParams {
                    max_volume: max_volume_linea,
                    time_interval: Duration::from_secs(time_interval_linea),
                    block_delay: block_delay_linea,
                    reorg_protection: REORG_PROTECTION_DEPTH_LINEA,
                }
            );
            map.insert(
                ETHEREUM_CHAIN_ID as u32,
                ChainParams {
                    max_volume: max_volume_ethereum,
                    time_interval: Duration::from_secs(time_interval_ethereum),
                    block_delay: block_delay_ethereum,
                    reorg_protection: REORG_PROTECTION_DEPTH_ETHEREUM,
                }
            );
            map.insert(
                BASE_CHAIN_ID as u32,
                ChainParams {
                    max_volume: max_volume_base,
                    time_interval: Duration::from_secs(time_interval_base),
                    block_delay: block_delay_base,
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

    let sample_size: i64 = std::env::var("RESET_TX_MANAGER_SAMPLE_SIZE").expect("RESET_TX_MANAGER_SAMPLE_SIZE must be set in .env").parse::<u64>().unwrap().try_into().unwrap();
    let multiplier = std::env::var("RESET_TX_MANAGER_MULTIPLIER").expect("RESET_TX_MANAGER_MULTIPLIER must be set in .env").parse::<f64>().unwrap();
    let max_retries_reset: u32 = std::env::var("RESET_TX_MANAGER_MAX_RETRIES").expect("RESET_TX_MANAGER_MAX_RETRIES must be set in .env").parse::<u64>().unwrap().try_into().unwrap();
    let retry_delay_secs = std::env::var("RESET_TX_MANAGER_RETRY_DELAY_SECS").expect("RESET_TX_MANAGER_RETRY_DELAY_SECS must be set in .env").parse::<u64>().unwrap();
    let poll_interval_secs = std::env::var("RESET_TX_MANAGER_POLL_INTERVAL_SECS").expect("RESET_TX_MANAGER_POLL_INTERVAL_SECS must be set in .env").parse::<u64>().unwrap();
    let batch_limit: i64 = std::env::var("RESET_TX_MANAGER_BATCH_LIMIT").expect("RESET_TX_MANAGER_BATCH_LIMIT must be set in .env").parse::<u64>().unwrap().try_into().unwrap();
    let api_key = std::env::var("REBALANCER_API_KEY").expect("REBALANCER_API_KEY must be set in .env");
    let rebalancer_url = std::env::var("REBALANCER_URL").expect("REBALANCER_URL must be set in .env");
    let rebalance_delay = std::env::var("REBALANCER_DELAY_SECONDS").expect("REBALANCER_DELAY_SECONDS must be set in .env").parse::<u64>().unwrap();
    let minimum_usd_value = std::env::var("REBALANCER_MINIMUM_USD_VALUE").expect("REBALANCER_MINIMUM_USD_VALUE must be set in .env").parse::<f64>().unwrap();
    let reset_tx_manager_config = ResetTxManagerConfig {
        sample_size: sample_size,
        multiplier: multiplier,
        max_retries: max_retries,
        retry_delay_secs: retry_delay_secs,
        poll_interval_secs: poll_interval_secs,
        batch_limit: batch_limit,
        max_retries_reset: max_retries_reset as i64,
        api_key,
        rebalancer_url,
        rebalance_delay,
        minimum_usd_value,
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

    // Initialize GasFeeDistributer
    let chains = vec![LINEA_CHAIN_ID as u64, ETHEREUM_CHAIN_ID as u64, BASE_CHAIN_ID as u64];
    let mut markets_per_chain = HashMap::new();
    for &chain_id in &chains {
        markets_per_chain.insert(chain_id, markets.clone());
    }
    let mut rpc_urls = HashMap::new();
    rpc_urls.insert(LINEA_CHAIN_ID as u64, rpc_url_linea_fallback().to_string());
    rpc_urls.insert(ETHEREUM_CHAIN_ID as u64, rpc_url_ethereum_fallback().to_string());
    rpc_urls.insert(BASE_CHAIN_ID as u64, rpc_url_base_fallback().to_string());
    let poll_interval_secs = 60; // Set the polling interval for ETH balance updates
    let private_key = std::env::var("GAS_FEE_DISTRIBUTER_PRIVATE_KEY").expect("GAS_FEE_DISTRIBUTER_PRIVATE_KEY must be set in .env");
    let public_address = Address::from_str(&std::env::var("GAS_FEE_DISTRIBUTER_ADDRESS").expect("GAS_FEE_DISTRIBUTER_ADDRESS must be set in .env")).expect("Invalid GAS_FEE_DISTRIBUTER_ADDRESS");
    let min_seq_eth = U256::from_str(&std::env::var("MIN_SEQUENCER_BALANCE_ETHEREUM").expect("MIN_SEQUENCER_BALANCE_ETHEREUM must be set in .env")).unwrap();
    let min_seq_linea = U256::from_str(&std::env::var("MIN_SEQUENCER_BALANCE_LINEA").expect("MIN_SEQUENCER_BALANCE_LINEA must be set in .env")).unwrap();
    let min_seq_base = U256::from_str(&std::env::var("MIN_SEQUENCER_BALANCE_BASE").expect("MIN_SEQUENCER_BALANCE_BASE must be set in .env")).unwrap();
    let mut minimum_sequencer_balance_per_chain = std::collections::HashMap::new();
    minimum_sequencer_balance_per_chain.insert(ETHEREUM_CHAIN_ID as u64, min_seq_eth);
    minimum_sequencer_balance_per_chain.insert(LINEA_CHAIN_ID as u64, min_seq_linea);
    minimum_sequencer_balance_per_chain.insert(BASE_CHAIN_ID as u64, min_seq_base);
    let mut min_distributor_balance_per_chain = std::collections::HashMap::new();
    for (&chain_id, &min_seq) in &minimum_sequencer_balance_per_chain {
        let key = match chain_id {
            x if x == ETHEREUM_CHAIN_ID as u64 => "MIN_DISTRIBUTOR_BALANCE_ETHEREUM",
            x if x == LINEA_CHAIN_ID as u64 => "MIN_DISTRIBUTOR_BALANCE_LINEA",
            x if x == BASE_CHAIN_ID as u64 => "MIN_DISTRIBUTOR_BALANCE_BASE",
            _ => panic!("Unknown chain_id for distributor balance")
        };
        let min_dist = U256::from_str(&std::env::var(key).expect(&format!("{} must be set in .env", key))).unwrap();
        min_distributor_balance_per_chain.insert(chain_id, min_dist);
    }
    let mut target_sequencer_balance_per_chain = std::collections::HashMap::new();
    for (&chain_id, &min_seq) in &minimum_sequencer_balance_per_chain {
        let key = match chain_id {
            x if x == ETHEREUM_CHAIN_ID as u64 => "TARGET_SEQUENCER_BALANCE_ETHEREUM",
            x if x == LINEA_CHAIN_ID as u64 => "TARGET_SEQUENCER_BALANCE_LINEA",
            x if x == BASE_CHAIN_ID as u64 => "TARGET_SEQUENCER_BALANCE_BASE",
            _ => panic!("Unknown chain_id for target sequencer balance")
        };
        let target_seq = U256::from_str(&std::env::var(key).expect(&format!("{} must be set in .env", key))).unwrap();
        target_sequencer_balance_per_chain.insert(chain_id, target_seq);
    }
    let mut minimum_harvest_balance_per_chain = std::collections::HashMap::new();
    for &chain_id in &[ETHEREUM_CHAIN_ID as u64, LINEA_CHAIN_ID as u64, BASE_CHAIN_ID as u64] {
        let key = match chain_id {
            x if x == ETHEREUM_CHAIN_ID as u64 => "MIN_HARVEST_BALANCE_ETHEREUM",
            x if x == LINEA_CHAIN_ID as u64 => "MIN_HARVEST_BALANCE_LINEA",
            x if x == BASE_CHAIN_ID as u64 => "MIN_HARVEST_BALANCE_BASE",
            _ => panic!("Unknown chain_id for min harvest balance")
        };
        let min_harvest = U256::from_str(&std::env::var(key).expect(&format!("{} must be set in .env", key))).unwrap();
        minimum_harvest_balance_per_chain.insert(chain_id, min_harvest);
    }
    let bridge_fee_percentage: u32 = std::env::var("BRIDGE_FEE_PERCENTAGE").expect("BRIDGE_FEE_PERCENTAGE must be set in .env").parse().unwrap();
    let min_amount_to_bridge = U256::from_str(&std::env::var("MIN_AMOUNT_TO_BRIDGE").expect("MIN_AMOUNT_TO_BRIDGE must be set in .env")).unwrap();
    let mut gas_fee_distributer = GasFeeDistributer::new(
        chains,
        markets_per_chain,
        rpc_urls,
        poll_interval_secs,
        private_key,
        public_address,
        sequencer_address,
        minimum_sequencer_balance_per_chain,
        min_distributor_balance_per_chain,
        target_sequencer_balance_per_chain,
        minimum_harvest_balance_per_chain,
        bridge_fee_percentage,
        min_amount_to_bridge,
    );
    let gas_fee_distributer_handle = tokio::spawn(async move {
        gas_fee_distributer.start_polling_balances().await;
    });

    // Wait for all tasks to complete
    tokio::try_join!(
        lane_manager_handle,
        reset_tx_manager_handle,
        // gas_fee_distributer_handle,
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
