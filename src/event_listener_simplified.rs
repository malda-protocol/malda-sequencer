use alloy::{
    eips::BlockNumberOrTag,
    network::Ethereum,
    primitives::{keccak256, Address},
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Log},
    transports::http::reqwest::Url,
};
use chrono::Utc;
use eyre::{eyre, Result, WrapErr};
use std::collections::HashMap;
use std::time::Duration;
use tokio::time::interval;
use tracing::{error, info, warn};

use crate::events::{parse_supplied_event, parse_withdraw_on_extension_chain_event};
use crate::{HOST_BORROW_ON_EXTENSION_CHAIN_SIG, HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG};
use malda_rs::constants::*;
use crate::events::{MINT_EXTERNAL_SELECTOR, REPAY_EXTERNAL_SELECTOR};
use sequencer::database::{Database, EventStatus, EventUpdate};

// Simplified provider type
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

#[derive(Debug, Clone)]
pub struct EventConfig {
    pub primary_ws_url: String,
    pub fallback_ws_url: String,
    pub chain_id: u64,
    pub max_retries: u32,
    pub retry_delay_secs: u64,
    pub poll_interval_secs: u64,
    pub max_block_delay_secs: u64,
    pub block_range_offset_from: u64,
    pub block_range_offset_to: u64,
    pub is_retarded: bool,
    // Multiple markets and events
    pub markets: Vec<Address>,
    pub events: Vec<String>,
}

pub struct EventListener {
    configs: Vec<EventConfig>,
    db: Database,
}

impl EventListener {
    pub async fn new(configs: Vec<EventConfig>, db: Database) -> Result<()> {
        info!("Starting event listener for {} chains in parallel", configs.len());

        // Create tasks for each chain
        let mut tasks = Vec::new();
        
        for config in configs {
            let db_clone = db.clone();
            let task = tokio::spawn(async move {
                Self::process_chain(config, db_clone).await
            });
            tasks.push(task);
        }

        // Wait for all tasks to complete
        futures::future::join_all(tasks).await;
        
        Ok(())
    }

    async fn process_chain(config: EventConfig, db: Database) -> Result<()> {
        info!("Starting chain {} with {} markets and {} events", 
              config.chain_id, config.markets.len(), config.events.len());

        // Create filter for this chain
        let filter = Filter::new()
            .address(config.markets.clone())
            .events(&config.events);

        let mut last_processed_block = 0u64;
        let mut interval = interval(Duration::from_secs(config.poll_interval_secs));
        
        // Cache the provider to avoid creating new connections
        let mut cached_provider: Option<ProviderType> = None;

        info!("Started polling for chain {}", config.chain_id);

        loop {
            interval.tick().await;

            // Get provider for this chain (reuse cached if available)
            let provider = if let Some(provider) = &cached_provider {
                // Check if cached provider is still fresh
                if Self::is_provider_fresh(provider, config.max_block_delay_secs).await {
                    provider.clone()
                } else {
                    // Provider is stale, get a new one
                    match Self::get_provider(&config).await {
                        Ok(new_provider) => {
                            cached_provider = Some(new_provider.clone());
                            new_provider
                        }
                        Err(e) => {
                            error!("Failed to get provider for chain {}: {:?}", config.chain_id, e);
                            continue;
                        }
                    }
                }
            } else {
                // No cached provider, get a new one
                match Self::get_provider(&config).await {
                    Ok(new_provider) => {
                        cached_provider = Some(new_provider.clone());
                        new_provider
                    }
                    Err(e) => {
                        error!("Failed to get provider for chain {}: {:?}", config.chain_id, e);
                        continue;
                    }
                }
            };

            // Get current block for this chain
            let current_block = match provider.get_block_number().await {
                Ok(block) => block,
                Err(e) => {
                    error!("Failed to get block number for chain {}: {:?}", config.chain_id, e);
                    // Clear cached provider on error
                    cached_provider = None;
                    continue;
                }
            };

            if last_processed_block >= current_block {
                continue;
            }

            let from_block = if last_processed_block > 0 {
                last_processed_block + 1
            } else {
                current_block - config.block_range_offset_from
            };

            let to_block = current_block - config.block_range_offset_to;

            // Get block timestamps for this chain
            let mut block_timestamps = HashMap::new();
            for block_number in from_block..=to_block {
                let block = match provider
                    .get_block_by_number(BlockNumberOrTag::Number(block_number))
                    .await
                {
                    Ok(Some(block)) => block,
                    Ok(None) => {
                        error!("Block {} not found on chain {}", block_number, config.chain_id);
                        continue;
                    }
                    Err(e) => {
                        error!("Failed to get block {} on chain {}: {:?}", block_number, config.chain_id, e);
                        continue;
                    }
                };
                block_timestamps.insert(block_number, block.header.inner.timestamp);
            }

            // Get logs for this chain
            let logs = match provider.get_logs(&filter
                .clone()
                .from_block(from_block)
                .to_block(current_block))
                .await
            {
                Ok(logs) => logs,
                Err(e) => {
                    error!("Failed to get logs for chain {}: {:?}", config.chain_id, e);
                    continue;
                }
            };

            if logs.is_empty() {
                last_processed_block = current_block;
                continue;
            }

            // Process events for this chain
            let mut event_updates = Vec::new();
            for log in logs {
                let timestamp = *block_timestamps
                    .get(&log.block_number.unwrap_or_default())
                    .unwrap_or(&0);
                
                if let Ok(event_update) = Self::process_event(&config, log, timestamp).await {
                    event_updates.push(event_update);
                }
            }

            // Save to database
            if !event_updates.is_empty() {
                if let Err(e) = db.add_new_events(&event_updates, config.is_retarded).await {
                    error!("Failed to save events to database for chain {}: {:?}", config.chain_id, e);
                } else {
                    info!("Added {} new events to database for chain {}", event_updates.len(), config.chain_id);
                }
            }

            last_processed_block = to_block;
        }
    }

