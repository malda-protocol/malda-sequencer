//! # Main Sequencer Module
//!
//! This module serves as the entry point for the Malda sequencer system. It orchestrates
//! the startup and coordination of all sequencer components, including event listeners,
//! proof generators, transaction managers, and auxiliary services.
//!
//! ## Key Features:
//! - **System Initialization**: Sets up logging, crypto providers, and environment
//! - **Configuration Management**: Loads and validates sequencer configuration
//! - **Component Orchestration**: Starts all sequencer components in parallel
//! - **Database Management**: Initializes database connections and resets events
//! - **Graceful Shutdown**: Handles shutdown signals and cleanup
//!
//! ## Architecture:
//! ```
//! Main Function Flow
//! ├── Initialize System: Logging, crypto, environment
//! ├── Load Configuration: Validate and parse config
//! ├── Print Summary: Configuration details for deployment
//! ├── Log Details: Detailed logging for debugging
//! ├── Initialize Database: Connect and reset events
//! ├── Start Components: All sequencer components in parallel
//! └── Wait for Shutdown: Graceful shutdown handling
//! ```
//!
//! ## Component Startup Order:
//! 1. **Batch Event Listener**: Monitors batch processing events
//! 2. **Event Listener**: Monitors blockchain events across all chains
//! 3. **Proof Generator**: Generates ZK proofs for events
//! 4. **Transaction Manager**: Submits transactions to destination chains
//! 5. **Auxiliary Components**: Lane manager, proof checker, reset manager, gas distributor
//!
//! ## Error Handling:
//! - Configuration errors are propagated to main
//! - Database initialization errors are handled gracefully
//! - Component failures are logged but don't stop other components
//! - Shutdown signals are handled cleanly

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
use batch_event_listener::BatchEventListener;

mod lane_manager;
use lane_manager::{LaneManager, LaneManagerConfig};

mod reset_tx_manager;
use reset_tx_manager::{ResetTxManager, ResetTxManagerConfig};

use sequencer::database::{ChainParams, Database};

use std::collections::HashMap;

mod event_proof_ready_checker;
use event_proof_ready_checker::EventProofReadyChecker;

mod gas_fee_distributer;
mod provider_helper;
mod api_module;
use api_module::{ApiServer, ApiConfig};

/// Main entry point for the Malda sequencer system
///
/// This function orchestrates the complete startup sequence for the sequencer,
/// including system initialization, configuration loading, database setup,
/// and starting all sequencer components in parallel.
///
/// ## Workflow:
/// 1. **System Initialization**: Set up logging, crypto providers, and environment
/// 2. **Configuration Loading**: Load and validate sequencer configuration
/// 3. **Configuration Display**: Print summary for deployment scripts
/// 4. **Detailed Logging**: Log configuration details for debugging
/// 5. **Database Setup**: Initialize database connection and reset events
/// 6. **Component Startup**: Start all sequencer components in parallel
/// 7. **Shutdown Handling**: Wait for shutdown signal and cleanup
///
/// ## Error Handling:
/// - Returns `Result<(), Box<dyn std::error::Error>>` for comprehensive error handling
/// - Configuration errors are propagated to caller
/// - Component failures are logged but don't stop the system
///
/// ## Example:
/// ```rust
/// #[tokio::main]
/// async fn main() -> Result<(), Box<dyn std::error::Error>> {
///     // This function handles the complete sequencer startup
///     Ok(())
/// }
/// ```
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Step 1: Initialize system dependencies (logging, crypto, environment)
    initialize_system()?;

    // Step 2: Load and validate sequencer configuration
    let config = load_configuration()?;

    // Step 3: Print configuration summary for deployment scripts
    print_configuration_summary(&config);

    // Step 4: Log detailed configuration information for debugging
    log_configuration_details(&config);

    // Step 5: Initialize database connection and reset events
    let db = initialize_database(&config).await?;

    // Step 6: Start all sequencer components in parallel
    start_all_sequencer_components(config.clone(), db.clone()).await?;

    // Step 6.5: Start API server
    info!("About to start API server...");
    start_api_server(config, db).await;
    info!("API server startup initiated");

    // Step 7: Wait for shutdown signal and cleanup
    wait_for_shutdown().await;

    Ok(())
}

