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
use tokio_stream::wrappers::ReceiverStream;
use tracing::{error, info, warn};

pub mod constants;
pub mod events;
pub mod types;

use crate::{constants::*, events::*};

mod event_listener;
use event_listener::{EventConfig, EventListener, RawEvent, ProcessedEvent};
use tokio::sync::mpsc;

mod proof_generator;
use proof_generator::{ProofGenerator};

mod transaction_manager;
use transaction_manager::{TransactionConfig, TransactionManager};

mod batch_event_listener;
use batch_event_listener::{BatchEventConfig, BatchEventListener};

use std::fs;
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;

use alloy::primitives::TxHash;
use dotenv::dotenv;
use sequencer::database::{Database, EventStatus, EventUpdate};

use std::collections::HashMap;
use std::env;

pub const TX_TIMEOUT: Duration = Duration::from_secs(30);
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

    // Initialize channels
    let (processed_sender, processed_receiver) = mpsc::channel::<ProcessedEvent>(100);
    
    // Initialize database
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/sequencer".to_string());
    info!(
        "Using database URL: {}",
        database_url.replace("postgres://", "postgres://*****:*****@")
    );
    let db = Database::new(&database_url).await?;

    // Test database connection
    info!("Testing database connection...");
    let test_update = EventUpdate {
        tx_hash: TxHash::from_slice(&[0; 32]),
        status: EventStatus::Received,
        ..Default::default()
    };

    match db.update_event(test_update).await {
        Ok(_) => info!("Successfully wrote test event to database"),
        Err(e) => error!("Failed to write test event: {:?}", e),
    }

    // Markets
    let markets = vec![
        WETH_MARKET_SEPOLIA,
        USDC_MARKET_SEPOLIA,
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

        let config = BatchEventConfig {
            ws_url: ws_url.to_string(),
            batch_submitter: BATCH_SUBMITTER,
            chain_id,
        };

        let batch_listener = BatchEventListener::new(config, db.clone());
        let handle = tokio::spawn(async move {
            if let Err(e) = batch_listener.start().await {
                error!("Batch event listener failed: {:?}", e);
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
                };

                let listener = EventListener::new(config, processed_sender.clone(), db.clone());
                let handle = tokio::spawn(async move {
                    if let Err(e) = listener.start().await {
                        error!("Event listener failed: {:?}", e);
                    }
                });

                handles.push(handle);
                tokio::time::sleep(LISTENER_SPAWN_DELAY).await;
            }
        }
    }

    info!("All event listeners started");

    // Create proof generator
    let mut proof_generator = ProofGenerator::new(
        MAX_PROOF_RETRIES,
        PROOF_RETRY_DELAY,
        db.clone(),
        BATCH_SIZE,
    );

    // Add the config before creating TransactionManager
    let tx_config = TransactionConfig {
        max_retries: 3,
        retry_delay: Duration::from_secs(1),
        rpc_urls: vec![
            (
                ETHEREUM_SEPOLIA_CHAIN_ID as u32,
                RPC_URL_ETHEREUM_SEPOLIA.to_string(),
            ),
            (
                OPTIMISM_SEPOLIA_CHAIN_ID as u32,
                RPC_URL_OPTIMISM_SEPOLIA.to_string(),
            ),
            (
                LINEA_SEPOLIA_CHAIN_ID as u32,
                RPC_URL_LINEA_SEPOLIA.to_string(),
            ),
        ],
        poll_interval: Duration::from_secs(5), // Check for new events every 5 seconds
    };

    // Create transaction manager
    let mut transaction_manager = TransactionManager::new(tx_config, db.clone());

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

    // Log configuration summary
    info!("----------------- SEQUENCER CONFIGURATION -----------------");
    info!(
        "Database: {}",
        database_url.replace("postgres://", "postgres://*****:*****@")
    );
    info!("Markets: {}", markets.len());
    info!("Chains: {}", chain_configs.len());
    info!("Max proof retries: {}", MAX_PROOF_RETRIES);
    info!("Transaction timeout: {:?}", TX_TIMEOUT);
    info!("Socket path: {}", UNIX_SOCKET_PATH);
    info!("----------------------------------------------------------");
    info!("All components initialized and running");

    // Set up Unix socket for manual event injection
    let socket_path = UNIX_SOCKET_PATH;
    // Remove the socket file if it exists
    let _ = fs::remove_file(socket_path);
    let listener = UnixListener::bind(socket_path)?;

    let processed_sender_clone = processed_sender.clone();
    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                let tx = processed_sender_clone.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    if let Ok(_) = socket.read_to_end(&mut buf).await {
                        if let Ok(event) = serde_json::from_slice::<ProcessedEvent>(&buf) {
                            if let Err(e) = tx.send(event).await {
                                error!("Failed to forward manual event: {}", e);
                            }
                        }
                    }
                });
            }
        }
    });

    // Wait for all tasks to complete
    for handle in handles {
        if let Err(e) = handle.await {
            error!("Task failed: {:?}", e);
        }
    }

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
