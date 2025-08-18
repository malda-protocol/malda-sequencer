//! # Event Listener Module
//!
//! This module provides a simplified, unified event listener for blockchain events across multiple chains.
//! It processes events from various markets and chains in parallel, with fault isolation and efficient
//! provider management using the shared provider helper.
//!
//! ## Key Features:
//! - **Unified Processing**: Single listener handles all chains, markets, and events
//! - **Parallel Execution**: Independent tasks for each chain with fault isolation
//! - **Shared Provider Management**: Uses common provider helper for efficient connection management
//! - **Automatic Recovery**: Fallback provider support with freshness validation
//! - **Event Classification**: Unified event type system for consistent processing
//!
//! ## Architecture:
//! ```
//! EventListener::new()
//! ├── Spawns parallel tasks for each EventConfig
//! ├── Each task runs process_chain() independently
//! ├── Uses ProviderState for provider management
//! ├── Block range calculation and log retrieval
//! ├── Event parsing and database storage
//! └── Continuous polling loop
//! ```

use alloy::{
    eips::BlockNumberOrTag,
    primitives::Address,
    providers::Provider,
    rpc::types::{Filter, Log},
};
use chrono::Utc;
use eyre::{eyre, Result};
use std::collections::HashMap;
use tokio::time::interval;
use tracing::{error, info};

use crate::events::{parse_supplied_event, parse_withdraw_on_extension_chain_event};
use crate::provider_helper::{ProviderConfig, ProviderState, ProviderType};
use malda_rs::constants::*;
use sequencer::database::{Database, EventStatus, EventUpdate};

/// Configuration for event listening on a specific chain
///
/// This struct contains all necessary parameters for monitoring events on a blockchain:
/// - Connection details (primary/fallback WebSocket URLs)
/// - Chain-specific parameters (chain ID, polling intervals)
/// - Block range configuration for event processing
/// - Market addresses and event signatures to monitor
/// - Retry and delay settings for reliability
#[derive(Debug, Clone)]
pub struct EventConfig {
    /// Primary WebSocket URL for blockchain connection
    pub primary_ws_url: String,
    /// Fallback WebSocket URL in case primary fails
    pub fallback_ws_url: String,
    /// Unique identifier for the blockchain network
    pub chain_id: u64,
    /// Interval between polling cycles in seconds
    pub poll_interval_secs: u64,
    /// Maximum allowed delay for block freshness in seconds
    pub max_block_delay_secs: u64,
    /// Offset from current block for processing range start
    pub block_range_offset_from: u64,
    /// Offset from current block for processing range end
    pub block_range_offset_to: u64,
    /// Whether this is a "retarded" (delayed) listener configuration
    pub is_retarded: bool,
    /// List of market contract addresses to monitor for events
    pub markets: Vec<Address>,
    /// List of event signatures to filter and process
    pub events: Vec<String>,
}

/// Manages the state for a single chain's event processing
///
/// This struct encapsulates all stateful information needed for processing events
/// on a specific chain, including configuration, block tracking, provider management,
/// and polling intervals.
struct ChainState {
    /// Configuration for this chain's event processing
    config: EventConfig,
    /// Last processed block number to maintain continuity
    last_processed_block: u64,
    /// Provider state for this chain (using shared provider helper)
    provider_state: ProviderState,
    /// Polling interval timer for this chain
    interval: tokio::time::Interval,
}

impl ChainState {
    /// Creates a new chain state with the given configuration
    ///
    /// # Arguments
    /// * `config` - The event configuration for this chain
    ///
    /// # Returns
    /// A new ChainState instance with initialized polling interval and provider state
    fn new(config: EventConfig) -> Self {
        let interval = interval(std::time::Duration::from_secs(config.poll_interval_secs));

        // Create provider configuration for this chain
        let provider_config = ProviderConfig {
            primary_url: config.primary_ws_url.clone(),
            fallback_url: config.fallback_ws_url.clone(),
            max_block_delay_secs: config.max_block_delay_secs,
            chain_id: config.chain_id,
            use_websocket: true, // Event listener uses WebSocket connections
        };

        Self {
            config,
            last_processed_block: 0,
            provider_state: ProviderState::new(provider_config),
            interval,
        }
    }

    /// Waits for the next polling cycle based on the configured interval
    async fn wait_for_next_poll(&mut self) {
        self.interval.tick().await;
    }

    /// Updates the last processed block number to maintain processing continuity
    ///
    /// # Arguments
    /// * `block` - The block number that was just processed
    fn update_last_processed_block(&mut self, block: u64) {
        self.last_processed_block = block;
    }
}