    async fn get_provider(config: &EventConfig) -> Result<ProviderType> {
        // Try primary first
        if let Ok(provider) = Self::connect_provider(&config.primary_ws_url).await {
            if Self::is_provider_fresh(&provider, config.max_block_delay_secs).await {
                return Ok(provider);
            }
        }

        // Try fallback
        let provider = Self::connect_provider(&config.fallback_ws_url).await?;
        if Self::is_provider_fresh(&provider, config.max_block_delay_secs).await {
            return Ok(provider);
        }

        Err(eyre!("Both primary and fallback providers are unavailable for chain {}", config.chain_id))
    }

    async fn connect_provider(url: &str) -> Result<ProviderType> {
        let ws_url: Url = url.parse()
            .wrap_err_with(|| format!("Invalid WSS URL: {}", url))?;
        
        let ws_connect = WsConnect::new(ws_url);
        ProviderBuilder::new()
            .connect_ws(ws_connect)
            .await
            .wrap_err("Failed to connect to WebSocket")
    }

    async fn is_provider_fresh(provider: &ProviderType, max_delay: u64) -> bool {
        match provider.get_block_by_number(BlockNumberOrTag::Latest).await {
            Ok(Some(block)) => {
                let block_timestamp: u64 = block.header.inner.timestamp.into();
                let current_time = chrono::Utc::now().timestamp() as u64;
                
                current_time <= block_timestamp || 
                (current_time - block_timestamp) <= max_delay
            }
            _ => false
        }
    }

    async fn process_event(config: &EventConfig, log: Log, timestamp: u64) -> Result<EventUpdate> {
        let tx_hash = log.transaction_hash
            .ok_or_else(|| eyre!("No transaction hash"))?;

        let current_block = log.block_number.unwrap_or_default() as i32;
        let market = log.address(); // Get market from log address

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

        // Process based on chain ID
        if config.chain_id == LINEA_SEPOLIA_CHAIN_ID || config.chain_id == LINEA_CHAIN_ID {
            let event = parse_withdraw_on_extension_chain_event(&log);
            let event_signature = &log.topics()[0];

            let (event_type, target_function) = if *event_signature == keccak256(HOST_BORROW_ON_EXTENSION_CHAIN_SIG.as_bytes()) {
                ("HostBorrow", "outHere")
            } else if *event_signature == keccak256(HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG.as_bytes()) {
                ("HostWithdraw", "outHere")
            } else {
                return Err(eyre!("Unknown event signature: {}", event_signature));
            };

            event_update.msg_sender = Some(event.sender);
            event_update.dst_chain_id = Some(event.dst_chain_id);
            event_update.amount = Some(event.amount);
            event_update.target_function = Some(target_function.to_string());
            event_update.event_type = Some(event_type.to_string());
        } else {
            let event = parse_supplied_event(&log);

            if event.linea_method_selector != MINT_EXTERNAL_SELECTOR
                && event.linea_method_selector != REPAY_EXTERNAL_SELECTOR {
                return Err(eyre!("Invalid method selector: {}", event.linea_method_selector));
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

        Ok(event_update)
    }
} 