/// Initialize system dependencies and logging infrastructure
///
/// This function sets up the foundational components required for the sequencer
/// to operate, including the crypto provider, logging system, and environment
/// variables.
///
/// ## Initialization Steps:
/// 1. **Crypto Provider**: Install rustls crypto provider for secure connections
/// 2. **Logging System**: Configure tracing subscriber with file/line info
/// 3. **Environment Variables**: Load .env file for configuration
///
/// ## Error Handling:
/// - Crypto provider installation failures are fatal (expect)
/// - Environment loading is optional (dotenv::dotenv().ok())
///
/// # Returns
/// * `Result<()>` - Success or error status
///
/// # Example
/// ```rust
/// initialize_system()?;
/// ```
fn initialize_system() -> Result<()> {
    // Install rustls crypto provider for secure connections
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    // Configure tracing subscriber with detailed logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .init();

    // Load environment variables from .env file (optional)
    dotenv::dotenv().ok();

    Ok(())
}

/// Load and validate sequencer configuration
///
/// This function loads the sequencer configuration from environment variables
/// and validates that all required settings are present and correct.
///
/// ## Configuration Sources:
/// - Environment variables (loaded from .env file)
/// - Chain-specific configurations
/// - Database connection settings
/// - Component-specific parameters
///
/// ## Validation:
/// - Ensures all required environment variables are set
/// - Validates chain configurations
/// - Checks database connection parameters
///
/// # Returns
/// * `Result<SequencerConfig>` - Validated configuration or error
///
/// # Example
/// ```rust
/// let config = load_configuration()?;
/// ```
fn load_configuration() -> Result<SequencerConfig> {
    SequencerConfig::new()
}

/// Print configuration summary to stdout for deployment scripts
///
/// This function outputs a formatted configuration summary to stdout,
/// which is used by deployment scripts to verify the sequencer setup.
/// The output includes environment, database, and chain information.
///
/// ## Output Format:
/// - Environment type (development/production)
/// - Database connection string
/// - Chain configurations with markets and events
/// - Market addresses and event types per chain
///
/// ## Usage:
/// This output is typically captured by deployment scripts to verify
/// that the sequencer is configured correctly before starting.
///
/// # Arguments
/// * `config` - Reference to the sequencer configuration
///
/// # Example
/// ```rust
/// print_configuration_summary(&config);
/// ```
fn print_configuration_summary(config: &SequencerConfig) {
    println!("\n================ Sequencer Configuration Summary ================");
    println!("  Environment: {:?}", config.environment);
    println!("  Database: {}", config.database_url);
    if let Some(ref fallback_url) = config.fallback_database_url {
        println!("  Fallback Database: {}", fallback_url);
    }
    println!("  Chains:");

    // Print detailed information for each configured chain
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
}

/// Log detailed configuration information for debugging
///
/// This function logs comprehensive configuration details using the tracing
/// system, providing detailed information for debugging and monitoring
/// the sequencer startup process.
///
/// ## Logged Information:
/// - Environment type and startup mode
/// - Total number of configured chains
/// - Per-chain details (name, ID, type, markets, events)
/// - Configuration validation status
///
/// ## Log Levels:
/// - Uses `info!` level for configuration details
/// - Provides structured logging for easy parsing
///
/// # Arguments
/// * `config` - Reference to the sequencer configuration
///
/// # Example
/// ```rust
/// log_configuration_details(&config);
/// ```
fn log_configuration_details(config: &SequencerConfig) {
    info!("🚀 Starting sequencer in {:?} mode", config.environment);

    let chains = config.get_all_chains();
    info!("📋 Configuration Summary:");
    info!("   Environment: {:?}", config.environment);
    info!("   Database: {}", config.database_url);
    if let Some(ref fallback_url) = config.fallback_database_url {
        info!("   Fallback Database: {}", fallback_url);
        info!("   Database Failover: Enabled");
    } else {
        info!("   Database Failover: Disabled");
    }
    info!("   Total chains configured: {}", chains.len());

    // Log detailed information for each chain
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
}

