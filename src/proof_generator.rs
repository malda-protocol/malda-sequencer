//! # Proof Generator Module
//!
//! ## Overview
//!
//! The Proof Generator is a critical component of the sequencer that processes blockchain events
//! and generates Zero-Knowledge (ZK) proofs. It handles the complete proof generation workflow
//! from event retrieval to database updates.
//!
//! ## Architecture
//!
//! ```
//! ProofGenerator::new()
//! ├── Continuous polling loop
//! │   ├── Retrieve ready events from database
//! │   ├── Process events in batches
//! │   └── Update database with proof data
//! ├── Event processing
//! │   ├── Group events by source chain
//! │   ├── Sort events (Linea first, then by destination)
//! │   └── Prepare data for proof generation
//! ├── Proof generation
//! │   ├── Try boundless proof first (onchain=true)
//! │   ├── Fall back to SDK method if boundless fails
//! │   └── Use provider validation for fallback decisions
//! └── Database updates
//!     ├── Create journal index mapping
//!     └── Update events with proof data
//! ```
//!
//! ## Key Features
//!
//! - **Batch Processing**: Processes events in configurable batches for efficiency
//! - **Dual Proof Methods**: Supports both boundless and SDK proof generation
//! - **Automatic Fallback**: Falls back to SDK when boundless fails
//! - **Provider Validation**: Uses shared provider helper for freshness checks
//! - **Database Integration**: Updates events with proof data and indices
//! - **Retry Logic**: Implements exponential backoff with configurable retries
//!
//! ## Proof Generation Flow
//!
//! 1. **Event Retrieval**: Polls database for events ready for proof generation
//! 2. **Event Grouping**: Groups events by source chain and sorts them
//! 3. **Boundless Attempt**: Tries boundless proof generation first (onchain=true)
//! 4. **SDK Fallback**: Falls back to SDK method if boundless fails
//! 5. **Provider Validation**: Uses provider helper to determine fallback mode
//! 6. **Database Update**: Updates events with proof data and journal indices
//!
//! ## Configuration
//!
//! The proof generator is configured via `ProofGeneratorConfig`:
//! - `max_retries`: Maximum retry attempts for proof generation
//! - `retry_delay`: Delay between retry attempts
//! - `batch_size`: Number of events to process in each batch
//! - `ethereum_max_block_delay_secs`: Max block delay for Ethereum chains
//! - `l2_max_block_delay_secs`: Max block delay for L2 chains
//!
//! ## Provider Integration
//!
//! Uses the shared `provider_helper` module for:
//! - Provider freshness validation
//! - Primary/fallback provider selection
//! - Connection caching and reuse
//! - Automatic fallback detection
//!
//! ## Error Handling
//!
//! - **Boundless Failures**: Automatically falls back to SDK method
//! - **Provider Issues**: Uses provider helper's fallback logic
//! - **Database Errors**: Logs errors and continues processing
//! - **Retry Logic**: Implements exponential backoff with max retries

use alloy::primitives::{Address, Bytes, TxHash, U256};
use alloy_sol_types::SolValue;
use eyre::{eyre, Result};
use hex;
use sequencer::database::{Database, EventUpdate};
use std::env;
use std::time::Duration;
use tokio::time::{sleep, Instant};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::provider_helper::{ProviderConfig, ProviderState};
use malda_rs::constants::*;
use malda_rs::types::{abi, SolidityDataType, TakeLastXBytes};
use malda_rs::viewcalls::{get_proof_data_prove_boundless, get_proof_data_prove_sdk};

/// Proof information containing journal, seal, and metadata
///
/// This struct contains all the data needed to verify a ZK proof:
/// - `journal`: The proof journal containing the public inputs
/// - `seal`: The cryptographic seal proving the computation
/// - `uuid`: Unique identifier for the proof session
/// - `stark_time`: Time taken for STARK proof generation
/// - `snark_time`: Time taken for SNARK proof generation
/// - `total_cycles`: Total computational cycles used
#[derive(Debug)]
pub struct ProofInfo {
    /// The proof journal containing public inputs and outputs
    pub journal: Bytes,
    /// The cryptographic seal proving the computation
    pub seal: Bytes,
    /// Unique identifier for the proof session
    pub uuid: String,
    /// Time taken for STARK proof generation (in seconds)
    pub stark_time: i32,
    /// Time taken for SNARK proof generation (in seconds)
    pub snark_time: i32,
    /// Total computational cycles used for the proof
    pub total_cycles: i64,
}

/// Configuration for proof generation
///
/// This struct contains all the parameters needed to configure
/// the proof generator's behavior and performance characteristics.
#[derive(Clone)]
pub struct ProofGeneratorConfig {
    /// Maximum number of retry attempts for proof generation
    pub max_retries: u32,
    /// Delay between retry attempts
    pub retry_delay: Duration,
    /// Number of events to process in each batch
    pub batch_size: usize,
    /// Maximum allowed delay for Ethereum block freshness in seconds
    pub ethereum_max_block_delay_secs: u64,
    /// Maximum allowed delay for L2 block freshness in seconds
    pub l2_max_block_delay_secs: u64,
    /// Whether to enable dummy proof generation mode
    pub dummy_mode: bool,
}