/// Main event listener that processes blockchain events across multiple chains
///
/// This struct orchestrates the event listening process by:
/// 1. Managing multiple chain configurations
/// 2. Spawning parallel processing tasks
/// 3. Coordinating database operations
/// 4. Providing fault isolation between chains
pub struct EventListener;

impl EventListener {
    /// Creates and starts a new event listener for multiple chains
    ///
    /// This method initializes the event listener and immediately starts processing
    /// events for all configured chains in parallel. Each chain runs independently,
    /// ensuring that failures on one chain don't affect others.
    ///
    /// # Arguments
    /// * `configs` - Vector of event configurations for different chains
    /// * `db` - Database connection for storing processed events
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let configs = vec![chain1_config, chain2_config];
    /// let db = Database::new("connection_string").await?;
    /// EventListener::new(configs, db).await?;
    /// ```
    pub async fn new(configs: Vec<EventConfig>, db: Database) -> Result<()> {
        info!(
            "Starting event listener for {} chains in parallel",
            configs.len()
        );

        // Create independent tasks for each chain configuration
        let mut tasks = Vec::new();

        for config in configs {
            let db_clone = db.clone();
            let task = tokio::spawn(async move { Self::process_chain(config, db_clone).await });
            tasks.push(task);
        }

        // Wait for all chain processing tasks to complete (they run indefinitely)
        futures::future::join_all(tasks).await;

        Ok(())
    }

    /// Processes events for a single chain in a continuous loop
    ///
    /// This is the main processing function for each chain. It:
    /// 1. Sets up the event filter for the chain's markets and events
    /// 2. Enters a continuous polling loop
    /// 3. Uses shared provider helper for fresh provider connections
    /// 4. Calculates block ranges for event processing
    /// 5. Retrieves and processes blockchain logs
    /// 6. Updates the last processed block
    ///
    /// # Arguments
    /// * `config` - Event configuration for this chain
    /// * `db` - Database connection for event storage
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn process_chain(config: EventConfig, db: Database) -> Result<()> {
        info!(
            "Starting chain {} with {} markets and {} events",
            config.chain_id,
            config.markets.len(),
            config.events.len()
        );

        // Create filter for all markets and events on this chain
        let filter = Filter::new()
            .address(config.markets.clone())
            .events(&config.events);
        let mut chain_state = ChainState::new(config);

        info!("Started polling for chain {}", chain_state.config.chain_id);

        // Main processing loop - runs indefinitely
        loop {
            // Wait for next polling cycle
            chain_state.wait_for_next_poll().await;

            // Get a fresh provider connection using shared provider helper with retry logic
            let (provider, _is_fallback) = match chain_state.provider_state.get_fresh_provider().await {
                Ok(result) => result,
                Err(e) => {
                    error!(
                        "Failed to get fresh provider for chain {}: {:?}, retrying in next cycle",
                        chain_state.config.chain_id, e
                    );
                    continue; // Continue to next polling cycle instead of terminating
                }
            };

            // Get current block number for range calculation
            let current_block = match provider.get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    error!(
                        "Failed to get block number for chain {}: {:?}, retrying in next cycle",
                        chain_state.config.chain_id, e
                    );
                    continue; // Continue to next polling cycle instead of terminating
                }
            };

            // Skip if we're already at or past the current block
            if chain_state.last_processed_block >= current_block {
                continue;
            }

            // Calculate block range for event processing
            let from_block = if chain_state.last_processed_block > 0 {
                chain_state.last_processed_block + 1
            } else {
                current_block - chain_state.config.block_range_offset_from
            };
            let to_block = current_block - chain_state.config.block_range_offset_to;

            // Get block timestamps for event processing
            let block_timestamps = match Self::get_block_timestamps(&provider, from_block, to_block, &chain_state.config).await {
                Ok(timestamps) => timestamps,
                Err(e) => {
                    error!(
                        "Failed to get block timestamps for chain {}: {:?}, retrying in next cycle",
                        chain_state.config.chain_id, e
                    );
                    continue; // Continue to next polling cycle instead of terminating
                }
            };

            // Get and process blockchain logs
            let logs = match Self::get_logs(
                &provider,
                &filter,
                from_block,
                current_block,
                &chain_state.config,
            ).await {
                Ok(logs) => logs,
                Err(e) => {
                    error!(
                        "Failed to get logs for chain {}: {:?}, retrying in next cycle",
                        chain_state.config.chain_id, e
                    );
                    continue; // Continue to next polling cycle instead of terminating
                }
            };

            // Process events if any logs were found
            if !logs.is_empty() {
                if let Err(e) = Self::process_logs(logs, &block_timestamps, &chain_state.config, &db).await {
                    error!(
                        "Failed to process logs for chain {}: {:?}, continuing to next cycle",
                        chain_state.config.chain_id, e
                    );
                    // Don't continue here - we still want to update the last processed block
                }
            }

