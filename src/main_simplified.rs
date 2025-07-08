use alloy::{
    primitives::{Address, U256},
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
pub mod config;

use crate::{constants::*, events::*, config::*};

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

use sequencer::database::{Database, ChainParams};

use std::collections::HashMap;

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
    RootProvider<alloy::network::Ethereum>,
    alloy::network::Ethereum,
>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing subscriber for console output
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .init();

    // Load environment variables
    dotenv::dotenv().ok();

    // Load configuration - this is the only configuration needed!
    let config = SequencerConfig::new()?;
    
    info!("Starting sequencer in {:?} mode", config.environment);
    info!("Configured chains: {:?}", 
        config.get_all_chains().iter().map(|c| c.name.clone()).collect::<Vec<_>>());

    // Initialize database
    let db = Database::new(&config.database_url).await?;
    db.sequencer_start_events_reset().await?;

    // Start all components using the centralized configuration
    start_sequencer_components(config, db).await?;

    // Keep the main thread alive
    tokio::signal::ctrl_c().await?;
    info!("Shutting down sequencer...");
    
    Ok(())
}

async fn start_sequencer_components(config: SequencerConfig, db: Database) -> Result<()> {
    let mut handles = vec![];

    // Start batch event listeners for all chains
    for chain in config.get_all_chains() {
        info!("Starting batch event listener for {}", chain.name);
        
        let batch_config = BatchEventConfig {
            primary_ws_url: chain.ws_url.clone(),
            fallback_ws_url: chain.fallback_ws_url.clone(),
            batch_submitter: chain.batch_submitter_address,
            chain_id: chain.chain_id,
            block_delay: chain.block_delay,
            max_block_delay_secs: chain.max_block_delay_secs,
            is_retarded: false,
        };

        let retarded_batch_config = BatchEventConfig {
            primary_ws_url: chain.ws_url.clone(),
            fallback_ws_url: chain.fallback_ws_url.clone(),
            batch_submitter: chain.batch_submitter_address,
            chain_id: chain.chain_id,
            block_delay: chain.retarded_block_delay,
            max_block_delay_secs: chain.max_block_delay_secs,
            is_retarded: true,
        };

        let db_clone = db.clone();
        let config_clone = config.clone();
        
        let handle = tokio::spawn(async move {
            start_batch_event_listeners(batch_config, retarded_batch_config, db_clone, config_clone.restart_config).await
        });
        
        handles.push(handle);
    }

    // Start event listeners for all markets and chains
    for chain in config.get_all_chains() {
        for market in &chain.markets {
            for event in &chain.events {
                info!("Starting event listener for market={:?}, chain={}, event={}", 
                    market, chain.name, event);

                let event_config = create_event_config(chain, market, event, &config, false);
                let retarded_event_config = create_event_config(chain, market, event, &config, true);

                let db_clone = db.clone();
                let config_clone = config.clone();
                
                let handle = tokio::spawn(async move {
                    start_event_listeners(event_config, retarded_event_config, db_clone, config_clone.restart_config).await
                });
                
                handles.push(handle);
            }
        }
    }

    // Start proof generator
    let proof_generator = ProofGenerator::new(
        config.proof_config.max_retries,
        config.proof_config.retry_delay,
        db.clone(),
        config.proof_config.batch_limit,
        config.get_l1_chains().first().map(|c| c.max_block_delay_secs).unwrap_or(24000000000),
        config.get_l2_chains().first().map(|c| c.max_block_delay_secs).unwrap_or(12000000000),
    );

    tokio::spawn(async move {
        if let Err(e) = proof_generator.start().await {
            error!("Proof generator failed: {:?}", e);
        }
    });

    // Start transaction manager
    let transaction_config = create_transaction_config(&config);
    let mut transaction_manager = TransactionManager::new(transaction_config, db.clone());
    
    tokio::spawn(async move {
        if let Err(e) = transaction_manager.start().await {
            error!("Transaction manager failed: {:?}", e);
        }
    });

    // Start other components (lane manager, gas fee distributer, etc.)
    start_auxiliary_components(&config, db).await?;

    // Wait for all handles to complete (they shouldn't unless there's an error)
    futures::future::join_all(handles).await;
    
    Ok(())
}

fn create_event_config(
    chain: &ChainConfig, 
    market: &Address, 
    event: &str, 
    config: &SequencerConfig, 
    is_retarded: bool
) -> EventConfig {
    let event_config = &config.event_listener_config;
    
    let (max_retries, retry_delay_secs, poll_interval_secs, block_range_offset_from, block_range_offset_to) = 
        if is_retarded {
            (
                event_config.retarded_max_retries,
                event_config.retarded_retry_delay_secs,
                event_config.retarded_poll_interval_secs,
                if chain.is_l1 { event_config.retarded_block_range_offset_l1_from } else { event_config.retarded_block_range_offset_l2_from },
                if chain.is_l1 { event_config.retarded_block_range_offset_l1_to } else { event_config.retarded_block_range_offset_l2_to },
            )
        } else {
            (
                event_config.max_retries,
                event_config.retry_delay_secs,
                event_config.poll_interval_secs,
                if chain.is_l1 { event_config.block_range_offset_l1_from } else { event_config.block_range_offset_l2_from },
                if chain.is_l1 { event_config.block_range_offset_l1_to } else { event_config.block_range_offset_l2_to },
            )
        };

    EventConfig {
        primary_ws_url: chain.ws_url.clone(),
        fallback_ws_url: chain.fallback_ws_url.clone(),
        market: *market,
        event_signature: event.to_string(),
        chain_id: chain.chain_id,
        max_retries,
        retry_delay_secs,
        poll_interval_secs: poll_interval_secs,
        max_block_delay_secs: chain.max_block_delay_secs,
        block_range_offset_from,
        block_range_offset_to,
        is_retarded,
    }
}

