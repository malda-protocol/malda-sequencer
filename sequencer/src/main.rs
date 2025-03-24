use alloy::{
    providers::{
        fillers::{
            BlobGasFiller, ChainIdFiller, GasFiller, JoinFill, NonceFiller,
        },
        ProviderBuilder, Identity, RootProvider,
    },
    transports::http::reqwest::Url,
    network::EthereumWallet,
    signers::local::PrivateKeySigner,
};

use std::time::Duration;
use eyre::Result;
use tracing::{info, error, warn};
use malda_rs::constants::*;

pub mod events;
pub mod types;
pub mod constants;

use crate::{
    events::*,
    constants::*,
};

mod event_listener;
use event_listener::{EventListener, EventConfig, RawEvent};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

mod event_processor;
use event_processor::EventProcessor;
use event_processor::ProcessedEvent;

mod proof_generator;
use proof_generator::{ProofGenerator, ProofReadyEvent};

mod transaction_manager;
use transaction_manager::{TransactionManager, TransactionConfig};

mod batch_event_listener;
use batch_event_listener::{BatchEventListener, BatchEventConfig};

use tokio::net::UnixListener;
use tokio::io::AsyncReadExt;
use std::fs;

use dotenv::dotenv;
use sequencer::database::{Database, EventStatus, EventUpdate};
use alloy::primitives::TxHash;

use std::env;
use std::collections::HashMap;

pub const TX_TIMEOUT: Duration = Duration::from_secs(30);

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
    // Set required environment variables if not already set
    if env::var("BONSAI_API_KEY").is_err() {
        env::set_var("BONSAI_API_KEY", "SrSzB6P4SFaWv7WAK12ph5K6aL6dXs4S1a0XMif5");
    }
    if env::var("BONSAI_API_URL").is_err() {
        env::set_var("BONSAI_API_URL", "https://api.bonsai.xyz/");
    }

    // Log environment setup
    info!("BONSAI_API_URL set to: {}", env::var("BONSAI_API_URL").unwrap());
    info!("BONSAI_API_KEY is set"); // Don't log the actual key

    // Load .env file
    dotenv().ok();

    // Create channels
    let (event_sender, event_receiver) = mpsc::channel::<RawEvent>(100);
    let (processed_sender, processed_receiver) = mpsc::channel::<ProcessedEvent>(100);
    let (proof_sender, proof_receiver) = mpsc::channel::<Vec<ProofReadyEvent>>(100);

    // Initialize database
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/sequencer".to_string());
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

    // Create event processor
    let mut event_processor = EventProcessor::new(
        event_receiver, // Clone the receiver
        processed_sender.clone(),
        db.clone()
    );

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
        (WS_URL_LINEA_SEPOLIA, LINEA_SEPOLIA_CHAIN_ID, vec![HOST_BORROW_ON_EXTENSION_CHAIN_SIG, HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG]),
        (WS_URL_OPT_SEPOLIA, OPTIMISM_SEPOLIA_CHAIN_ID, vec![EXTENSION_SUPPLIED_SIG]),
        (WS_URL_ETH_SEPOLIA, ETHEREUM_SEPOLIA_CHAIN_ID, vec![EXTENSION_SUPPLIED_SIG]),
    ];
    info!("Configured chains: {:?}", chain_configs.iter().map(|(_, id, _)| id).collect::<Vec<_>>());

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
        
        let listener = BatchEventListener::new(config);
        let handle = tokio::spawn(async move {
            if let Err(e) = listener.start().await {
                error!("Batch event listener failed: {:?}", e);
            }
        });
        
        handles.push(handle);
        tokio::time::sleep(LISTENER_SPAWN_DELAY).await;
    }

    info!("All batch event listeners started");

    // Spawn event listeners
    let mut handles = vec![];
    
    for market in markets {
        for (ws_url, chain_id, events) in chain_configs.iter() {
            for event in events {
                info!(
                    "Starting listener for market={:?}, chain={}, event={}", 
                    market, chain_id, event
                );
                
                let config = EventConfig {
                    ws_url: ws_url.to_string(),
                    market,
                    event_signature: event.to_string(),
                    chain_id: *chain_id,
                };
                
                let listener = EventListener::new(
                    config,
                    event_sender.clone(),
                );
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
    let proof_generator = ProofGenerator::new(
        processed_receiver,
        proof_sender,
        MAX_PROOF_RETRIES,
        PROOF_RETRY_DELAY,
        db.clone(),
    );

    // Add the config before creating TransactionManager
    let tx_config = TransactionConfig {
        max_retries: 3,
        retry_delay: Duration::from_secs(1),
        rpc_urls: vec![
            (ETHEREUM_SEPOLIA_CHAIN_ID as u32, RPC_URL_ETHEREUM_SEPOLIA.to_string()),
            (OPTIMISM_SEPOLIA_CHAIN_ID as u32, RPC_URL_OPTIMISM_SEPOLIA.to_string()),
            (LINEA_SEPOLIA_CHAIN_ID as u32, RPC_URL_LINEA_SEPOLIA.to_string()),
        ],
    };

    // Create transaction manager
    let mut transaction_manager = TransactionManager::new(
        proof_receiver,
        tx_config,
        db.clone(),
    );

    // Spawn processors
    let processor_handle = tokio::spawn(async move {
        if let Err(e) = event_processor.start().await {
            error!("Event processor failed: {:?}", e);
        }
    });
    handles.push(processor_handle);

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

    info!("All components initialized and running");

    // Set up Unix socket for manual event injection
    let socket_path = "/tmp/sequencer.sock";
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

async fn create_provider(rpc_url: Url, private_key: &str) -> Result<ProviderType, Box<dyn std::error::Error>> {
    let signer: PrivateKeySigner = private_key
        .parse()
        .expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url);

    Ok(provider)
}

