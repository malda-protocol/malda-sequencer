use alloy::primitives::Address;

use eyre::Result;
use std::time::Duration;
use tracing::{error, info};

pub mod config;
pub mod constants;
pub mod events;
pub mod types;

use crate::config::*;

mod event_listener;


mod proof_generator;
use proof_generator::ProofGenerator;

mod transaction_manager;
use transaction_manager::{TransactionConfig, TransactionManager};

mod batch_event_listener;
use batch_event_listener::{BatchEventConfig, BatchEventListener};

mod lane_manager;
use lane_manager::{LaneManager, LaneManagerConfig};

mod reset_tx_manager;
use reset_tx_manager::{ResetTxManager, ResetTxManagerConfig};

use sequencer::database::{ChainParams, Database};

use std::collections::HashMap;

mod event_proof_ready_checker;
use event_proof_ready_checker::EventProofReadyChecker;

mod gas_fee_distributer;
use gas_fee_distributer::GasFeeDistributer;

pub const UNIX_SOCKET_PATH: &str = "/tmp/sequencer.sock";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");
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

    // Print configuration summary to stdout for deploy.sh
    println!("\n================ Sequencer Configuration Summary ================");
    println!("  Environment: {:?}", config.environment);
    println!("  Database: {}", config.database_url);
    println!("  Chains:");
    for chain in config.get_all_chains() {
        println!("    - {} (ID: {})", chain.name, chain.chain_id);
        println!("      Type: {}", if chain.is_l1 { "L1" } else { "L2" });
        println!(
            "      Markets: {} ({})",
            chain.markets.len(),
            chain
                .markets
                .iter()
                .map(|m| format!("{:?}", m))
                .collect::<Vec<_>>()
                .join(", ")
        );
        println!(
            "      Events: {} ({})",
            chain.events.len(),
            chain.events.join(", ")
        );
    }
    println!("================================================================\n");

    info!("🚀 Starting sequencer in {:?} mode", config.environment);

    // Display configured chains and markets
    let chains = config.get_all_chains();
    info!("📋 Configuration Summary:");
    info!("   Environment: {:?}", config.environment);
    info!("   Total chains configured: {}", chains.len());

    for chain in &chains {
        info!("   🔗 Chain: {} (ID: {})", chain.name, chain.chain_id);
        info!("      Type: {}", if chain.is_l1 { "L1" } else { "L2" });
        info!(
            "      Markets: {} ({})",
            chain.markets.len(),
            chain
                .markets
                .iter()
                .map(|m| format!("{:?}", m))
                .collect::<Vec<_>>()
                .join(", ")
        );
        info!(
            "      Events: {} ({})",
            chain.events.len(),
            chain.events.join(", ")
        );
    }

    info!("✅ Sequencer configuration loaded successfully!");

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
            chain_id: chain.chain_id as u64,
            block_delay: chain.block_delay,
            max_block_delay_secs: chain.max_block_delay_secs,
            is_retarded: false,
        };

        let retarded_batch_config = BatchEventConfig {
            primary_ws_url: chain.ws_url.clone(),
            fallback_ws_url: chain.fallback_ws_url.clone(),
            batch_submitter: chain.batch_submitter_address,
            chain_id: chain.chain_id as u64,
            block_delay: chain.retarded_block_delay,
            max_block_delay_secs: chain.max_block_delay_secs,
            is_retarded: true,
        };

        let db_clone = db.clone();
        let config_clone = config.clone();

        let handle = tokio::spawn(async move {
            start_batch_event_listeners(
                batch_config,
                retarded_batch_config,
                db_clone,
                config_clone.restart_config,
            )
            .await
        });

        handles.push(handle);
    }

    // Start simplified event listener for all chains, markets, and events
    let mut all_event_configs = Vec::new();
    
    for chain in config.get_all_chains() {
        info!(
            "Adding chain {} with {} markets and {} events to unified listener",
            chain.name, chain.markets.len(), chain.events.len()
        );

        // Create normal config for this chain
        let normal_config = create_event_config(chain, &config, false);
        all_event_configs.push(normal_config);

        // Create retarded config for this chain
        let retarded_config = create_event_config(chain, &config, true);
        all_event_configs.push(retarded_config);
    }

    info!("Starting unified event listener with {} configs", all_event_configs.len());
    
    let db_clone = db.clone();
    let handle = tokio::spawn(async move {
        event_listener::EventListener::new(all_event_configs, db_clone).await
    });

    handles.push(handle);

    // Start proof generator
    let db_clone_proof_gen = db.clone();
    let max_retries = config.proof_config.max_retries;
    let retry_delay = config.proof_config.retry_delay;
    let batch_limit = config.proof_config.batch_limit;
    let l1_max_block_delay = config
        .get_l1_chains()
        .first()
        .map(|c| c.max_block_delay_secs)
        .unwrap_or(24000000000);
    let l2_max_block_delay = config
        .get_l2_chains()
        .first()
        .map(|c| c.max_block_delay_secs)
        .unwrap_or(12000000000);

    tokio::spawn(async move {
        let mut proof_generator = ProofGenerator::new(
            max_retries,
            retry_delay,
            db_clone_proof_gen,
            batch_limit,
            l1_max_block_delay,
            l2_max_block_delay,
        );

        if let Err(e) = proof_generator.start().await {
            error!("Proof generator failed: {:?}", e);
        }

        // Keep the task alive
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Start transaction manager
    let transaction_config = create_transaction_config(&config);
    let db_clone_tx = db.clone();

    tokio::spawn(async move {
        let mut transaction_manager = TransactionManager::new(transaction_config, db_clone_tx);

        if let Err(e) = transaction_manager.start().await {
            error!("Transaction manager failed: {:?}", e);
        }

        // Keep the task alive
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
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
    config: &SequencerConfig,
    is_retarded: bool,
) -> event_listener::EventConfig {
    let event_config = &config.event_listener_config;

    let block_range_offset_from = if chain.is_l1 {
        if is_retarded {
            event_config.retarded_block_range_offset_l1_from
        } else {
            event_config.block_range_offset_l1_from
        }
    } else {
        if is_retarded {
            event_config.retarded_block_range_offset_l2_from
        } else {
            event_config.block_range_offset_l2_from
        }
    };

    let block_range_offset_to = if chain.is_l1 {
        if is_retarded {
            event_config.retarded_block_range_offset_l1_to
        } else {
            event_config.block_range_offset_l1_to
        }
    } else {
        if is_retarded {
            event_config.retarded_block_range_offset_l2_to
        } else {
            event_config.block_range_offset_l2_to
        }
    };

    event_listener::EventConfig {
        primary_ws_url: chain.ws_url.clone(),
        fallback_ws_url: chain.fallback_ws_url.clone(),
        chain_id: chain.chain_id as u64,
        max_retries: if is_retarded {
            event_config.retarded_max_retries
        } else {
            event_config.max_retries
        },
        retry_delay_secs: if is_retarded {
            event_config.retarded_retry_delay_secs
        } else {
            event_config.retry_delay_secs
        },
        poll_interval_secs: if is_retarded {
            event_config.retarded_poll_interval_secs
        } else {
            event_config.poll_interval_secs
        },
        max_block_delay_secs: chain.max_block_delay_secs,
        block_range_offset_from,
        block_range_offset_to,
        is_retarded,
        // Include all markets and events for this chain
        markets: chain.markets.clone(),
        events: chain.events.clone(),
    }
}

fn create_transaction_config(config: &SequencerConfig) -> TransactionConfig {
    let mut chain_configs = HashMap::new();

    for chain in config.get_all_chains() {
        // Try to get transaction-specific RPC URL, fall back to regular RPC URL
        let transaction_rpc_url = std::env::var(format!(
            "RPC_URL_{}_TRANSACTION",
            chain.name.to_uppercase().replace(" ", "_")
        ))
        .unwrap_or_else(|_| chain.rpc_url.clone());

        info!(
            "Creating transaction config for chain {} ({}): using RPC URL {}",
            chain.chain_id, chain.name, transaction_rpc_url
        );

        chain_configs.insert(
            chain.chain_id,
            transaction_manager::ChainConfig {
                max_retries: chain.transaction_config.max_retries,
                retry_delay: chain.transaction_config.retry_delay,
                rpc_url: transaction_rpc_url,
                submission_delay_seconds: chain.transaction_config.submission_delay_seconds,
                poll_interval: chain.transaction_config.poll_interval,
                max_tx: chain.transaction_config.max_tx,
                tx_timeout: chain.transaction_config.tx_timeout,
                gas_percentage_increase_per_retry: chain
                    .transaction_config
                    .gas_percentage_increase_per_retry,
            },
        );
    }

    info!(
        "Created transaction config for {} chains",
        chain_configs.len()
    );
    TransactionConfig { chain_configs }
}

async fn start_batch_event_listeners(
    normal_config: BatchEventConfig,
    retarded_config: BatchEventConfig,
    db: Database,
    restart_config: RestartConfig,
) -> Result<()> {
    loop {
        let mut normal_listener = BatchEventListener::new(normal_config.clone(), db.clone());
        let mut retarded_listener = BatchEventListener::new(retarded_config.clone(), db.clone());

        info!("Starting new batch event listener instance");

        if let Err(e) = normal_listener.start().await {
            error!("Batch event listener failed: {:?}", e);
        }

        if let Err(e) = retarded_listener.start().await {
            error!("Retarded batch event listener failed: {:?}", e);
        }

        tokio::time::sleep(Duration::from_secs(restart_config.listener_delay_seconds)).await;
    }
}



async fn start_auxiliary_components(config: &SequencerConfig, db: Database) -> Result<()> {
    // Start lane manager
    let lane_config = LaneManagerConfig {
        poll_interval_secs: 2,
        chain_params: config
            .get_all_chains()
            .iter()
            .map(|c| {
                (
                    c.chain_id,
                    ChainParams {
                        max_volume: c.lane_config.max_volume,
                        time_interval: c.lane_config.time_interval,
                        block_delay: c.lane_config.block_delay,
                        reorg_protection: c.lane_config.reorg_protection,
                    },
                )
            })
            .collect(),
        market_addresses: config
            .get_all_chains()
            .iter()
            .flat_map(|c| c.markets.clone())
            .collect(),
    };

    let db_clone_lane = db.clone();
    tokio::spawn(async move {
        if let Err(e) = LaneManager::new(lane_config, db_clone_lane).await {
            error!("Lane manager failed: {:?}", e);
        }
    });

    // Start event proof ready checker
    let proof_checker_chain_configs: Vec<event_proof_ready_checker::ChainConfig> = config
        .get_all_chains()
        .iter()
        .map(|c| event_proof_ready_checker::ChainConfig {
            chain_id: c.chain_id as u64,
            rpc_url: c.rpc_url.clone(),
            fallback_rpc_url: c.fallback_rpc_url.clone(),
            is_l2: !c.is_l1,
            max_block_delay_secs: c.max_block_delay_secs,
        })
        .collect();

    let db_clone_proof = db.clone();
    let poll_interval = Duration::from_secs(
        config
            .event_proof_ready_checker_config
            .poll_interval_seconds,
    );
    let block_update_interval = Duration::from_secs(
        config
            .event_proof_ready_checker_config
            .block_update_interval_seconds,
    );

    tokio::spawn(async move {
        let mut proof_checker = EventProofReadyChecker::new(
            db_clone_proof,
            poll_interval,
            block_update_interval,
            proof_checker_chain_configs,
        );

        if let Err(e) = proof_checker.start().await {
            error!("Event proof ready checker failed: {:?}", e);
        }

        // Keep the task alive
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    // Start reset tx manager
    let reset_tx_manager_config = ResetTxManagerConfig {
        sample_size: config.reset_tx_manager_config.sample_size,
        multiplier: config.reset_tx_manager_config.multiplier,
        max_retries: config.reset_tx_manager_config.max_retries,
        retry_delay_secs: config.reset_tx_manager_config.retry_delay_secs,
        poll_interval_secs: config.reset_tx_manager_config.poll_interval_secs,
        batch_limit: config.reset_tx_manager_config.batch_limit,
        max_retries_reset: config.reset_tx_manager_config.max_retries_reset,
        api_key: config.reset_tx_manager_config.api_key.clone(),
        rebalancer_url: config.reset_tx_manager_config.rebalancer_url.clone(),
        rebalance_delay: config.reset_tx_manager_config.rebalance_delay,
        minimum_usd_value: config.reset_tx_manager_config.minimum_usd_value,
    };

    let db_clone_reset = db.clone();
    tokio::spawn(async move {
        let mut current_manager = None;
        loop {
            let mut new_manager =
                ResetTxManager::new(reset_tx_manager_config.clone(), db_clone_reset.clone());
            info!("Starting new reset tx manager instance");

            if let Err(e) = new_manager.start().await {
                error!("Reset tx manager failed: {:?}", e);
            }

            tokio::time::sleep(Duration::from_secs(1)).await;

            if let Some(manager) = current_manager.take() {
                drop(manager);
            }

            current_manager = Some(new_manager);
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
    });

    // Start gas fee distributer
    let chains: Vec<u64> = config
        .get_all_chains()
        .iter()
        .map(|c| c.chain_id as u64)
        .collect();
    let markets_per_chain: HashMap<u64, Vec<Address>> = config
        .get_all_chains()
        .iter()
        .map(|c| (c.chain_id as u64, c.markets.clone()))
        .collect();
    let rpc_urls: HashMap<u64, String> = config
        .get_all_chains()
        .iter()
        .map(|c| (c.chain_id as u64, c.rpc_url.clone()))
        .collect();

    let gas_fee_distributer_config = config.gas_fee_distributer_config.clone();
    let gas_fee_distributer_private_key = config.gas_fee_distributer_private_key.clone();
    let gas_fee_distributer_address = config.gas_fee_distributer_address;
    let sequencer_address = config.sequencer_address;

    tokio::spawn(async move {
        let mut gas_fee_distributer = GasFeeDistributer::new(
            chains,
            markets_per_chain,
            rpc_urls,
            gas_fee_distributer_config.poll_interval_secs,
            gas_fee_distributer_private_key,
            gas_fee_distributer_address,
            sequencer_address,
            gas_fee_distributer_config
                .minimum_sequencer_balance_per_chain
                .clone(),
            gas_fee_distributer_config
                .min_distributor_balance_per_chain
                .clone(),
            gas_fee_distributer_config
                .target_sequencer_balance_per_chain
                .clone(),
            gas_fee_distributer_config
                .minimum_harvest_balance_per_chain
                .clone(),
            gas_fee_distributer_config.bridge_fee_percentage,
            gas_fee_distributer_config.min_amount_to_bridge,
        );

        gas_fee_distributer.start_polling_balances().await;

        // Keep the task alive (in case start_polling_balances returns)
        loop {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });

    Ok(())
}
