use alloy::{
    primitives::Address,
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Log},
    transports::http::reqwest::Url,
};
use eyre::{Result, WrapErr};
use futures_util::StreamExt;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};
use alloy::primitives::{ TxHash, U256};
use eyre::eyre;
use hex;
use lazy_static::lazy_static;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::task;
use tokio::time::interval;
use tokio::time::sleep;
use chrono::{DateTime, Utc};

use crate::{
    events::{parse_supplied_event, parse_withdraw_on_extension_chain_event},
};

// Import the chain ID constant from malda_rs
use malda_rs::constants::{
    ETHEREUM_SEPOLIA_CHAIN_ID, L1_BLOCK_ADDRESS_OPSTACK, LINEA_SEPOLIA_CHAIN_ID,
    rpc_url_optimism_sepolia,
};

use crate::constants::ETHEREUM_BLOCK_DELAY;
use crate::create_provider;
use crate::events::{MINT_EXTERNAL_SELECTOR, REPAY_EXTERNAL_SELECTOR};
use crate::types::IL1Block;
use sequencer::database::{Database, EventStatus, EventUpdate};
use serde::{Deserialize, Serialize};
lazy_static! {
    pub static ref ETHEREUM_BLOCK_NUMBER: AtomicU64 = AtomicU64::new(0);
}

#[derive(Debug, Clone)]
pub struct EventConfig {
    pub ws_url: String,
    pub market: Address,
    pub event_signature: String,
    pub chain_id: u64,
}

#[derive(Debug)]
pub struct RawEvent {
    pub log: Log,
    pub market: Address,
    pub chain_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessedEvent {
    HostWithdraw {
        tx_hash: TxHash,
        sender: Address,
        dst_chain_id: u32,
        amount: U256,
        market: Address,
    },
    HostBorrow {
        tx_hash: TxHash,
        sender: Address,
        dst_chain_id: u32,
        amount: U256,
        market: Address,
    },
    ExtensionSupply {
        tx_hash: TxHash,
        from: Address,
        amount: U256,
        src_chain_id: u32,
        dst_chain_id: u32,
        market: Address,
        method_selector: String,
    },
}

pub struct EventListener {
    config: EventConfig,
    db: Database,
}

impl EventListener {
    pub fn new(config: EventConfig, db: Database) -> Self {
        // Start the background task to update Ethereum block number
        task::spawn(async {
            let mut interval = interval(Duration::from_secs(6));
            let provider = create_provider(
                Url::parse(rpc_url_optimism_sepolia()).unwrap(),
                "0xbd0974bec39a17e36ba2a6b4d238ff944bacb481cbed5efcae784d7bf4a2ff80",
            )
            .await
            .map_err(|e| eyre::eyre!("Failed to create provider: {}", e))
            .unwrap();
            let l1_block_contract = IL1Block::new(L1_BLOCK_ADDRESS_OPSTACK, provider);

            loop {
                interval.tick().await;
                match l1_block_contract.number().call().await {
                    Ok(number_return) => {
                        let block_number = number_return._0;
                        ETHEREUM_BLOCK_NUMBER.store(block_number, Ordering::SeqCst);
                    }
                    Err(e) => {
                        error!("Failed to fetch Ethereum block number: {}", e);
                    }
                }
            }
        });

        Self {
            config,
            db,
        }
    }

    pub async fn start(&self) -> Result<()> {
        info!(
            "Starting event listener for market={:?} chain={} event={}",
            self.config.market, self.config.chain_id, self.config.event_signature
        );

        let mut retry_count = 0;
        let max_retries = 5;
        let mut retry_delay = Duration::from_secs(1);

        loop {
            match self.run_event_listener().await {
                Ok(_) => {
                    warn!("Event listener stopped, attempting to reconnect...");
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Ok(());
                    }
                    retry_count += 1;
                    retry_delay *= 2; // Exponential backoff
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
                Err(e) => {
                    error!("Event listener error: {:?}", e);
                    if retry_count >= max_retries {
                        error!("Max retries reached, giving up on reconnection");
                        return Err(e);
                    }
                    retry_count += 1;
                    retry_delay *= 2; // Exponential backoff
                    info!("Waiting {} seconds before reconnection attempt {}", retry_delay.as_secs(), retry_count);
                    sleep(retry_delay).await;
                }
            }
        }
    }

    async fn run_event_listener(&self) -> Result<()> {
        let ws_url: Url = self
            .config
            .ws_url
            .parse()
            .wrap_err_with(|| format!("Invalid WSS URL: {}", self.config.ws_url))?;

        debug!("Connecting to WebSocket at {}", ws_url);
        let ws = WsConnect::new(ws_url);
        let provider = ProviderBuilder::new()
            .on_ws(ws)
            .await
            .wrap_err("Failed to connect to WebSocket")?;

        let filter = Filter::new()
            .event(&self.config.event_signature)
            .address(self.config.market);

        debug!("Subscribing to events with filter: {:?}", filter);
        let sub = provider.subscribe_logs(&filter).await?;
        let mut stream = sub.into_stream();

        info!("Successfully subscribed to events");

        while let Some(log) = stream.next().await {
            debug!(
                "Received event on chain {} for market {:?}",
                self.config.chain_id, self.config.market
            );

            let raw_event = RawEvent {
                log,
                market: self.config.market,
                chain_id: self.config.chain_id,
            };

            let _ = self.process_raw_event(raw_event).await;
        }

        warn!("Event stream ended unexpectedly");
        Ok(())
    }

