use alloy::{
    eips::BlockNumberOrTag,
    primitives::{Address, TxHash},
    providers::Provider,
    rpc::types::Filter,
};
use eyre::Result;
use std::collections::HashMap;
use std::time::Duration as StdDuration;
use std::vec::Vec;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info};
use futures;

use crate::events::{
    parse_batch_process_failed_event, parse_batch_process_success_event, BATCH_PROCESS_FAILED_SIG, BATCH_PROCESS_SUCCESS_SIG,
};
use alloy::primitives::keccak256;
use sequencer::database::{Database, EventStatus};
use crate::provider_helper::{ProviderConfig, ProviderState, ProviderType};

#[derive(Debug, Clone)]
pub struct ChainConfig {
    pub primary_ws_url: String,
    pub fallback_ws_url: String,
    pub batch_submitter: Address,
    pub chain_id: u64,
    pub block_delay: u64,
    pub max_block_delay_secs: u64,
    pub is_retarded: bool,
}

/// Configuration for batch event listening on multiple chains
#[derive(Debug, Clone)]
pub struct BatchEventConfig {
    pub chain_configs: Vec<ChainConfig>,
}

/// Batch event listener that processes success/failure events across multiple chains
pub struct BatchEventListener;

impl BatchEventListener {
    /// Creates and starts a batch event listener for multiple chains
    /// 
    /// Spawns independent tasks for each chain with fault isolation.
    /// Each chain runs independently - failures on one don't affect others.
    /// 
    /// # Arguments
    /// * `config` - Batch event listener configuration
    /// * `db` - Database connection for event updates
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error status
    pub async fn new(config: BatchEventConfig, db: Database) -> Result<()> {
        info!("Starting batch event listener for {} chains", config.chain_configs.len());

        let mut tasks = Vec::new();
        
        for chain_config in config.chain_configs {
            let db_clone = db.clone();
            let task = tokio::spawn(async move {
                Self::process_chain(chain_config, db_clone).await
            });
            tasks.push(task);
        }

        futures::future::join_all(tasks).await;
        
        Ok(())
    }