/// Main proof generator that processes events and generates proofs
///
/// This is a unit struct that provides static methods for proof generation.
/// The actual state is managed through the configuration and database.
pub struct ProofGenerator;

impl ProofGenerator {
    /// Creates and starts a new proof generator
    ///
    /// This method initializes the proof generator and immediately starts processing
    /// events in a continuous loop. It retrieves events ready for proof generation
    /// from the database and processes them in batches.
    ///
    /// ## Workflow
    ///
    /// 1. **Initialization**: Sets up logging and configuration
    /// 2. **Polling Loop**: Continuously polls for ready events
    /// 3. **Batch Processing**: Processes events in background tasks
    /// 4. **Error Handling**: Logs errors but continues processing
    ///
    /// ## Configuration
    ///
    /// The proof delay is read from the `PROOF_GENERATOR_PROOF_REQUEST_DELAY`
    /// environment variable and must be set in the `.env` file.
    ///
    /// ## Background Processing
    ///
    /// Each batch is processed in a separate background task to avoid blocking
    /// the main polling loop. This ensures that one slow batch doesn't delay
    /// processing of subsequent batches.
    ///
    /// # Arguments
    /// * `config` - Proof generator configuration containing retry settings and batch size
    /// * `db` - Database connection for event retrieval and updates
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status (errors are logged but don't stop processing)
    ///
    /// # Errors
    /// * Returns error if environment variable `PROOF_GENERATOR_PROOF_REQUEST_DELAY` is not set
    /// * Returns error if the delay value cannot be parsed as a u64
    pub async fn new(config: ProofGeneratorConfig, db: Database) -> Result<()> {
        info!(
            "Starting proof generator with batch size {}",
            config.batch_size
        );

        // Read proof delay from environment variable
        let proof_delay = Duration::from_secs(
            env::var("PROOF_GENERATOR_PROOF_REQUEST_DELAY")
                .expect("PROOF_GENERATOR_PROOF_REQUEST_DELAY must be set in .env")
                .parse::<u64>()
                .unwrap(),
        );

        info!(
            "Proof generator started, polling for events every {:?}",
            proof_delay
        );

        // Main processing loop - runs indefinitely
        loop {
            // Retrieve events ready for proof generation
            let events = match Self::get_ready_events(&db, proof_delay, config.batch_size).await {
                Ok(events) => events,
                Err(e) => {
                    error!("Database error in proof generator: {:?}, continuing to next cycle", e);
                    sleep(proof_delay).await;
                    continue;
                }
            };

            if !events.is_empty() {
                info!(
                    "Processing batch of {} events for proof generation",
                    events.len()
                );

                // Process batch in background task to avoid blocking the main loop
                let config_clone = config.clone();
                let db_clone = db.clone();
                tokio::spawn(async move {
                    if let Err(e) = Self::process_batch(events, config_clone, db_clone).await {
                        error!("Failed to generate proofs for batch: {}", e);
                    }
                });
            }

            // Wait before next polling cycle
            sleep(proof_delay).await;
        }
    }

    /// Retrieves events ready for proof generation from the database
    ///
    /// This method queries the database for events that are ready to be processed
    /// for proof generation. Events are considered ready if they have been in the
    /// queue for at least the specified proof delay duration.
    ///
    /// ## Event Selection Criteria
    ///
    /// Events are selected based on:
    /// - Time since they were queued (must exceed proof delay)
    /// - Batch size limit (to prevent memory issues)
    /// - Database availability and connection status
    ///
    /// # Arguments
    /// * `db` - Database connection for querying events
    /// * `proof_delay` - Minimum delay required before events are considered ready
    /// * `batch_size` - Maximum number of events to retrieve in one batch
    ///
    /// # Returns
    /// * `Result<Vec<EventUpdate>>` - Events ready for proof generation, or error if query fails
    async fn get_ready_events(
        db: &Database,
        proof_delay: Duration,
        batch_size: usize,
    ) -> Result<Vec<EventUpdate>> {
        db.get_ready_to_request_proof_events(proof_delay.as_secs() as i64, batch_size as i64)
            .await
    }

    /// Processes a batch of events for proof generation
    ///
    /// This method handles the complete proof generation workflow for a batch of events:
    /// 1. Groups events by source chain for efficient processing
    /// 2. Generates proof using boundless or SDK method
    /// 3. Updates database with proof data and journal indices
    ///
    /// ## Processing Steps
    ///
    /// 1. **Event Grouping**: Events are grouped by source chain and sorted
    /// 2. **Proof Generation**: Attempts boundless first, then SDK fallback
    /// 3. **Database Update**: Updates all events in the batch with proof data
    /// 4. **Performance Tracking**: Measures and logs processing time
    ///
    /// ## Error Handling
    ///
    /// - If proof generation fails, the error is logged and returned
    /// - Database update errors are logged and returned
    /// - Individual event failures don't stop the entire batch
    ///
    /// # Arguments
    /// * `events` - Events to process for proof generation
    /// * `config` - Proof generator configuration for retry settings
    /// * `db` - Database connection for updates
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn process_batch(
        events: Vec<EventUpdate>,
        config: ProofGeneratorConfig,
        db: Database,
    ) -> Result<()> {
        let start_time = Instant::now();

        // Group events by source chain for efficient proof generation
        let (users, markets, dst_chain_ids, src_chain_ids) =
            Self::group_events_by_chain(events.clone());

        debug!(
            "Starting batch proof generation for {} source chains",
            src_chain_ids.len()
        );

        // Generate proof with retry logic and fallback methods
        let proof_info = match Self::generate_proof(users, markets, dst_chain_ids, src_chain_ids, config).await {
            Ok(proof_info) => proof_info,
            Err(e) => {
                error!("Failed to generate proof: {:?}", e);
                return Err(e);
            }
        };

        // Log processing time for performance monitoring
        let duration_ms = start_time.elapsed().as_millis() as u64;
        debug!("Batch proof generation completed in {}ms", duration_ms);

        // Update database with proof data and journal indices
        if let Err(e) = Self::update_database_with_proof(events, proof_info, db).await {
            error!("Failed to update database with proof data: {:?}", e);
            return Err(e);
        }

        Ok(())
    }

    /// Groups events by source chain for proof generation
    ///
    /// This method sorts and groups events by their source chain to optimize
    /// proof generation. Events are sorted with Linea first (as the primary chain),
    /// then by destination chain for consistent ordering.
    ///
    /// ## Sorting Logic
    ///
    /// 1. **Primary Sort**: By source chain ID (Linea first)
    /// 2. **Secondary Sort**: By destination chain ID
    /// 3. **Grouping**: Events with same source chain are grouped together
    ///
    /// ## Data Structure
    ///
    /// Returns four vectors where each index corresponds to a source chain:
    /// - `users`: User addresses for each source chain
    /// - `markets`: Market addresses for each source chain
    /// - `dst_chain_ids`: Destination chain IDs for each source chain
    /// - `src_chain_ids`: Source chain IDs (one per group)
    ///
    /// ## Example
    ///
    /// If we have events from Linea and Ethereum:
    /// - `users[0]`: All user addresses from Linea events
    /// - `users[1]`: All user addresses from Ethereum events
    /// - `src_chain_ids[0]`: Linea chain ID
    /// - `src_chain_ids[1]`: Ethereum chain ID
    ///
    /// # Arguments
    /// * `events` - Events to group by source chain
    ///
    /// # Returns
    /// * `(Vec<Vec<Address>>, Vec<Vec<Address>>, Vec<Vec<u64>>, Vec<u64>)` - Grouped data for proof generation
    fn group_events_by_chain(
        mut events: Vec<EventUpdate>,
    ) -> (
        Vec<Vec<Address>>,
        Vec<Vec<Address>>,
        Vec<Vec<u64>>,
        Vec<u64>,
    ) {
        // Sort events by source chain (Linea first) and destination chain
        events.sort_by(|a, b| {
            let a_src = a.src_chain_id.unwrap_or(LINEA_CHAIN_ID as u32) as u64;
            let b_src = b.src_chain_id.unwrap_or(LINEA_CHAIN_ID as u32) as u64;
            let a_dst = a.dst_chain_id.unwrap_or(0) as u64;
            let b_dst = b.dst_chain_id.unwrap_or(0) as u64;

            // Primary sort by source chain, secondary by destination
            match a_src.cmp(&b_src) {
                std::cmp::Ordering::Equal => a_dst.cmp(&b_dst),
                other => other,
            }
        });

        // Initialize vectors to hold grouped data
        let mut users: Vec<Vec<Address>> = Vec::new();
        let mut markets: Vec<Vec<Address>> = Vec::new();
        let mut dst_chain_ids: Vec<Vec<u64>> = Vec::new();
        let mut src_chain_ids: Vec<u64> = Vec::new();

        // Track current group for batching
        let mut current_src_chain: Option<u64> = None;
        let mut current_users: Vec<Address> = Vec::new();
        let mut current_markets: Vec<Address> = Vec::new();
        let mut current_dst_chains: Vec<u64> = Vec::new();

        // Group events by source chain
        for event in events.iter() {
            let src_chain = event.src_chain_id.unwrap_or(0) as u64;
            let dst_chain = event.dst_chain_id.unwrap_or(0) as u64;
            let user = event.msg_sender.unwrap_or_default();
            let market = event.market.unwrap_or_default();

            // If we encounter a new source chain, save the current group and start a new one
            if current_src_chain != Some(src_chain) {
                if !current_users.is_empty() {
                    users.push(current_users);
                    markets.push(current_markets);
                    dst_chain_ids.push(current_dst_chains);
                    src_chain_ids.push(current_src_chain.unwrap());
                }
                current_users = Vec::new();
                current_markets = Vec::new();
                current_dst_chains = Vec::new();
                current_src_chain = Some(src_chain);
            }

            // Add event data to current group
            current_users.push(user);
            current_markets.push(market);
            current_dst_chains.push(dst_chain);
        }

        // Push the last batch if it contains any events
        if !current_users.is_empty() {
            users.push(current_users);
            markets.push(current_markets);
            dst_chain_ids.push(current_dst_chains);
            src_chain_ids.push(current_src_chain.unwrap());
        }

        (users, markets, dst_chain_ids, src_chain_ids)
    }

    /// Updates database with proof data and journal indices
    ///
    /// This method updates all events in the batch with the generated proof data.
    /// It creates a mapping from transaction hash to journal index and updates
    /// the database with proof information.
    ///
    /// ## Update Process
    ///
    /// 1. **Index Mapping**: Creates mapping from TxHash to journal index
    /// 2. **Batch Update**: Updates all events with proof data in a single operation
    /// 3. **Error Handling**: Logs and returns errors if update fails
    ///
    /// ## Proof Data
    ///
    /// The following proof data is stored for each event:
    /// - `journal`: The proof journal containing public inputs
    /// - `seal`: The cryptographic seal proving the computation
    /// - `uuid`: Unique identifier for the proof session
    /// - `stark_time`: Time taken for STARK proof generation
    /// - `snark_time`: Time taken for SNARK proof generation
    /// - `total_cycles`: Total computational cycles used
    ///
    /// # Arguments
    /// * `events` - Original events for index mapping
    /// * `proof_info` - Generated proof information to store
    /// * `db` - Database connection for updates
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn update_database_with_proof(
        events: Vec<EventUpdate>,
        proof_info: ProofInfo,
        db: Database,
    ) -> Result<()> {
        // Create mapping of TxHash to journal index for efficient database updates
        let updates_with_index: Vec<(TxHash, i32)> = events
            .iter()
            .enumerate()
            .map(|(idx, event)| (event.tx_hash, idx as i32))
            .collect();

        // Update database with proof data if we have events to update
        if !updates_with_index.is_empty() {
            if let Err(e) = db
                .set_events_proof_received_with_index(
                    &updates_with_index,
                    &proof_info.journal,
                    &proof_info.seal,
                    &proof_info.uuid,
                    proof_info.stark_time,
                    proof_info.snark_time,
                    proof_info.total_cycles,
                )
                .await
            {
                error!("Failed to update events with proof data: {:?}", e);
                return Err(e);
            }
        }

        Ok(())
    }

    /// Generates proof with retry logic and fallback methods
    ///
    /// This method implements the complete proof generation workflow with automatic
    /// fallback logic. It first attempts boundless proof generation, then falls back
    /// to SDK method if boundless fails.
    ///
    /// ## Proof Generation Strategy
    ///
    /// 1. **Boundless Attempt**: Try boundless proof generation first (onchain=true)
    /// 2. **SDK Fallback**: If boundless fails, use SDK method with retry logic
    /// 3. **Provider Validation**: Use provider helper to determine fallback mode
    /// 4. **Retry Logic**: Implement exponential backoff with configurable retries
    ///
    /// ## Fallback Logic
    ///
    /// The fallback decision is based on:
    /// - Provider freshness and availability
    /// - Number of retry attempts (more attempts = more likely to use fallback)
    /// - Chain-specific provider status
    ///
    /// ## Performance Monitoring
    ///
    /// - Logs proof generation time
    /// - Tracks number of transactions processed
    /// - Monitors cycle count and timing statistics
    ///
    /// # Arguments
    /// * `users` - User addresses grouped by chain
    /// * `markets` - Market addresses grouped by chain
    /// * `dst_chain_ids` - Destination chain IDs grouped by chain
    /// * `src_chain_ids` - Source chain IDs
    /// * `config` - Proof generator configuration for retry settings
    ///
    /// # Returns
    /// * `Result<ProofInfo>` - Generated proof information
    async fn generate_proof(
        users: Vec<Vec<Address>>,
        markets: Vec<Vec<Address>>,
        dst_chain_ids: Vec<Vec<u64>>,
        src_chain_ids: Vec<u64>,
        config: ProofGeneratorConfig,
    ) -> Result<ProofInfo> {
        debug!(
            "Starting proof generation for {} source chains",
            src_chain_ids.len()
        );

        // Check if dummy mode is enabled via configuration
        if config.dummy_mode {
            // Use dummy proof generation when dummy mode is enabled (default)
            info!("Dummy proof mode enabled, using dummy proof generation");
            generate_dummy_proof(
                users,
                markets,
                dst_chain_ids,
                src_chain_ids,
                false, // l1_inclusion
                false, // fallback
            )
            .await
        } else {
            info!("Dummy proof mode disabled, using real proof generation");

            // Try boundless proof generation first (preferred method)
            if let Ok(proof_info) = Self::try_boundless_proof(
                users.clone(),
                markets.clone(),
                dst_chain_ids.clone(),
                src_chain_ids.clone(),
            )
            .await
            {
                return Ok(proof_info);
            }

            // Fall back to SDK method with retry logic if boundless fails
            let mut attempts = 0;
            loop {
                // Determine whether to use fallback mode based on provider status
                let should_use_fallback = Self::should_use_fallback(
                    &src_chain_ids,
                    attempts,
                    config.max_retries,
                    config.ethereum_max_block_delay_secs,
                    config.l2_max_block_delay_secs,
                )
                .await;

                if should_use_fallback {
                    info!(
                        "Using fallback mode for proof generation (attempt {})",
                        attempts
                    );
                } else {
                    info!(
                        "Using primary mode for proof generation (attempt {})",
                        attempts
                    );
                }

                let start_time = Instant::now();

                // Attempt SDK proof generation
                match get_proof_data_prove_sdk(
                    users.clone(),
                    markets.clone(),
                    dst_chain_ids.clone(),
                    src_chain_ids.clone(),
                    false, // l1_inclusion
                    should_use_fallback,
                )
                .await
                {
                    Ok(proof_info) => {
                        let duration = start_time.elapsed();
                        let proof_result = Self::create_proof_info_from_sdk(proof_info)?;

                        // Log performance statistics
                        let tx_num = users.clone().iter().flatten().count();
                        info!(
                            "Generated proof for {} transactions with {} cycles in {:?}{}",
                            tx_num,
                            proof_result.total_cycles,
                            duration,
                            if should_use_fallback {
                                " (fallback mode)"
                            } else {
                                ""
                            }
                        );

                        return Ok(proof_result);
                    }
                    Err(e) if attempts < config.max_retries => {
                        attempts += 1;
                        warn!(
                            "Proof generation attempt {} failed: {}. Retrying...",
                            attempts, e
                        );
                        sleep(config.retry_delay).await;
                    }
                    Err(e) => {
                        error!(
                            "Failed to generate proof after {} attempts: {}",
                            attempts, e
                        );
                        return Err(eyre!(
                            "Failed to generate proof after {} attempts: {}",
                            attempts,
                            e
                        ));
                    }
                }
            }
        }
    }

    /// Attempts boundless proof generation
    ///
    /// This method attempts to generate a proof using the boundless method with
    /// onchain=true. Boundless proofs are preferred as they are more efficient
    /// and provide better performance characteristics.
    ///
    /// ## Boundless Method
    ///
    /// The boundless method:
    /// - Uses onchain=true for better performance
    /// - Generates proofs directly without intermediate steps
    /// - Returns journal and seal data for verification
    ///
    /// ## Error Handling
    ///
    /// - If boundless succeeds, returns the proof immediately
    /// - If boundless fails, logs the error and returns Err
    /// - Special handling for CryptoProvider errors (TLS/HTTPS issues)
    ///
    /// ## Performance
    ///
    /// - Logs the number of transactions processed
    /// - Records proof generation time
    /// - Tracks journal and seal sizes for monitoring
    ///
    /// # Arguments
    /// * `users` - User addresses grouped by chain
    /// * `markets` - Market addresses grouped by chain
    /// * `dst_chain_ids` - Destination chain IDs grouped by chain
    /// * `src_chain_ids` - Source chain IDs
    ///
    /// # Returns
    /// * `Result<ProofInfo>` - Generated proof information or error
    async fn try_boundless_proof(
        users: Vec<Vec<Address>>,
        markets: Vec<Vec<Address>>,
        dst_chain_ids: Vec<Vec<u64>>,
        src_chain_ids: Vec<u64>,
    ) -> Result<ProofInfo> {
        info!("Attempting boundless proof generation with onchain=true");

        let tx_num = users.iter().flatten().count();
        match get_proof_data_prove_boundless(
            users,
            markets,
            dst_chain_ids,
            src_chain_ids,
            false, // l1_inclusion
            false, // fallback
            true,  // onchain
        )
        .await
        {
            Ok((journal, seal)) => {
                info!(
                    "Successfully generated boundless proof for {} transactions",
                    tx_num
                );

                // Log proof details for debugging and monitoring
                debug!(
                    "Boundless proof details - journal: 0x{}, seal: 0x{}",
                    hex::encode(&journal),
                    hex::encode(&seal)
                );

                // Create proof info with boundless-specific metadata
                Ok(ProofInfo {
                    journal,
                    seal,
                    uuid: "boundless".to_string(),
                    stark_time: 0,
                    snark_time: 0,
                    total_cycles: 0,
                })
            }
            Err(e) => {
                warn!(
                    "Boundless proof generation failed: {}. Falling back to SDK method",
                    e
                );
                // Special handling for CryptoProvider errors
                if e.to_string().contains("CryptoProvider") {
                    error!("CryptoProvider error detected - this may indicate a TLS/HTTPS configuration issue");
                }
                Err(eyre!("Boundless proof generation failed: {}", e))
            }
        }
    }

    /// Determines whether to use fallback mode based on provider status
    ///
    /// This method checks the status of all source chain providers to determine
    /// whether fallback mode should be used for proof generation. It leverages
    /// the provider helper's built-in fallback detection logic.
    ///
    /// ## Fallback Decision Logic
    ///
    /// 1. **Retry Count**: If we've retried many times, always use fallback
    /// 2. **Provider Status**: Check if any chain's providers are having issues
    /// 3. **Provider Helper**: Use provider helper's built-in fallback detection
    ///
    /// ## Provider Validation
    ///
    /// For each source chain:
    /// - Creates provider configuration with primary and fallback URLs
    /// - Uses provider helper to check provider status
    /// - Considers fallback used if provider helper used fallback
    /// - Considers fallback needed if provider helper failed completely
    ///
    /// ## Chain Configuration
    ///
    /// Different chains have different block delay requirements:
    /// - Ethereum chains: Use `ethereum_max_block_delay_secs` (longer delays)
    /// - L2 chains: Use `l2_max_block_delay_secs` (shorter delays)
    ///
    /// # Arguments
    /// * `src_chain_ids` - Source chain IDs to check
    /// * `attempts` - Current retry attempt number
    /// * `max_retries` - Maximum number of retries
    /// * `ethereum_max_block_delay_secs` - Maximum block delay for Ethereum chains
    /// * `l2_max_block_delay_secs` - Maximum block delay for L2 chains
    ///
    /// # Returns
    /// * `bool` - True if fallback mode should be used
    async fn should_use_fallback(
        src_chain_ids: &[u64],
        attempts: u32,
        max_retries: u32,
        ethereum_max_block_delay_secs: u64,
        l2_max_block_delay_secs: u64,
    ) -> bool {
        // If we've retried many times, always use fallback for better reliability
        if attempts >= max_retries / 2 {
            return true;
        }

        // Check if any chain's providers are having issues
        for &chain_id in src_chain_ids {
            // Get chain-specific configuration
            let (primary_url, fallback_url, max_block_delay_secs) = Self::get_chain_config(
                chain_id,
                ethereum_max_block_delay_secs,
                l2_max_block_delay_secs,
            );

            // Create provider configuration for this chain
            let provider_config = ProviderConfig {
                primary_url,
                fallback_url,
                max_block_delay_secs,
                chain_id,
                use_websocket: false, // Proof generator uses HTTP connections
            };

            let mut provider_state = ProviderState::new(provider_config);

            // Use provider helper's built-in fallback detection
            match provider_state.get_fresh_provider().await {
                Ok((_provider, is_fallback_used)) => {
                    // If provider helper used fallback, we should use fallback mode
                    if is_fallback_used {
                        return true;
                    }
                }
                Err(_) => {
                    // If provider helper can't get any provider, use fallback mode
                    return true;
                }
            }
        }

        // All providers are healthy, use primary mode
        false
    }

    /// Gets chain configuration with primary and fallback URLs
    ///
    /// This method returns the appropriate RPC URLs and block delay settings
    /// for each supported chain. It handles both mainnet and testnet configurations
    /// and provides fallback URLs for reliability.
    ///
    /// ## Supported Chains
    ///
    /// - **Ethereum**: Mainnet and Sepolia testnet
    /// - **Optimism**: Mainnet and Sepolia testnet
    /// - **Linea**: Mainnet and Sepolia testnet
    /// - **Base**: Mainnet and Sepolia testnet
    ///
    /// ## URL Configuration
    ///
    /// Each chain has:
    /// - Primary URL: Primary RPC endpoint
    /// - Fallback URL: Backup RPC endpoint for reliability
    /// - Block delay: Maximum allowed block delay for freshness validation
    ///
    /// ## Block Delay Logic
    ///
    /// - **Ethereum chains**: Use `ethereum_max_block_delay_secs` (longer delays)
    /// - **L2 chains**: Use `l2_max_block_delay_secs` (shorter delays)
    ///
    /// # Arguments
    /// * `chain_id` - Chain ID to get configuration for
    /// * `ethereum_max_block_delay_secs` - Maximum block delay for Ethereum chains
    /// * `l2_max_block_delay_secs` - Maximum block delay for L2 chains
    ///
    /// # Returns
    /// * `(String, String, u64)` - Primary URL, fallback URL, and max block delay
    fn get_chain_config(
        chain_id: u64,
        ethereum_max_block_delay_secs: u64,
        l2_max_block_delay_secs: u64,
    ) -> (String, String, u64) {
        match chain_id {
            // Ethereum mainnet
            id if id == ETHEREUM_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("ETHEREUM", false, false).to_string(),
                malda_rs::constants::get_rpc_url("ETHEREUM", true, false).to_string(),
                ethereum_max_block_delay_secs,
            ),
            // Ethereum Sepolia testnet
            id if id == ETHEREUM_SEPOLIA_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("ETHEREUM", false, true).to_string(),
                malda_rs::constants::get_rpc_url("ETHEREUM", true, true).to_string(),
                ethereum_max_block_delay_secs,
            ),
            // Optimism mainnet
            id if id == OPTIMISM_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("OPTIMISM", false, false).to_string(),
                malda_rs::constants::get_rpc_url("OPTIMISM", true, false).to_string(),
                l2_max_block_delay_secs,
            ),
            // Optimism Sepolia testnet
            id if id == OPTIMISM_SEPOLIA_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("OPTIMISM", false, true).to_string(),
                malda_rs::constants::get_rpc_url("OPTIMISM", true, true).to_string(),
                l2_max_block_delay_secs,
            ),
            // Linea mainnet
            id if id == LINEA_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("LINEA", false, false).to_string(),
                malda_rs::constants::get_rpc_url("LINEA", true, false).to_string(),
                l2_max_block_delay_secs,
            ),
            // Linea Sepolia testnet
            id if id == LINEA_SEPOLIA_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("LINEA", false, true).to_string(),
                malda_rs::constants::get_rpc_url("LINEA", true, true).to_string(),
                l2_max_block_delay_secs,
            ),
            // Base mainnet
            id if id == BASE_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("BASE", false, false).to_string(),
                malda_rs::constants::get_rpc_url("BASE", true, false).to_string(),
                l2_max_block_delay_secs,
            ),
            // Base Sepolia testnet
            id if id == BASE_SEPOLIA_CHAIN_ID => (
                malda_rs::constants::get_rpc_url("BASE", false, true).to_string(),
                malda_rs::constants::get_rpc_url("BASE", true, true).to_string(),
                l2_max_block_delay_secs,
            ),
            // Unknown chain - default to Ethereum with warning
            _ => {
                warn!(
                    "Unknown chain ID: {}, defaulting to fallback mode",
                    chain_id
                );
                (
                    malda_rs::constants::get_rpc_url("ETHEREUM", false, false).to_string(),
                    malda_rs::constants::get_rpc_url("ETHEREUM", true, false).to_string(),
                    ethereum_max_block_delay_secs,
                )
            }
        }
    }

    /// Creates ProofInfo from SDK proof result
    ///
    /// This method converts the SDK proof result into the standardized ProofInfo
    /// format. It handles seal encoding and extracts all necessary metadata.
    ///
    /// ## Conversion Process
    ///
    /// 1. **Seal Encoding**: Encodes the receipt seal using risc0-ethereum-contracts
    /// 2. **Journal Extraction**: Extracts journal bytes from the receipt
    /// 3. **Metadata Mapping**: Maps SDK metadata to ProofInfo fields
    /// 4. **Error Handling**: Handles encoding errors gracefully
    ///
    /// ## Data Mapping
    ///
    /// - `receipt.journal.bytes` → `journal`
    /// - `encoded_seal` → `seal`
    /// - `proof_info.uuid` → `uuid`
    /// - `proof_info.stark_time` → `stark_time`
    /// - `proof_info.snark_time` → `snark_time`
    /// - `proof_info.stats.total_cycles` → `total_cycles`
    ///
    /// ## Error Handling
    ///
    /// - Seal encoding errors are logged and returned as errors
    /// - Journal extraction errors are handled gracefully
    /// - Debug logging provides visibility into the conversion process
    ///
    /// # Arguments
    /// * `proof_info` - SDK proof information to convert
    ///
    /// # Returns
    /// * `Result<ProofInfo>` - Converted proof information or error
    fn create_proof_info_from_sdk(
        proof_info: malda_rs::viewcalls::MaldaProveInfo,
    ) -> Result<ProofInfo> {
        let receipt = proof_info.receipt;

        // Encode the seal using risc0-ethereum-contracts
        let seal = match risc0_ethereum_contracts::encode_seal(&receipt) {
            Ok(seal_data) => {
                debug!("Successfully encoded seal");
                Bytes::from(seal_data)
            }
            Err(e) => {
                error!("Failed to encode seal: {}", e);
                return Err(eyre!("Failed to encode seal: {}", e));
            }
        };

        // Extract journal bytes from the receipt
        let journal = Bytes::from(receipt.journal.bytes);

        // Log proof details for debugging and monitoring
        debug!(
            "Proof details - journal: 0x{}, seal: 0x{}",
            hex::encode(&journal),
            hex::encode(&seal)
        );

        // Create ProofInfo with all metadata
        Ok(ProofInfo {
            journal,
            seal,
            uuid: proof_info.uuid,
            stark_time: proof_info.stark_time as i32,
            snark_time: proof_info.snark_time as i32,
            total_cycles: proof_info.stats.total_cycles as i64,
        })
    }
}