fn create_transaction_config(config: &SequencerConfig) -> TransactionConfig {
    let mut chain_configs = HashMap::new();
    
    for chain in config.get_all_chains() {
        chain_configs.insert(chain.chain_id, ChainConfig {
            max_retries: chain.transaction_config.max_retries,
            retry_delay: chain.transaction_config.retry_delay,
            rpc_url: chain.rpc_url.clone(),
            submission_delay_seconds: chain.transaction_config.submission_delay_seconds,
            poll_interval: chain.transaction_config.poll_interval,
            max_tx: chain.transaction_config.max_tx,
            tx_timeout: chain.transaction_config.tx_timeout,
            gas_percentage_increase_per_retry: chain.transaction_config.gas_percentage_increase_per_retry,
        });
    }
    
    TransactionConfig { chain_configs }
}

async fn start_batch_event_listeners(
    normal_config: BatchEventConfig,
    retarded_config: BatchEventConfig,
    db: Database,
    restart_config: RestartConfig,
) -> Result<()> {
    let mut current_listener = None;
    let mut current_retarded_listener = None;
    
    loop {
        let mut new_listener = BatchEventListener::new(normal_config.clone(), db.clone());
        let mut new_retarded_listener = BatchEventListener::new(retarded_config.clone(), db.clone());
        
        if let Err(e) = new_listener.start().await {
            error!("Batch event listener failed: {:?}", e);
        }
        
        if let Err(e) = new_retarded_listener.start().await {
            error!("Retarded batch event listener failed: {:?}", e);
        }
        
        tokio::time::sleep(Duration::from_secs(restart_config.listener_delay_seconds)).await;
        
        if let Some(listener) = current_listener.take() {
            drop(listener);
        }
        if let Some(listener) = current_retarded_listener.take() {
            drop(listener);
        }
        
        current_listener = Some(new_listener);
        current_retarded_listener = Some(new_retarded_listener);
        tokio::time::sleep(Duration::from_secs(restart_config.listener_interval_seconds)).await;
    }
}

async fn start_event_listeners(
    normal_config: EventConfig,
    retarded_config: EventConfig,
    db: Database,
    restart_config: RestartConfig,
) -> Result<()> {
    let mut current_listener = None;
    let mut current_retarded_listener = None;
    
    loop {
        let mut new_listener = EventListener::new(normal_config.clone(), db.clone());
        let mut new_retarded_listener = EventListener::new(retarded_config.clone(), db.clone());
        
        if let Err(e) = new_listener.start().await {
            error!("Event listener failed: {:?}", e);
        }
        
        if let Err(e) = new_retarded_listener.start().await {
            error!("Retarded event listener failed: {:?}", e);
        }
        
        tokio::time::sleep(Duration::from_secs(restart_config.listener_delay_seconds)).await;
        
        if let Some(listener) = current_listener.take() {
            drop(listener);
        }
        if let Some(listener) = current_retarded_listener.take() {
            drop(listener);
        }
        
        current_listener = Some(new_listener);
        current_retarded_listener = Some(new_retarded_listener);
        tokio::time::sleep(Duration::from_secs(restart_config.listener_interval_seconds)).await;
    }
}

async fn start_auxiliary_components(config: &SequencerConfig, db: Database) -> Result<()> {
    // Start lane manager
    let lane_config = LaneManagerConfig {
        max_retries: 5,
        retry_delay_secs: 1,
        poll_interval_secs: 2,
        chain_params: config.get_all_chains()
            .iter()
            .map(|c| (c.chain_id as u64, ChainParams {
                max_volume: c.lane_config.max_volume,
                time_interval: c.lane_config.time_interval,
                block_delay: c.lane_config.block_delay,
                reorg_protection: c.lane_config.reorg_protection,
            }))
            .collect(),
    };
    
    let mut lane_manager = LaneManager::new(lane_config, db.clone());
    tokio::spawn(async move {
        if let Err(e) = lane_manager.start().await {
            error!("Lane manager failed: {:?}", e);
        }
    });

    // Start gas fee distributer
    let gas_fee_distributer = GasFeeDistributer::new(
        config.gas_fee_distributer_private_key.clone(),
        config.gas_fee_distributer_address,
        db.clone(),
    );
    
    tokio::spawn(async move {
        if let Err(e) = gas_fee_distributer.start().await {
            error!("Gas fee distributer failed: {:?}", e);
        }
    });

    Ok(())
}

async fn create_provider(
    rpc_url: Url,
    private_key: &str,
) -> Result<ProviderType, Box<dyn std::error::Error>> {
    let signer = PrivateKeySigner::from_str(private_key)?;
    let wallet = EthereumWallet::from(signer);
    
    let provider = ProviderBuilder::new()
        .on_http(rpc_url)
        .await?;
    
    let provider = provider
        .with_signer(wallet)
        .await?;
    
    Ok(provider)
} 