    /// Processes events for a single chain
    /// 
    /// Sets up unified filter for success/failure events and enters polling loop.
    /// Uses provider helper for connections and handles retry logic.
    /// 
    /// # Arguments
    /// * `config` - Chain configuration
    /// * `db` - Database connection
    /// 
    /// # Returns
    /// * `Result<()>` - Success or error status
    async fn process_chain(config: ChainConfig, db: Database) -> Result<()> {
        info!(
            "Starting batch event listener for submitter={:?} chain={}",
            config.batch_submitter, config.chain_id
        );

        let mut retry_count = 0;
        let max_retries = 5;
        let mut retry_delay = StdDuration::from_secs(1);

        loop {
            let unified_filter = Filter::new()
                .events(vec![BATCH_PROCESS_SUCCESS_SIG, BATCH_PROCESS_FAILED_SIG])
                .address(config.batch_submitter);

            debug!("Setting up polling with unified filter");

            let mut last_processed_block = 0u64;
            let poll_interval = StdDuration::from_secs(5);
            let mut interval = interval(poll_interval);

            info!("Started polling for batch events");

            let mut provider_state = Self::create_provider_state(&config);

            loop {
                interval.tick().await;

                let (provider, _is_fallback) = match provider_state.get_fresh_provider().await {
                    Ok(result) => result,
                    Err(e) => {
                        error!("Failed to get fresh provider: {:?}", e);
                        if retry_count >= max_retries {
                            error!("Max retries reached, giving up on reconnection");
                            return Err(e);
                        }
                        retry_count += 1;
                        retry_delay *= 2;
                        info!(
                            "Waiting {} seconds before reconnection attempt {}",
                            retry_delay.as_secs(),
                            retry_count
                        );
                        sleep(retry_delay).await;
                        continue;
                    }
                };

                let current_block = match provider.get_block_number().await {
                    Ok(block) => block,
                    Err(e) => {
                        error!("Provider failed to get block number: {:?}", e);
                        continue;
                    }
                };

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

                let target_block = if current_block > config.block_delay {
                    current_block - config.block_delay
                } else {
                    current_block
                };

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

                if let Err(e) = Self::process_block_range(
                    &provider,
                    &unified_filter,
                    from_block,
                    target_block,
                    &db,
                    &config,
                ).await {
                    error!("Failed to process block range: {:?}", e);
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Err(e);
                    }
                    retry_count += 1;
                    retry_delay *= 2;
                    info!(
                        "Waiting {} seconds before reconnection attempt {}",
                        retry_delay.as_secs(),
                        retry_count
                    );
                    sleep(retry_delay).await;
                    continue;
                }

                last_processed_block = target_block;
            }
        }
    }

    /// Processes events for a specific block range
    /// 
    /// # Arguments
    /// * `provider` - Provider for blockchain access
    /// * `unified_filter` - Filter for both success and failure events
    /// * `from_block` - Starting block number
    /// * `to_block` - Ending block number
    /// * `db` - Database connection
    /// * `config` - Configuration
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
        let range_filter = unified_filter
            .clone()
            .from_block(from_block)
            .to_block(to_block);

        let all_logs = match provider.get_logs(&range_filter).await {
            Ok(logs) => logs,
            Err(e) => {
                error!("Provider failed to get logs: {:?}", e);
                return Err(e.into());
            }
        };

        let block_timestamps = Self::get_block_timestamps(provider, from_block, to_block).await?;

        let (success_logs, failure_logs) = Self::separate_logs_by_event_type(&all_logs);

        Self::process_batch_events(&success_logs, &block_timestamps, db, config, EventStatus::TxProcessSuccess, "success").await;

        Self::process_batch_events(&failure_logs, &block_timestamps, db, config, EventStatus::TxProcessFail, "failure").await;

        Ok(())
    }

    /// Separates logs by event type using topic[0]
    /// 
    /// # Arguments
    /// * `logs` - All logs from the unified filter
    /// 
    /// # Returns
    /// * `(Vec<alloy::rpc::types::Log>, Vec<alloy::rpc::types::Log>)` - Success and failure logs
    fn separate_logs_by_event_type(logs: &[alloy::rpc::types::Log]) -> (Vec<alloy::rpc::types::Log>, Vec<alloy::rpc::types::Log>) {
        let mut success_logs = Vec::new();
        let mut failure_logs = Vec::new();

        let success_sig_hash = keccak256(BATCH_PROCESS_SUCCESS_SIG.as_bytes());
        let failure_sig_hash = keccak256(BATCH_PROCESS_FAILED_SIG.as_bytes());

        for log in logs {
            if let Some(topic0) = log.topics().get(0) {
                let topic0_bytes = topic0.as_slice();
                
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
    /// # Arguments
    /// * `provider` - Provider for blockchain access
    /// * `from_block` - Starting block number
    /// * `to_block` - Ending block number
    /// 
    /// # Returns
    /// * `Result<HashMap<u64, u64>>` - Block number to timestamp mapping
    async fn get_block_timestamps(
        provider: &ProviderType,
        from_block: u64,
        to_block: u64,
    ) -> Result<HashMap<u64, u64>> {
        let mut block_timestamps = HashMap::new();

        for block_number in from_block..=to_block {
            let block = match provider
                .get_block_by_number(BlockNumberOrTag::Number(block_number))
                .await
            {
                Ok(Some(block)) => block,
                Ok(None) => return Err(eyre::eyre!("Block {} not found", block_number)),
                Err(e) => {
                    error!("Failed to get block {}: {:?}", block_number, e);
                    return Err(e.into());
                }
            };

            let timestamp = block.header.inner.timestamp;
            block_timestamps.insert(block_number, timestamp);
        }

        Ok(block_timestamps)
    }

    /// Processes batch events and updates database
    /// 
    /// # Arguments
    /// * `logs` - Event logs to process
    /// * `block_timestamps` - Block timestamp mapping
    /// * `db` - Database connection
    /// * `config` - Configuration
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
        let events: Vec<TxHash> = match event_status {
            EventStatus::TxProcessSuccess => {
                logs.iter()
                    .map(|log| parse_batch_process_success_event(log).init_hash)
                    .collect()
            }
            EventStatus::TxProcessFail => {
                logs.iter()
                    .map(|log| parse_batch_process_failed_event(log).init_hash)
                    .collect()
            }
            _ => {
                error!("Unsupported event status: {:?}", event_status);
                return;
            }
        };

        if events.is_empty() {
            return;
        }

        let timestamps: Vec<i64> = match event_status {
            EventStatus::TxProcessSuccess => {
                logs.iter()
                    .map(|log| {
                        let event = parse_batch_process_success_event(log);
                        *block_timestamps.get(&event.block_number).unwrap_or(&0) as i64
                    })
                    .collect()
            }
            EventStatus::TxProcessFail => {
                logs.iter()
                    .map(|log| {
                        let event = parse_batch_process_failed_event(log);
                        *block_timestamps.get(&event.block_number).unwrap_or(&0) as i64
                    })
                    .collect()
            }
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

        if let Err(e) = db
            .update_finished_events(
                &events,
                event_status,
                &timestamps,
                config.is_retarded,
            )
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
    /// # Arguments
    /// * `config` - Chain configuration
    /// 
    /// # Returns
    /// * `ProviderState` - Provider state for this chain
    fn create_provider_state(config: &ChainConfig) -> ProviderState {
        let provider_config = ProviderConfig {
            primary_url: config.primary_ws_url.clone(),
            fallback_url: config.fallback_ws_url.clone(),
            max_block_delay_secs: config.max_block_delay_secs,
            chain_id: config.chain_id,
            use_websocket: true,
        };

        ProviderState::new(provider_config)
    }
}