    async fn process_raw_event(&self, raw_event: RawEvent) -> Result<ProcessedEvent> {
        let tx_hash = raw_event
            .log
            .transaction_hash
            .ok_or_else(|| eyre!("No transaction hash"))?;

        // Initial event receipt
        let update = EventUpdate {
            tx_hash,
            event_type: Some(format!("{:?}", raw_event.log.topics())),
            src_chain_id: Some(raw_event.chain_id.try_into().unwrap()),
            market: Some(raw_event.market),
            status: EventStatus::Received,
            received_at: Some(Utc::now()),
            ..Default::default()
        };

        if let Err(e) = self.db.update_event(update).await {
            error!("Failed to write event to database: {:?}", e);
        }

        // Process the event
        let processed = self.process_event(&raw_event).await?;

        // Extract fields based on ProcessedEvent variant
        let (dst_chain_id, msg_sender, amount, target_function) = match &processed {
            ProcessedEvent::HostWithdraw {
                dst_chain_id,
                sender,
                amount,
                ..
            } => (
                Some(*dst_chain_id),
                Some(*sender),
                Some(*amount),
                Some("outHere".to_string()),
            ),
            ProcessedEvent::HostBorrow {
                dst_chain_id,
                sender,
                amount,
                ..
            } => (
                Some(*dst_chain_id),
                Some(*sender),
                Some(*amount),
                Some("outHere".to_string()),
            ),
            ProcessedEvent::ExtensionSupply {
                dst_chain_id,
                from,
                amount,
                method_selector,
                ..
            } => {
                let function = if method_selector == &MINT_EXTERNAL_SELECTOR {
                    "mintExternal"
                } else if method_selector == &REPAY_EXTERNAL_SELECTOR {
                    "repayExternal"
                } else {
                    method_selector
                };
                (
                    Some(*dst_chain_id),
                    Some(*from),
                    Some(*amount),
                    Some(function.to_string()),
                )
            }
        };

        // Update with processed information
        let update = EventUpdate {
            tx_hash,
            dst_chain_id: dst_chain_id,
            msg_sender,
            amount,
            target_function,
            market: Some(raw_event.market),
            status: EventStatus::Processed,
            processed_at: Some(Utc::now()),
            ..Default::default()
        };

        if let Err(e) = self.db.update_event(update).await {
            error!("Failed to update processed status: {:?}", e);
        }

        Ok(processed)
    }

    async fn process_event(&self, raw_event: &RawEvent) -> Result<ProcessedEvent> {
        // Log when event is received
        info!(
            "Processing event for market: {:?}, chain_id: {}",
            raw_event.market, raw_event.chain_id
        );

        let chain_id = raw_event.chain_id;
        let market = raw_event.market;
        let log = &raw_event.log;
        let tx_hash = log.transaction_hash.expect("Log should have tx hash");
        debug!("Processing event with tx_hash: {}", hex::encode(tx_hash.0));

        // Add delay for ETH Sepolia events
        if chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
            let event_block = log.block_number.expect("Log should have block number");
            while event_block > ETHEREUM_BLOCK_NUMBER.load(Ordering::SeqCst) {
                debug!("ETH Sepolia event block {} not yet reached, current block {}, waiting {} seconds", 
                    event_block, 
                    ETHEREUM_BLOCK_NUMBER.load(Ordering::SeqCst), 
                    ETHEREUM_BLOCK_DELAY
                );
                sleep(Duration::from_secs(ETHEREUM_BLOCK_DELAY)).await;
            }
        }

        let processed = if chain_id == LINEA_SEPOLIA_CHAIN_ID {
            // Process host chain events
            let event = parse_withdraw_on_extension_chain_event(log);

            // Update database with more details from the event
            if let Err(e) = self
                .db
                .update_event(EventUpdate {
                    tx_hash,
                    msg_sender: Some(event.sender),
                    dst_chain_id: Some(event.dst_chain_id),
                    amount: Some(event.amount),
                    target_function: Some("outHere".to_string()),
                    market: Some(market),
                    processed_at: Some(Utc::now()),
                    ..Default::default()
                })
                .await
            {
                error!("Failed to update event details: {:?}", e);
            }

            ProcessedEvent::HostWithdraw {
                tx_hash,
                sender: event.sender,
                dst_chain_id: event.dst_chain_id,
                amount: event.amount,
                market,
            }
        } else {
            // Process extension chain events
            let event = parse_supplied_event(log);

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

            // Update database with more details from the event
            if let Err(e) = self
                .db
                .update_event(EventUpdate {
                    tx_hash,
                    msg_sender: Some(event.from),
                    src_chain_id: Some(event.src_chain_id),
                    dst_chain_id: Some(event.dst_chain_id),
                    amount: Some(event.amount),
                    target_function: Some(function_name.to_string()),
                    market: Some(market),
                    processed_at: Some(Utc::now()),
                    ..Default::default()
                })
                .await
            {
                error!("Failed to update event details: {:?}", e);
            }

            ProcessedEvent::ExtensionSupply {
                tx_hash,
                from: event.from,
                amount: event.amount,
                src_chain_id: event.src_chain_id,
                dst_chain_id: event.dst_chain_id,
                market,
                method_selector: event.linea_method_selector,
            }
        };

        // Log when event is processed
        let event_type = match &processed {
            ProcessedEvent::HostBorrow { .. } => "HostBorrow",
            ProcessedEvent::HostWithdraw { .. } => "HostWithdraw",
            ProcessedEvent::ExtensionSupply { .. } => "ExtensionSupply",
        };

        self.db
            .update_event(EventUpdate {
                tx_hash: tx_hash,
                status: EventStatus::Processed,
                event_type: Some(event_type.to_string()),
                processed_at: Some(Utc::now()),
                ..Default::default()
            })
            .await?;

        Ok(processed)
    }
}
