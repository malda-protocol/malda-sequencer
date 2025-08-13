//! # Batch Event Listener Module
//!
//! This module provides a batch event listener that monitors blockchain events for batch processing
//! success and failure events across multiple chains. It handles both normal and retarded event
//! processing with unified filtering and parallel chain processing.
//!
//! ## Key Features:
//! - **Multi-Chain Support**: Processes events across multiple chains in parallel
//! - **Unified Event Filtering**: Single filter for both success and failure events
//! - **Fault Isolation**: Each chain processes independently - failures don't affect others
//! - **Provider Management**: Uses provider helper for reliable blockchain connections
//! - **Retry Logic**: Implements exponential backoff for connection failures
//! - **Event Separation**: Automatically separates success/failure events by topic
//!
//! ## Architecture:
//! ```
//! BatchEventListener::new()
//! ├── Spawn Tasks: One task per chain for parallel processing
//! ├── Process Chain: Individual chain processing with retry logic
//! ├── Unified Filter: Single filter for success/failure events
//! ├── Event Separation: Separate logs by topic[0] signature
//! ├── Database Updates: Update finished_events table
//! └── Provider Management: Use provider helper for connections
//! ```
//!
//! ## Workflow:
//! 1. **Initialization**: Creates tasks for each configured chain
//! 2. **Parallel Processing**: Each chain runs in its own task
//! 3. **Event Polling**: Continuously polls for new batch events
//! 4. **Event Separation**: Separates success/failure events by signature
//! 5. **Database Updates**: Updates event status in the database
//! 6. **Error Recovery**: Implements retry logic with exponential backoff

use alloy::{
    eips::BlockNumberOrTag,
    primitives::{Address, TxHash},
    providers::Provider,
    rpc::types::Filter,
};
use eyre::Result;
use futures;
use std::collections::HashMap;
use std::time::Duration as StdDuration;
use std::vec::Vec;
use tokio::time::interval;
use tracing::{debug, error, info};

use crate::events::{
    parse_batch_process_failed_event, parse_batch_process_success_event, BATCH_PROCESS_FAILED_SIG,
    BATCH_PROCESS_SUCCESS_SIG,
};
use crate::provider_helper::{ProviderConfig, ProviderState, ProviderType};
use alloy::primitives::keccak256;
use sequencer::database::{Database, EventStatus};

/// Configuration for a single chain in the batch event listener
///
/// This struct contains all necessary parameters for monitoring batch events
/// on a specific chain, including connection details and processing settings.
#[derive(Debug, Clone)]
pub struct ChainConfig {
    /// Primary WebSocket URL for blockchain connection
    pub primary_ws_url: String,
    /// Fallback WebSocket URL in case primary fails
    pub fallback_ws_url: String,
    /// Batch submitter contract address to monitor
    pub batch_submitter: Address,
    /// Chain ID for identification and logging
    pub chain_id: u64,
    /// Block delay for processing (how many blocks behind to process)
    pub block_delay: u64,
    /// Maximum allowed block delay in seconds
    pub max_block_delay_secs: u64,
    /// Whether this is a retarded listener (different processing logic)
    pub is_retarded: bool,
}

/// Configuration for batch event listening on multiple chains
///
/// This struct holds configurations for all chains that should be monitored
/// by the batch event listener. Each chain will be processed in parallel.
#[derive(Debug, Clone)]
pub struct BatchEventConfig {
    /// Vector of chain configurations for parallel processing
    pub chain_configs: Vec<ChainConfig>,
}

/// Batch event listener that processes success/failure events across multiple chains
///
/// This component monitors blockchain events for batch processing success and failure
/// events. It operates with parallel processing for multiple chains and implements
/// fault isolation - if one chain fails, others continue processing.
///
/// ## Processing Strategy
///
/// - **Parallel Processing**: Each chain runs in its own spawned task
/// - **Unified Filtering**: Single filter captures both success and failure events
/// - **Event Separation**: Logs are separated by topic[0] signature hash
/// - **Database Updates**: Events are moved to finished_events table
/// - **Retry Logic**: Exponential backoff for connection failures
///
/// ## Error Handling
///
/// - Individual chain failures don't affect other chains
/// - Connection failures trigger retry with exponential backoff
/// - Database errors are logged but don't stop processing
/// - Provider failures trigger fallback to secondary provider
pub struct BatchEventListener;