/// Generates a dummy proof for testing purposes
///
/// This function constructs a dummy proof for testing and development purposes.
/// It uses the same logic as the validator but with dummy values for amounts
/// and an empty seal since no real proof is generated.
///
/// ## Dummy Proof Construction
///
/// 1. **Journal Construction**: Uses the same input structure as the validator
/// 2. **Dummy Values**: Uses dummy amounts since real contract calls aren't made
/// 3. **Empty Seal**: Returns empty seal since no real proof is generated
/// 4. **Dummy Metadata**: Generates reasonable dummy timing and cycle values
///
/// ## Input Structure
///
/// For each user/market/target_chain_id tuple:
/// - User address
/// - Market address
/// - Dummy amount in
/// - Dummy amount out
/// - Source chain ID (with shift)
/// - Target chain ID (with shift)
/// - L1 inclusion flag
///
/// ## Use Cases
///
/// - **Testing**: Unit tests and integration tests
/// - **Development**: Local development without real proofs
/// - **Debugging**: Troubleshooting proof generation logic
///
/// # Arguments
/// * `users` - Vector of user address vectors, one per chain
/// * `markets` - Vector of market contract address vectors, one per chain
/// * `target_chain_ids` - Vector of target chain IDs to query (vector of vectors)
/// * `chain_ids` - Vector of chain IDs to query
/// * `l1_inclusion` - Whether to include L1 data in the proof
/// * `_fallback` - Whether to use fallback mode (unused in dummy mode)
///
/// # Returns
/// * `Result<ProofInfo>` - Dummy proof information with constructed journal and empty seal
pub async fn generate_dummy_proof(
    users: Vec<Vec<Address>>,
    markets: Vec<Vec<Address>>,
    target_chain_ids: Vec<Vec<u64>>,
    chain_ids: Vec<u64>,
    l1_inclusion: bool,
    _fallback: bool,
) -> Result<ProofInfo> {
    let start_time = Instant::now();

    // Construct journal data using the same logic as the validator
    let mut journal_data: Vec<Bytes> = Vec::new();

    // Process each chain's data
    for (chain_idx, chain_id) in chain_ids.iter().enumerate() {
        let chain_users = &users[chain_idx];
        let chain_markets = &markets[chain_idx];
        let chain_target_ids = &target_chain_ids[chain_idx];

        // Process each user/market/target_chain_id tuple for this chain
        for ((user, market), target_chain_id) in chain_users
            .iter()
            .zip(chain_markets.iter())
            .zip(chain_target_ids.iter())
        {
            // Use dummy amounts since we can't make real contract calls
            let dummy_amount_in = U256::from(1000000000000000000u64);
            let dummy_amount_out = U256::from(1000000000000000000u64);

            // Construct the same input structure as in the validator
            let input = vec![
                SolidityDataType::Address(*user),
                SolidityDataType::Address(*market),
                SolidityDataType::Number(dummy_amount_in),
                SolidityDataType::Number(dummy_amount_out),
                SolidityDataType::NumberWithShift(U256::from(*chain_id), TakeLastXBytes(32)),
                SolidityDataType::NumberWithShift(U256::from(*target_chain_id), TakeLastXBytes(32)),
                SolidityDataType::Bool(l1_inclusion),
            ];

            // Encode using the same method as the validator
            let (bytes, _hash) = abi::encode_packed(&input);
            journal_data.push(bytes.into());
        }
    }

    // Create dummy receipt with constructed journal and empty seal
    let journal = journal_data.abi_encode();
    let seal = Bytes::from(vec![]); // Empty seal since no real proof is generated

    // Generate dummy statistics
    let total_transactions = users.iter().flatten().count();
    let dummy_cycles = total_transactions as u64 * 1000; // Reasonable dummy cycle count

    // Generate UUID for the session
    let uuid = Uuid::new_v4().to_string();

    // Use dummy timing values
    let stark_time = 1u64; // 1 second
    let snark_time = 2u64; // 2 seconds

    let duration = start_time.elapsed();
    info!(
        "Generated dummy proof for {} transactions with {} cycles in {:?}",
        total_transactions, dummy_cycles, duration
    );
    debug!(
        "Dummy proof details - journal: 0x{}, seal: 0x{}",
        hex::encode(&journal),
        hex::encode(&seal)
    );

    Ok(ProofInfo {
        journal: journal.into(),
        seal,
        uuid,
        stark_time: stark_time as i32,
        snark_time: snark_time as i32,
        total_cycles: dummy_cycles as i64,
    })
}