/// Initialize database connection and reset events
///
/// This function establishes a connection to the PostgreSQL database and
/// performs initial setup tasks, including resetting events that were
/// in progress when the sequencer was last stopped.
///
/// ## Database Operations:
/// 1. **Connection Setup**: Establishes connection pool with SSL
/// 2. **Migration**: Runs any pending database migrations
/// 3. **Event Reset**: Resets events from 'ProofRequested' to 'ReadyToRequestProof'
///
/// ## Error Handling:
/// - Database connection failures are propagated
/// - Migration failures are handled gracefully
/// - Event reset failures are logged but don't stop startup
///
/// # Arguments
/// * `config` - Reference to the sequencer configuration
///
/// # Returns
/// * `Result<Database>` - Database connection or error
///
/// # Example
/// ```rust
/// let db = initialize_database(&config).await?;
/// ```
async fn initialize_database(config: &SequencerConfig) -> Result<Database> {
    // Create database connection with fallback support if configured
    let db = if let Some(ref fallback_url) = config.fallback_database_url {
        info!("Initializing database with fallback support");
        info!("Primary database: {}", config.database_url);
        info!("Fallback database: {}", fallback_url);
        
        Database::new_with_simple_fallback(&config.database_url, Some(fallback_url.clone())).await?
    } else {
        info!("Initializing database without fallback support");
        info!("Database URL: {}", config.database_url);
        
        Database::new(&config.database_url).await?
    };

    // Reset events that were in progress when sequencer was stopped
    db.sequencer_start_events_reset().await?;

    Ok(db)
}

/// Start all sequencer components in parallel
///
/// This function orchestrates the startup of all sequencer components,
/// starting them in parallel to maximize efficiency. Each component
/// runs independently and failures in one component don't affect others.
///
/// ## Component Startup Order:
/// 1. **Batch Event Listener**: Monitors batch processing events
/// 2. **Event Listener**: Monitors blockchain events across all chains
/// 3. **Proof Generator**: Generates ZK proofs for events
/// 4. **Transaction Manager**: Submits transactions to destination chains
/// 5. **Auxiliary Components**: Lane manager, proof checker, reset manager, gas distributor
///
/// ## Parallel Execution:
/// - All components start simultaneously
/// - Each component runs in its own tokio task
/// - Handles are collected for graceful shutdown
///
/// ## Error Handling:
/// - Component failures are logged but don't stop other components
/// - Database is shared across all components
/// - Configuration is cloned for each component
///
/// # Arguments
/// * `config` - Sequencer configuration (consumed)
/// * `db` - Database connection (consumed)
///
/// # Returns
/// * `Result<()>` - Success or error status
///
/// # Example
/// ```rust
/// start_all_sequencer_components(config, db).await?;
/// ```
async fn start_all_sequencer_components(config: SequencerConfig, db: Database) -> Result<()> {
    let mut handles = vec![];

    // Start all components in parallel
    start_batch_event_listener(&config, &db, &mut handles).await;
    start_event_listener(&config, &db, &mut handles).await;
    start_proof_generator(&config, &db).await;
    start_transaction_manager(&config, &db).await;
    start_auxiliary_components(&config, db).await?;

    // Don't wait for handles to complete - they should run indefinitely
    // The main thread should continue to start the API server
    info!("All sequencer components started successfully");

    Ok(())
}

/// Start batch event listener for all chains
///
/// This function creates and starts the batch event listener component,
/// which monitors batch processing success and failure events across
/// all configured chains in parallel.
///
/// ## Configuration:
/// - Creates both normal and retarded configurations for each chain
/// - Uses WebSocket connections for real-time event monitoring
/// - Configures batch submitter addresses and block delays
///
/// ## Parallel Processing:
/// - Each chain processes events independently
/// - Failures in one chain don't affect others
/// - Uses unified configuration for efficiency
///
/// # Arguments
/// * `config` - Reference to sequencer configuration
/// * `db` - Reference to database connection
/// * `handles` - Mutable reference to task handles vector
///
/// # Example
/// ```rust
/// start_batch_event_listener(&config, &db, &mut handles).await;
/// ```
async fn start_batch_event_listener(
    config: &SequencerConfig,
    db: &Database,
    handles: &mut Vec<tokio::task::JoinHandle<Result<(), eyre::Report>>>,
) {
    info!("Starting unified batch event listener for all chains");

    let mut batch_chain_configs = Vec::new();

    // Create configurations for each chain (normal and retarded)
    for chain in config.get_all_chains() {
        info!(
            "Adding chain {} to unified batch event listener",
            chain.name
        );

        // Create normal batch config for this chain
        let normal_batch_config = batch_event_listener::ChainConfig {
            primary_ws_url: chain.ws_url.clone(),
            fallback_ws_url: chain.fallback_ws_url.clone(),
            batch_submitter: chain.batch_submitter_address,
            chain_id: chain.chain_id as u64,
            block_delay: chain.block_delay,
            max_block_delay_secs: chain.max_block_delay_secs,
            is_retarded: false,
        };
        batch_chain_configs.push(normal_batch_config);

        // Create retarded batch config for this chain
        let retarded_batch_config = batch_event_listener::ChainConfig {
            primary_ws_url: chain.ws_url.clone(),
            fallback_ws_url: chain.fallback_ws_url.clone(),
            batch_submitter: chain.batch_submitter_address,
            chain_id: chain.chain_id as u64,
            block_delay: chain.retarded_block_delay,
            max_block_delay_secs: chain.max_block_delay_secs,
            is_retarded: true,
        };
        batch_chain_configs.push(retarded_batch_config);
    }

    // Create unified batch configuration
    let unified_batch_config = batch_event_listener::BatchEventConfig {
        chain_configs: batch_chain_configs,
    };

    // Spawn batch event listener task
    let db_clone = db.clone();
    let handle =
        tokio::spawn(async move { BatchEventListener::new(unified_batch_config, db_clone).await });

    handles.push(handle);
}