impl BatchEventListener {
    /// Creates and starts a batch event listener for multiple chains
    ///
    /// This method initializes the batch event listener and spawns independent
    /// tasks for each configured chain. Each chain processes events independently
    /// with its own retry logic and fault isolation.
    ///
    /// ## Parallel Processing
    ///
    /// - Each chain runs in its own `tokio::spawn` task
    /// - Tasks run concurrently for optimal performance
    /// - Failures on one chain don't affect others
    /// - All tasks are awaited with `futures::future::join_all`
    ///
    /// ## Fault Isolation
    ///
    /// - Each chain has independent error handling
    /// - Connection failures are isolated per chain
    /// - Database errors don't propagate across chains
    /// - Provider issues are handled independently
    ///
    /// # Arguments
    /// * `config` - Batch event listener configuration with chain configs
    /// * `db` - Database connection for event updates
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let config = BatchEventConfig {
    ///     chain_configs: vec![chain1, chain2, chain3],
    /// };
    /// let db = Database::new("connection_string").await?;
    /// BatchEventListener::new(config, db).await?;
    /// ```
    pub async fn new(config: BatchEventConfig, db: Database) -> Result<()> {
        info!(
            "Starting batch event listener for {} chains",
            config.chain_configs.len()
        );

        let mut tasks = Vec::new();

        // Spawn independent task for each chain
        for chain_config in config.chain_configs {
            let db_clone = db.clone();
            let task =
                tokio::spawn(async move { Self::process_chain(chain_config, db_clone).await });
            tasks.push(task);
        }

        // Wait for all tasks to complete (they shouldn't unless there's an error)
        futures::future::join_all(tasks).await;

        Ok(())
    }

    /// Processes events for a single chain
    ///
    /// This function implements the core event processing logic for a single chain.
    /// It sets up a unified filter for both success and failure events, manages
    /// provider connections, and implements resilient error handling that never
    /// terminates the task.
    ///
    /// ## Processing Flow
    ///
    /// 1. **Filter Setup**: Creates unified filter for success/failure events
    /// 2. **Provider Management**: Uses provider helper for reliable connections
    /// 3. **Block Processing**: Processes blocks with configurable delay
    /// 4. **Event Processing**: Separates and processes events by type
    /// 5. **Database Updates**: Updates event status in the database
    ///
    /// ## Error Handling
    ///
    /// - **Never Terminates**: All errors are handled gracefully with continue to next cycle
    /// - **Provider Failures**: Log error and continue to next polling cycle
    /// - **Block Processing Failures**: Log error and continue to next polling cycle
    /// - **Database Errors**: Log error but don't stop processing
    /// - **Resilient Design**: Task runs indefinitely, never giving up on reconnection
    ///
    /// ## Block Processing
    ///
    /// - Processes blocks with configurable delay (block_delay)
    /// - Tracks last processed block to avoid duplicates
    /// - Handles chain reorganization gracefully
    /// - Processes blocks in ranges for efficiency
    ///
    /// # Arguments
    /// * `config` - Chain configuration with connection and processing settings
    /// * `db` - Database connection for event updates
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn process_chain(config: ChainConfig, db: Database) -> Result<()> {
        info!(
            "Starting batch event listener for submitter={:?} chain={}",
            config.batch_submitter, config.chain_id
        );

        // Create unified filter for both success and failure events
        let unified_filter = Filter::new()
            .events(vec![BATCH_PROCESS_SUCCESS_SIG, BATCH_PROCESS_FAILED_SIG])
            .address(config.batch_submitter);

        let mut last_processed_block = 0u64;
        let poll_interval = StdDuration::from_secs(5);
        let mut interval = interval(poll_interval);

        info!("Started polling for batch events");

        // Initialize provider state for this chain
        let mut provider_state = Self::create_provider_state(&config);

        // Main processing loop - runs indefinitely
        loop {
            // Wait for next polling cycle
            interval.tick().await;

            // Get fresh provider with fallback logic
            let (provider, _is_fallback) = match provider_state.get_fresh_provider().await {
                Ok(result) => result,
                Err(e) => {
                    error!(
                        "Failed to get fresh provider for chain {}: {:?}, retrying in next cycle",
                        config.chain_id, e
                    );
                    continue; // Continue to next polling cycle instead of terminating
                }
            };

            // Get current block number for processing logic
            let current_block = match provider.get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    error!(
                        "Failed to get block number for chain {}: {:?}, retrying in next cycle",
                        config.chain_id, e
                    );
                    continue; // Continue to next polling cycle instead of terminating
                }
            };

            // Initialize last_processed_block on first run
            if last_processed_block == 0 {
                last_processed_block = if current_block > config.block_delay {
                    current_block - config.block_delay - 2
                } else {
                    0
                };
                info!(
                    "Initializing last_processed_block to {} (current: {}, delay: {})",
                    last_processed_block, current_block, config.block_delay
                );
                continue;
            }

            // Calculate target block with delay
            let target_block = if current_block > config.block_delay {
                current_block - config.block_delay
            } else {
                current_block
            };

            // Skip if no new blocks to process
            if last_processed_block >= target_block {
                debug!("No new blocks to process. Last processed: {}, Target: {}, Current: {}, Delay: {}", 
                       last_processed_block, target_block, current_block, config.block_delay);
                continue;
            }

            let from_block = last_processed_block + 1;

            debug!(
                "Processing blocks {} to {} (current: {}, delay: {})",
                from_block, target_block, current_block, config.block_delay
            );

            // Process the block range and handle errors
            if let Err(e) = Self::process_block_range(
                &provider,
                &unified_filter,
                from_block,
                target_block,
                &db,
                &config,
            )
            .await
            {
                error!(
                    "Failed to process block range for chain {}: {:?}, retrying in next cycle",
                    config.chain_id, e
                );
                continue; // Continue to next polling cycle instead of terminating
            }

            // Update the last processed block for next iteration
            last_processed_block = target_block;
        }
    }

    /// Processes events for a specific block range
    ///
    /// This function handles the processing of events within a specific block range.
    /// It fetches logs using the unified filter, separates them by event type,
    /// and updates the database with the processed events.
    ///
    /// ## Processing Steps
    ///
    /// 1. **Log Fetching**: Gets all logs for the block range using unified filter
    /// 2. **Timestamp Collection**: Fetches block timestamps for event processing
    /// 3. **Event Separation**: Separates logs into success and failure events
    /// 4. **Database Updates**: Updates event status in the database
    ///
    /// ## Error Handling
    ///
    /// - **Provider Errors**: Log error and return Err to trigger retry in next cycle
    /// - **Timestamp Errors**: Log error and return Ok to continue processing
    /// - **Database Errors**: Log error but don't stop processing
    /// - **Resilient Design**: Never crashes, always returns Result for proper handling
    ///
    /// # Arguments
    /// * `provider` - Provider for blockchain access
    /// * `unified_filter` - Filter for both success and failure events
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// * `db` - Database connection for event updates
    /// * `config` - Chain configuration for processing context
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn process_block_range(
        provider: &ProviderType,
        unified_filter: &Filter,
        from_block: u64,
        to_block: u64,
        db: &Database,
        config: &ChainConfig,
    ) -> Result<()> {
        // Create range-specific filter
        let range_filter = unified_filter
            .clone()
            .from_block(from_block)
            .to_block(to_block);

        // Fetch all logs for the block range
        let all_logs = match provider.get_logs(&range_filter).await {
            Ok(logs) => logs,
            Err(e) => {
                error!(
                    "Provider failed to get logs for chain {}: {:?}",
                    config.chain_id, e
                );
                return Err(e.into());
            }
        };

        // Get block timestamps for event processing
        let block_timestamps = match Self::get_block_timestamps(provider, from_block, to_block).await {
            Ok(timestamps) => timestamps,
            Err(e) => {
                error!(
                    "Failed to get block timestamps for chain {}: {:?}, continuing to next cycle",
                    config.chain_id, e
                );
                return Ok(()); // Return early but don't crash
            }
        };

        // Separate logs by event type (success vs failure)
        let (success_logs, failure_logs) = Self::separate_logs_by_event_type(&all_logs);

        // Process success events
        Self::process_batch_events(
            &success_logs,
            &block_timestamps,
            db,
            config,
            EventStatus::TxProcessSuccess,
            "success",
        )
        .await;

        // Process failure events
        Self::process_batch_events(
            &failure_logs,
            &block_timestamps,
            db,
            config,
            EventStatus::TxProcessFail,
            "failure",
        )
        .await;

        Ok(())
    }

    /// Separates logs by event type using topic[0] signature hash
    ///
    /// This function analyzes the topic[0] of each log to determine whether it's
    /// a success or failure event. It uses keccak256 hashes of the event signatures
    /// for comparison.
    ///
    /// ## Event Identification
    ///
    /// - **Success Events**: Match `BATCH_PROCESS_SUCCESS_SIG` hash
    /// - **Failure Events**: Match `BATCH_PROCESS_FAILED_SIG` hash
    /// - **Unknown Events**: Logged for debugging but not processed
    ///
    /// ## Hash Comparison
    ///
    /// - Pre-computes signature hashes for efficiency
    /// - Compares topic[0] as byte slices
    /// - Handles missing topics gracefully
    ///
    /// # Arguments
    /// * `logs` - All logs from the unified filter
    ///
    /// # Returns
    /// * `(Vec<alloy::rpc::types::Log>, Vec<alloy::rpc::types::Log>)` - Success and failure logs
    fn separate_logs_by_event_type(
        logs: &[alloy::rpc::types::Log],
    ) -> (Vec<alloy::rpc::types::Log>, Vec<alloy::rpc::types::Log>) {
        let mut success_logs = Vec::new();
        let mut failure_logs = Vec::new();

        // Pre-compute signature hashes for efficiency
        let success_sig_hash = keccak256(BATCH_PROCESS_SUCCESS_SIG.as_bytes());
        let failure_sig_hash = keccak256(BATCH_PROCESS_FAILED_SIG.as_bytes());

        for log in logs {
            if let Some(topic0) = log.topics().get(0) {
                let topic0_bytes = topic0.as_slice();

                // Compare topic[0] with pre-computed hashes
                if topic0_bytes == success_sig_hash.as_slice() {
                    success_logs.push(log.clone());
                } else if topic0_bytes == failure_sig_hash.as_slice() {
                    failure_logs.push(log.clone());
                } else {
                    debug!("Unknown event type with topic[0]: {:?}", topic0);
                }
            }
        }

        (success_logs, failure_logs)
    }

    /// Gets block timestamps for a range of blocks
    ///
    /// This function fetches block timestamps for a range of blocks, which are
    /// needed for event processing and database updates. It handles missing blocks
    /// and provider errors gracefully.
    ///
    /// ## Block Fetching
    ///
    /// - Fetches each block individually for timestamp extraction
    /// - Handles missing blocks with appropriate error messages
    /// - Continues processing on individual block failures
    /// - Returns HashMap for efficient timestamp lookup
    ///
    /// ## Error Handling
    ///
    /// - Missing blocks return specific error messages
    /// - Provider errors are propagated up for retry logic
    /// - Individual block failures don't stop the entire range
    /// - Resilient design that never crashes the task
    ///
    /// # Arguments
    /// * `provider` - Provider for blockchain access
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    ///
    /// # Returns
    /// * `Result<HashMap<u64, u64>>` - Block number to timestamp mapping
    async fn get_block_timestamps(
        provider: &ProviderType,
        from_block: u64,
        to_block: u64,
    ) -> Result<HashMap<u64, u64>> {
        let mut block_timestamps = HashMap::new();

        // Fetch timestamps for each block in the range
        for block_number in from_block..=to_block {
            let block = match provider
                .get_block_by_number(BlockNumberOrTag::Number(block_number))
                .await
            {
                Ok(Some(block)) => block,
                Ok(None) => {
                    error!("Block {} not found, skipping", block_number);
                    continue; // Skip missing blocks but continue processing
                }
                Err(e) => {
                    error!("Failed to get block {}: {:?}", block_number, e);
                    return Err(e.into()); // Propagate provider errors for retry logic
                }
            };

            // Extract timestamp from block header
            let timestamp = block.header.inner.timestamp;
            block_timestamps.insert(block_number, timestamp);
        }

        Ok(block_timestamps)
    }

    /// Processes batch events and updates database
    ///
    /// This function processes a batch of events and updates the database with
    /// their status. It handles both success and failure events using pattern
    /// matching on the EventStatus parameter.
    ///
    /// ## Event Processing
    ///
    /// - **Success Events**: Extracts init_hash from success event logs
    /// - **Failure Events**: Extracts init_hash from failure event logs
    /// - **Timestamps**: Maps block numbers to timestamps for database updates
    /// - **Database Updates**: Moves events to finished_events table
    ///
    /// ## Pattern Matching
    ///
    /// Uses match statements to handle different event types:
    /// - `EventStatus::TxProcessSuccess`: Processes success events
    /// - `EventStatus::TxProcessFail`: Processes failure events
    /// - Other statuses: Logged as unsupported
    ///
    /// ## Database Operations
    ///
    /// - Updates event status in finished_events table
    /// - Handles retarded vs normal event processing
    /// - Logs success/failure of database operations
    ///
    /// # Arguments
    /// * `logs` - Event logs to process
    /// * `block_timestamps` - Block timestamp mapping for event processing
    /// * `db` - Database connection for updates
    /// * `config` - Chain configuration for processing context
    /// * `event_status` - Event status to use for database update
    /// * `event_type_name` - Name of event type for logging (e.g., "success", "failure")
    async fn process_batch_events(
        logs: &[alloy::rpc::types::Log],
        block_timestamps: &HashMap<u64, u64>,
        db: &Database,
        config: &ChainConfig,
        event_status: EventStatus,
        event_type_name: &str,
    ) {
        // Extract transaction hashes based on event type
        let events: Vec<TxHash> = match event_status {
            EventStatus::TxProcessSuccess => logs
                .iter()
                .map(|log| parse_batch_process_success_event(log).init_hash)
                .collect(),
            EventStatus::TxProcessFail => logs
                .iter()
                .map(|log| parse_batch_process_failed_event(log).init_hash)
                .collect(),
            _ => {
                error!("Unsupported event status: {:?}", event_status);
                return;
            }
        };

        // Skip processing if no events
        if events.is_empty() {
            return;
        }

        // Extract timestamps based on event type
        let timestamps: Vec<i64> = match event_status {
            EventStatus::TxProcessSuccess => logs
                .iter()
                .map(|log| {
                    let event = parse_batch_process_success_event(log);
                    *block_timestamps.get(&event.block_number).unwrap_or(&0) as i64
                })
                .collect(),
            EventStatus::TxProcessFail => logs
                .iter()
                .map(|log| {
                    let event = parse_batch_process_failed_event(log);
                    *block_timestamps.get(&event.block_number).unwrap_or(&0) as i64
                })
                .collect(),
            _ => {
                error!("Unsupported event status: {:?}", event_status);
                return;
            }
        };

        info!(
            "Processing batch of {} {} events on chain {}",
            events.len(),
            event_type_name,
            config.chain_id
        );

        // Update database with processed events
        if let Err(e) = db
            .update_finished_events(&events, event_status, &timestamps, config.is_retarded)
            .await
        {
            error!("Failed to update {} events: {:?}", event_type_name, e);
        } else {
            info!(
                "Successfully migrated {} {} events to finished_events",
                events.len(),
                event_type_name
            );
        }
    }

    /// Creates provider state for the batch event listener
    ///
    /// This function creates a ProviderState instance configured for WebSocket
    /// connections, which is optimal for event listening due to real-time
    /// subscription capabilities.
    ///
    /// ## Configuration
    ///
    /// - **WebSocket Connection**: Uses WebSocket for real-time event monitoring
    /// - **Primary/Fallback URLs**: Configures both primary and fallback connections
    /// - **Chain-Specific Settings**: Uses chain-specific delay and ID settings
    /// - **Provider Helper Integration**: Leverages shared provider management logic
    ///
    /// # Arguments
    /// * `config` - Chain configuration with connection details
    ///
    /// # Returns
    /// * `ProviderState` - Configured provider state for this chain
    fn create_provider_state(config: &ChainConfig) -> ProviderState {
        let provider_config = ProviderConfig {
            primary_url: config.primary_ws_url.clone(),
            fallback_url: config.fallback_ws_url.clone(),
            max_block_delay_secs: config.max_block_delay_secs,
            chain_id: config.chain_id,
            use_websocket: true, // Use WebSocket for real-time event monitoring
        };

        ProviderState::new(provider_config)
    }
}
