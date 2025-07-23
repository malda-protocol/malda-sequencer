use alloy::{
    eips::BlockNumberOrTag,
    network::Ethereum,
    primitives::{keccak256, Address},
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Log},
    transports::http::reqwest::Url,
};
use chrono::Utc;
use eyre::eyre;
use eyre::{Result, WrapErr};
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use tokio::time::interval;
use tokio::time::sleep;
use tracing::{debug, error, info, warn};

use crate::events::{parse_supplied_event, parse_withdraw_on_extension_chain_event};
use crate::{HOST_BORROW_ON_EXTENSION_CHAIN_SIG, HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG};
use malda_rs::constants::*;

// Import the chain ID constant from malda_rs
use malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID;

use crate::events::{MINT_EXTERNAL_SELECTOR, REPAY_EXTERNAL_SELECTOR};
use sequencer::database::{Database, EventStatus, EventUpdate};

// Type alias for the provider type returned by ProviderBuilder::connect_ws()
type ProviderType = alloy::providers::fillers::FillProvider<
    alloy::providers::fillers::JoinFill<
        alloy::providers::Identity,
        alloy::providers::fillers::JoinFill<
            alloy::providers::fillers::GasFiller,
            alloy::providers::fillers::JoinFill<
                alloy::providers::fillers::BlobGasFiller,
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::NonceFiller,
                    alloy::providers::fillers::ChainIdFiller,
                >,
            >,
        >,
    >,
    alloy::providers::RootProvider<Ethereum>,
>;

lazy_static! {
    pub static ref ETHEREUM_BLOCK_NUMBER: AtomicU64 = AtomicU64::new(0);
}

#[derive(Debug, Clone)]
pub struct EventConfig {
    pub primary_ws_url: String,
    pub fallback_ws_url: String,
    pub market: Address,
    pub event_signature: String,
    pub chain_id: u64,
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
    pub max_block_delay_secs: u64,
    pub block_range_offset_from: u64,
    pub block_range_offset_to: u64,
    pub is_retarded: bool,
}

#[derive(Clone)]
pub struct EventListener {
    config: EventConfig,
    db: Database,
    primary_provider: Option<ProviderType>,
}