/// Start event listener for all chains, markets, and events
///
/// This function creates and starts the event listener component,
/// which monitors blockchain events across all configured chains,
/// markets, and event types in a unified manner.
///
/// ## Event Monitoring:
/// - Monitors events from all configured markets on all chains
/// - Handles both normal and retarded event processing
/// - Uses unified filtering for efficiency
///
/// ## Configuration:
/// - Creates event configurations for each chain
/// - Includes all markets and events for each chain
/// - Configures polling intervals and retry logic
///
/// # Arguments
/// * `config` - Reference to sequencer configuration
/// * `db` - Reference to database connection
/// * `handles` - Mutable reference to task handles vector
///
/// # Example
/// ```rust
/// start_event_listener(&config, &db, &mut handles).await;
/// ```
async fn start_event_listener(
    config: &SequencerConfig,
    db: &Database,
    handles: &mut Vec<tokio::task::JoinHandle<Result<(), eyre::Report>>>,
) {
    let mut all_event_configs = Vec::new();

    // Create event configurations for each chain
    for chain in config.get_all_chains() {
        info!(
            "Adding chain {} with {} markets and {} events to unified listener",
            chain.name,
            chain.markets.len(),
            chain.events.len()
        );

        // Create normal config for this chain
        let normal_config = create_event_config(chain, config, false);
        all_event_configs.push(normal_config);

        // Create retarded config for this chain
        let retarded_config = create_event_config(chain, config, true);
        all_event_configs.push(retarded_config);
    }

    info!(
        "Starting unified event listener with {} configs",
        all_event_configs.len()
    );

    // Spawn event listener task
    let db_clone = db.clone();
    let handle = tokio::spawn(async move {
        event_listener::EventListener::new(all_event_configs, db_clone).await
    });

    handles.push(handle);
}

/// Start proof generator component
///
/// This function creates and starts the proof generator component,
/// which generates ZK proofs for events that are ready for proof
/// generation. The component handles both L1 and L2 chain events.
///
/// ## Proof Generation:
/// - Monitors events in 'ProofRequested' status
/// - Generates ZK proofs using the RISC0 framework
/// - Handles both boundless and SDK proof types
/// - Updates events with proof data and journal indices
///
/// ## Configuration:
/// - Configures retry logic and batch limits
/// - Sets up L1 and L2 block delay parameters
/// - Handles proof generation timeouts
///
/// # Arguments
/// * `config` - Reference to sequencer configuration
/// * `db` - Reference to database connection
///
/// # Example
/// ```rust
/// start_proof_generator(&config, &db).await;
/// ```
async fn start_proof_generator(config: &SequencerConfig, db: &Database) {
    let db_clone = db.clone();

    // Extract proof generation configuration
    let max_retries = config.proof_config.max_retries;
    let retry_delay = config.proof_config.retry_delay;
    let batch_limit = config.proof_config.batch_limit;
    let dummy_mode = config.proof_config.dummy_mode;

    // Calculate block delays for L1 and L2 chains
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

    // Spawn proof generator task
    tokio::spawn(async move {
        let proof_config = proof_generator::ProofGeneratorConfig {
            max_retries,
            retry_delay,
            batch_size: batch_limit,
            ethereum_max_block_delay_secs: l1_max_block_delay,
            l2_max_block_delay_secs: l2_max_block_delay,
            dummy_mode,
        };

        if let Err(e) = ProofGenerator::new(proof_config, db_clone).await {
            error!("Proof generator failed: {:?}", e);
        }
    });
}