            // Update the last processed block for next iteration
            chain_state.update_last_processed_block(to_block);
        }
    }

    /// Retrieves block timestamps for a range of blocks
    ///
    /// This method fetches block information for each block in the specified range
    /// and extracts timestamps for event processing. It handles missing blocks
    /// gracefully by logging errors and continuing.
    ///
    /// # Arguments
    /// * `provider` - Blockchain provider connection
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// * `config` - Event configuration for error context
    ///
    /// # Returns
    /// * `Result<HashMap<u64, u64>>` - Map of block numbers to timestamps
    async fn get_block_timestamps(
        provider: &ProviderType,
        from_block: u64,
        to_block: u64,
        config: &EventConfig,
    ) -> Result<HashMap<u64, u64>> {
        let mut block_timestamps = HashMap::new();

        // Iterate through each block in the range
        for block_number in from_block..=to_block {
            let block = match provider
                .get_block_by_number(BlockNumberOrTag::Number(block_number))
                .await
            {
                Ok(Some(block)) => block,
                Ok(None) => {
                    error!(
                        "Block {} not found on chain {}",
                        block_number, config.chain_id
                    );
                    continue;
                }
                Err(e) => {
                    error!(
                        "Failed to get block {} on chain {}: {:?}",
                        block_number, config.chain_id, e
                    );
                    continue;
                }
            };
            block_timestamps.insert(block_number, block.header.inner.timestamp);
        }

        Ok(block_timestamps)
    }

    /// Retrieves blockchain logs for the specified filter and block range
    ///
    /// This method queries the blockchain for logs matching the given filter
    /// within the specified block range. It's the core method for fetching
    /// raw event data from the blockchain.
    ///
    /// # Arguments
    /// * `provider` - Blockchain provider connection
    /// * `filter` - Event filter defining what logs to retrieve
    /// * `from_block` - Starting block number (inclusive)
    /// * `to_block` - Ending block number (inclusive)
    /// * `config` - Event configuration for error context
    ///
    /// # Returns
    /// * `Result<Vec<Log>>` - Vector of blockchain logs
    async fn get_logs(
        provider: &ProviderType,
        filter: &Filter,
        from_block: u64,
        to_block: u64,
        config: &EventConfig,
    ) -> Result<Vec<Log>> {
        provider
            .get_logs(&filter.clone().from_block(from_block).to_block(to_block))
            .await
            .map_err(|e| eyre!("Failed to get logs for chain {}: {:?}", config.chain_id, e))
    }

    /// Processes a batch of blockchain logs into database events
    ///
    /// This method takes raw blockchain logs and converts them into structured
    /// event updates for database storage. It handles event parsing, timestamp
    /// assignment, and batch database operations.
    ///
    /// # Arguments
    /// * `logs` - Vector of blockchain logs to process
    /// * `block_timestamps` - Map of block numbers to timestamps
    /// * `config` - Event configuration for processing context
    /// * `db` - Database connection for event storage
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn process_logs(
        logs: Vec<Log>,
        block_timestamps: &HashMap<u64, u64>,
        config: &EventConfig,
        db: &Database,
    ) -> Result<()> {
        let mut event_updates = Vec::new();

        // Process each log into a structured event update
        for log in logs {
            let timestamp = *block_timestamps
                .get(&log.block_number.unwrap_or_default())
                .unwrap_or(&0);

            if let Ok(event_update) = Self::process_event(config, log, timestamp).await {
                event_updates.push(event_update);
            }
        }

        // Batch save all processed events to database
        if !event_updates.is_empty() {
            if let Err(e) = Self::save_events_to_database(event_updates, config, db).await {
                error!("Failed to save events to database for chain {}: {:?}, continuing to next cycle", config.chain_id, e);
                // Don't return error, just log and continue
            }
        }

        Ok(())
    }

    /// Saves processed events to the database
    ///
    /// This method performs batch database operations to store processed events.
    /// It handles both normal and retarded event configurations and provides
    /// detailed logging for monitoring and debugging.
    ///
    /// # Arguments
    /// * `event_updates` - Vector of processed event updates to save
    /// * `config` - Event configuration for context and retarded flag
    /// * `db` - Database connection for storage
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn save_events_to_database(
        event_updates: Vec<EventUpdate>,
        config: &EventConfig,
        db: &Database,
    ) -> Result<()> {
        if let Err(e) = db.add_new_events(&event_updates, config.is_retarded).await {
            error!(
                "Failed to save events to database for chain {}: {:?}",
                config.chain_id, e
            );
            return Err(e);
        }

        info!(
            "Added {} new events to database for chain {}",
            event_updates.len(),
            config.chain_id
        );
        Ok(())
    }

    /// Processes a single blockchain log into a database event update
    ///
    /// This method converts a raw blockchain log into a structured event update
    /// suitable for database storage. It extracts transaction information, block
    /// details, and event-specific data.
    ///
    /// # Arguments
    /// * `config` - Event configuration for chain context
    /// * `log` - Blockchain log to process
    /// * `timestamp` - Block timestamp for the log
    ///
    /// # Returns
    /// * `Result<EventUpdate>` - Processed event update
    async fn process_event(config: &EventConfig, log: Log, timestamp: u64) -> Result<EventUpdate> {
        let tx_hash = log
            .transaction_hash
            .ok_or_else(|| eyre!("No transaction hash"))?;

        let current_block = log.block_number.unwrap_or_default() as i32;
        let market = log.address();

        // Create base event update with common fields
        let mut event_update = EventUpdate {
            tx_hash,
            src_chain_id: Some(config.chain_id.try_into().unwrap()),
            market: Some(market),
            received_at_block: Some(current_block),
            received_block_timestamp: Some(timestamp as i64),
            status: EventStatus::Received,
            received_at: Some(Utc::now()),
            ..Default::default()
        };

        // Process event based on chain type (host vs extension)
        Self::process_event_by_chain_type(config, &log, &mut event_update)?;

        Ok(event_update)
    }

    /// Routes event processing based on chain type
    ///
    /// This method determines whether the event is from a host chain (Linea)
    /// or an extension chain and routes to the appropriate processing function.
    ///
    /// # Arguments
    /// * `config` - Event configuration containing chain ID
    /// * `log` - Blockchain log to process
    /// * `event_update` - Mutable event update to populate
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    fn process_event_by_chain_type(
        config: &EventConfig,
        log: &Log,
        event_update: &mut EventUpdate,
    ) -> Result<()> {
        // Route based on chain ID - Linea chains are host chains
        match config.chain_id {
            LINEA_SEPOLIA_CHAIN_ID | LINEA_CHAIN_ID => {
                Self::process_host_chain_event(log, event_update)
            }
            _ => Self::process_extension_chain_event(log, event_update),
        }
    }

    /// Processes events from host chains (Linea mainnet and testnet)
    ///
    /// Host chain events are typically withdrawal events that indicate users
    /// withdrawing assets from the host chain to extension chains. This method
    /// parses the event data and populates the event update with host-specific
    /// information.
    ///
    /// # Arguments
    /// * `log` - Blockchain log containing the event data
    /// * `event_update` - Mutable event update to populate
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    fn process_host_chain_event(log: &Log, event_update: &mut EventUpdate) -> Result<()> {
        let event = parse_withdraw_on_extension_chain_event(log);
        let event_signature = &log.topics()[0];

        // Use unified event type system for classification
        let event_type = crate::events::EventType::from_signature(event_signature.as_slice());

        if event_type == crate::events::EventType::Unknown {
            return Err(eyre!(
                "Unknown host chain event signature: {}",
                hex::encode(event_signature)
            ));
        }

        // Populate event update with host chain specific data
        event_update.msg_sender = Some(event.sender);
        event_update.dst_chain_id = Some(event.dst_chain_id);
        event_update.amount = Some(event.amount);
        event_update.target_function = Some(event_type.target_function().to_string());
        event_update.event_type = Some(event_type.to_string().to_string());

        Ok(())
    }

    /// Processes events from extension chains (non-Linea chains)
    ///
    /// Extension chain events are typically supply/mint events that indicate
    /// users supplying assets to the protocol on extension chains. This method
    /// parses the event data and populates the event update with extension-specific
    /// information.
    ///
    /// # Arguments
    /// * `log` - Blockchain log containing the event data
    /// * `event_update` - Mutable event update to populate
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    fn process_extension_chain_event(log: &Log, event_update: &mut EventUpdate) -> Result<()> {
        let event = parse_supplied_event(log);

        // Use unified event type system for classification
        let event_type =
            crate::events::EventType::from_method_selector(&event.linea_method_selector);

        if event_type == crate::events::EventType::Unknown {
            return Err(eyre!(
                "Invalid method selector: {}",
                event.linea_method_selector
            ));
        }

        // Populate event update with extension chain specific data
        event_update.msg_sender = Some(event.from);
        event_update.src_chain_id = Some(event.src_chain_id);
        event_update.dst_chain_id = Some(event.dst_chain_id);
        event_update.amount = Some(event.amount);
        event_update.target_function = Some(event_type.target_function().to_string());
        event_update.event_type = Some(event_type.to_string().to_string());

        Ok(())
    }
}