impl EventListener {
    pub fn new(config: EventConfig, db: Database) -> Self {
        Self {
            config,
            db,
            primary_provider: None,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!(
            "Starting event listener for market={:?} chain={} event={}",
            self.config.market, self.config.chain_id, self.config.event_signature
        );

        let mut retry_count = 0;
        let max_retries = self.config.max_retries;
        let retry_delay = Duration::from_secs(self.config.retry_delay_secs);

        loop {
            match self.run_event_listener().await {
                Ok(_) => {
                    warn!("Event listener stopped, attempting to reconnect...");
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Ok(());
                    }
                    retry_count += 1;
                    info!(
                        "Waiting {} seconds before reconnection attempt {}",
                        retry_delay.as_secs(),
                        retry_count
                    );
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    error!("Event listener error: {:?}", e);
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Err(e);
                    }
                    retry_count += 1;
                    info!(
                        "Waiting {} seconds before reconnection attempt {}",
                        retry_delay.as_secs(),
                        retry_count
                    );
                    sleep(retry_delay).await;
                }
            }
        }
    }

    async fn run_event_listener(&mut self) -> Result<()> {
        debug!("Setting up polling with filter");
        let filter = Filter::new()
            .event(&self.config.event_signature)
            .address(self.config.market);

        // Track the latest block we've processed
        let mut last_processed_block = 0u64;

        // Poll interval in seconds
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut interval = interval(poll_interval);

        info!("Started polling for events");

        // Flag to track if we used fallback in the previous cycle
        let mut used_fallback = false;

        loop {
            interval.tick().await;

            // Determine which provider to use for this polling cycle
            let provider = match self.get_active_provider(used_fallback).await {
                Ok((provider, is_fallback)) => {
                    used_fallback = is_fallback;
                    provider
                }
                Err(e) => {
                    error!("Failed to get active provider: {:?}", e);
                    continue;
                }
            };

            // Get current block number
            let current_block = match provider.get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    error!("Provider failed to get block number: {:?}", e);
                    used_fallback = true; // Force new primary provider on next cycle
                    continue;
                }
            };

            // Don't query if we're already up to date
            if last_processed_block >= current_block {
                debug!("No new blocks since last poll (block: {})", current_block);
                continue;
            }

            // Set the from_block to the next block after the last processed one
            let from_block = if last_processed_block > 0 {
                last_processed_block + 1
            } else {
                current_block - self.config.block_range_offset_from
            };

            debug!(
                "Polling for logs from block {} to {}",
                from_block, current_block
            );

            let mut block_timestamps = HashMap::new();

            let to_block = current_block - self.config.block_range_offset_to;

            // Get block timestamps
            for block_number in from_block..=to_block {
                let block = match provider
                    .get_block_by_number(BlockNumberOrTag::Number(block_number))
                    .await
                {
                    Ok(Some(block)) => block,
                    Ok(None) => return Err(eyre::eyre!("Block {} not found", block_number)),
                    Err(e) => return Err(eyre::eyre!("Failed to get block: {:?}", e)),
                };

                let timestamp = block.header.inner.timestamp;
                block_timestamps.insert(block_number, timestamp);
            }

            // Update filter to include block range
            let range_filter = filter
                .clone()
                .from_block(from_block)
                .to_block(current_block);

            // Get logs
            let logs = match provider.get_logs(&range_filter).await {
                Ok(logs) => logs,
                Err(e) => {
                    error!("Provider failed to get logs: {:?}", e);
                    continue;
                }
            };

            if logs.is_empty() {
                debug!(
                    "No new events found in blocks {} to {}",
                    from_block, current_block
                );
                last_processed_block = current_block;
                continue;
            }

            // Process all logs in parallel
            let mut handles = Vec::new();

            for log in logs {
                let curr_timestamp = *block_timestamps
                    .get(&log.block_number.unwrap_or_default())
                    .unwrap_or(&0);
                let self_clone = self.clone();
                let handle = tokio::spawn(async move {
                    match self_clone.process_event(log, curr_timestamp).await {
                        Ok(event_update) => Some(event_update),
                        Err(e) => {
                            error!("Failed to process event: {:?}", e);
                            None
                        }
                    }
                });
                handles.push(handle);
            }

            // Collect results
            let mut pending_updates = Vec::new();
            for handle in handles {
                if let Ok(Some(event_update)) = handle.await {
                    pending_updates.push(event_update);
                }
            }

            // Write updates to database if any
            if !pending_updates.is_empty() {
                let count = pending_updates.len();
                match self
                    .db
                    .add_new_events(&pending_updates, self.config.is_retarded)
                    .await
                {
                    Ok(_) => info!(
                        "Successfully added batch of {} new events to database",
                        count
                    ),
                    Err(e) => error!("Failed to write events to database: {:?}", e),
                }
            }

            // Update last processed block
            last_processed_block = to_block;
        }
    }

    // Updated helper method to determine which provider to use
    async fn get_active_provider(
        &mut self,
        force_new_primary: bool,
    ) -> Result<(ProviderType, bool)> {
        // If we have a primary provider and don't need to force a new one, check if it's still valid
        if !force_new_primary && self.primary_provider.is_some() {
            let provider = self.primary_provider.as_ref().unwrap();

            // Check if the primary provider is fresh enough
            match provider.get_block_by_number(BlockNumberOrTag::Latest).await {
                Ok(Some(block)) => {
                    // Check block timestamp
                    let block_timestamp: u64 = block.header.inner.timestamp.into();
                    let current_time = chrono::Utc::now().timestamp() as u64;
                    let max_delay = self.config.max_block_delay_secs;

                    if current_time > block_timestamp
                        && (current_time - block_timestamp) > max_delay
                    {
                        warn!(
                            "Primary provider's latest block is too old: block_time={}, current_time={}, diff={}s, max_delay={}s, trying fallback at {}",
                            block_timestamp, current_time, current_time - block_timestamp, max_delay, self.config.fallback_ws_url
                        );
                        // Use fallback and mark primary as invalid
                        self.primary_provider = None;
                        return self.create_fallback_provider().await;
                    }

                    // Primary provider is good, use it
                    debug!("Reusing existing primary provider");
                    return Ok((provider.clone(), false));
                }
                _ => {
                    // Primary provider failed, clear it and try creating a new one
                    warn!("Existing primary provider failed to get latest block, creating new one");
                    self.primary_provider = None;
                }
            }
        }

        // Create a new primary provider
        let primary_ws_url: Url =
            self.config.primary_ws_url.parse().wrap_err_with(|| {
                format!("Invalid primary WSS URL: {}", self.config.primary_ws_url)
            })?;

        debug!("Connecting to primary provider at {}", primary_ws_url);
        let primary_ws = WsConnect::new(primary_ws_url);
        let primary_provider = match ProviderBuilder::new().connect_ws(primary_ws).await {
            Ok(provider) => provider,
            Err(e) => {
                warn!(
                    "Failed to connect to primary WebSocket: {:?}, trying fallback at {}",
                    e, self.config.fallback_ws_url
                );
                return self.create_fallback_provider().await;
            }
        };

        // Check if the primary provider is fresh enough
        let block = match primary_provider
            .get_block_by_number(BlockNumberOrTag::Latest)
            .await
        {
            Ok(Some(block)) => block,
            Ok(None) => {
                warn!(
                    "Primary provider returned no latest block, trying fallback at {}",
                    self.config.fallback_ws_url
                );
                return self.create_fallback_provider().await;
            }
            Err(e) => {
                warn!(
                    "Primary provider failed to get latest block: {:?}, trying fallback at {}",
                    e, self.config.fallback_ws_url
                );
                return self.create_fallback_provider().await;
            }
        };

        // Check if the block is fresh enough
        let block_timestamp: u64 = block.header.inner.timestamp.into();
        let current_time = chrono::Utc::now().timestamp() as u64;
        let max_delay = self.config.max_block_delay_secs;

        if current_time > block_timestamp && (current_time - block_timestamp) > max_delay {
            warn!(
                "Primary provider's latest block is too old: block_time={}, current_time={}, diff={}s, max_delay={}s, trying fallback at {}",
                block_timestamp, current_time, current_time - block_timestamp, max_delay, self.config.fallback_ws_url
            );
            return self.create_fallback_provider().await;
        }

        // Store the new primary provider
        debug!("Using new primary provider");
        self.primary_provider = Some(primary_provider.clone());
        Ok((primary_provider, false))
    }

    // Helper method to create fallback provider
    async fn create_fallback_provider(&self) -> Result<(ProviderType, bool)> {
        let fallback_ws_url: Url = self.config.fallback_ws_url.parse().wrap_err_with(|| {
            format!("Invalid fallback WSS URL: {}", self.config.fallback_ws_url)
        })?;

        info!("Connecting to fallback provider at {}", fallback_ws_url);
        let fallback_ws = WsConnect::new(fallback_ws_url);
        let fallback_provider = ProviderBuilder::new()
            .connect_ws(fallback_ws)
            .await
            .wrap_err("Failed to connect to fallback WebSocket")?;

        Ok((fallback_provider, true))
    }

    async fn process_event(&self, log: Log, timestamp: u64) -> Result<EventUpdate> {
        let tx_hash = log
            .transaction_hash
            .ok_or_else(|| eyre!("No transaction hash"))?;

        // Get the current block number
        let current_block = log.block_number.unwrap_or_default() as i32;

        // Create event update with initial data
        let mut event_update = EventUpdate {
            tx_hash,
            src_chain_id: Some(self.config.chain_id.try_into().unwrap()),
            market: Some(self.config.market),
            received_at_block: Some(current_block),
            received_block_timestamp: Some(timestamp as i64),
            status: EventStatus::Received, // Set to Processed immediately
            received_at: Some(Utc::now()),
            ..Default::default()
        };

        // Process the event based on chain ID
        if self.config.chain_id == LINEA_SEPOLIA_CHAIN_ID || self.config.chain_id == LINEA_CHAIN_ID
        {
            // Process host chain events
            let event = parse_withdraw_on_extension_chain_event(&log);

            // Get the event signature from topic 0
            let event_signature = &log.topics()[0];

            // Determine event type based on the event signature
            let (event_type, target_function) = if *event_signature
                == keccak256(HOST_BORROW_ON_EXTENSION_CHAIN_SIG.as_bytes())
            {
                ("HostBorrow", "outHere")
            } else if *event_signature == keccak256(HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG.as_bytes())
            {
                ("HostWithdraw", "outHere")
            } else {
                info!("Unknown event signature: {}", event_signature);
                return Err(eyre::eyre!("Unknown event signature: {}", event_signature));
            };

            event_update.msg_sender = Some(event.sender);
            event_update.dst_chain_id = Some(event.dst_chain_id);
            event_update.amount = Some(event.amount);
            event_update.target_function = Some(target_function.to_string());
            event_update.event_type = Some(event_type.to_string());
        } else {
            // Process extension chain events
            let event = parse_supplied_event(&log);

            // Validate method selector before processing
            if event.linea_method_selector != MINT_EXTERNAL_SELECTOR
                && event.linea_method_selector != REPAY_EXTERNAL_SELECTOR
            {
                return Err(eyre::eyre!(
                    "Invalid method selector: {}",
                    event.linea_method_selector
                ));
            }

            let function_name = if event.linea_method_selector == MINT_EXTERNAL_SELECTOR {
                "mintExternal"
            } else {
                "repayExternal"
            };

            event_update.msg_sender = Some(event.from);
            event_update.src_chain_id = Some(event.src_chain_id);
            event_update.dst_chain_id = Some(event.dst_chain_id);
            event_update.amount = Some(event.amount);
            event_update.target_function = Some(function_name.to_string());
            event_update.event_type = Some("ExtensionSupply".to_string());
        }

        // Don't update database here - return the event update for batch processing
        Ok(event_update)
    }
}

impl Drop for EventListener {
    fn drop(&mut self) {
        if self.primary_provider.is_some() {}
    }
}