/// Start transaction manager component
///
/// This function creates and starts the transaction manager component,
/// which submits transactions to destination chains for events that
/// have completed proof generation and are ready for execution.
///
/// ## Transaction Management:
/// - Monitors events in 'BatchIncluded' status
/// - Submits transactions to destination chains
/// - Handles gas estimation and transaction retries
/// - Updates event status based on transaction results
///
/// ## Configuration:
/// - Creates chain-specific transaction configurations
/// - Configures gas estimation and retry logic
/// - Sets up transaction timeouts and limits
///
/// # Arguments
/// * `config` - Reference to sequencer configuration
/// * `db` - Reference to database connection
///
/// # Example
/// ```rust
/// start_transaction_manager(&config, &db).await;
/// ```
async fn start_transaction_manager(config: &SequencerConfig, db: &Database) {
    // Create transaction configuration for all chains
    let transaction_config = create_transaction_config(config);
    let db_clone = db.clone();

    // Spawn transaction manager task
    tokio::spawn(async move {
        if let Err(e) = TransactionManager::new(transaction_config, db_clone).await {
            error!("Transaction manager failed: {:?}", e);
        }
    });
}

/// Wait for shutdown signal and perform cleanup
///
/// This function waits for a shutdown signal (Ctrl+C) and then
/// performs any necessary cleanup operations before the sequencer
/// terminates.
///
/// ## Shutdown Process:
/// 1. **Signal Handling**: Waits for Ctrl+C signal
/// 2. **Logging**: Logs shutdown message
/// 3. **Cleanup**: Allows components to finish gracefully
///
/// ## Signal Handling:
/// - Uses tokio::signal::ctrl_c() for cross-platform compatibility
/// - Handles graceful shutdown of all components
/// - Ensures database connections are closed properly
///
/// # Example
/// ```rust
/// wait_for_shutdown().await;
/// ```
async fn wait_for_shutdown() {
    // Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await.unwrap();

    // Log shutdown message
    info!("Shutting down sequencer...");
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
        if let Err(e) = EventProofReadyChecker::new(
            db_clone_proof,
            poll_interval,
            block_update_interval,
            proof_checker_chain_configs,
        )
        .await
        {
            error!("Event proof ready checker failed: {:?}", e);
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
        if let Err(e) = ResetTxManager::new(reset_tx_manager_config, db_clone_reset).await {
            error!("Reset tx manager failed: {:?}", e);
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
        if let Err(e) = gas_fee_distributer::GasFeeDistributer::new(
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
        )
        .await
        {
            error!("Gas fee distributer failed: {:?}", e);
        }
    });

    Ok(())
}

/// Start API server
///
/// This function creates and starts the API server component,
/// which provides REST endpoints for managing boundless users
/// and other database operations.
///
/// # Arguments
/// * `config` - Sequencer configuration
/// * `db` - Database connection
async fn start_api_server(_config: SequencerConfig, db: Database) {
    info!("start_api_server function called");
    
    // Create API configuration
    let api_config = ApiConfig {
        host: std::env::var("API_HOST").unwrap_or_else(|_| "0.0.0.0".to_string()),
        port: std::env::var("API_PORT")
            .unwrap_or_else(|_| "3000".to_string())
            .parse()
            .unwrap_or(3000),
        api_key: std::env::var("API_KEY").unwrap_or_else(|_| "default-api-key".to_string()),
    };

    info!("API config created: host={}, port={}", api_config.host, api_config.port);

    // Create and start API server in a separate task
    let api_server = ApiServer::new(api_config, db);
    info!("ApiServer instance created");
    
    tokio::spawn(async move {
        info!("API server task spawned, starting server...");
        if let Err(e) = api_server.start().await {
            error!("API server failed: {:?}", e);
        }
    });
    
    info!("API server task spawned successfully");
}
