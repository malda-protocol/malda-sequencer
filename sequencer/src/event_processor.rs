use alloy::{
    primitives::{Address, U256, TxHash},
};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::{info, error, debug};
use tokio::task;
use tokio::time::sleep;
use std::time::Duration;
use hex;
use std::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use tokio::time::interval;
use url::Url;
use eyre::eyre;

use crate::{
    event_listener::RawEvent,
    events::{parse_withdraw_on_extension_chain_event, parse_supplied_event},
};

// Import the chain ID constant from malda_rs
use malda_rs::constants::{LINEA_SEPOLIA_CHAIN_ID, ETHEREUM_SEPOLIA_CHAIN_ID, L1_BLOCK_ADDRESS_OPTIMISM, RPC_URL_OPTIMISM_SEPOLIA};

use crate::constants::ETHEREUM_BLOCK_DELAY;
use crate::events::{MINT_EXTERNAL_SELECTOR, REPAY_EXTERNAL_SELECTOR};
use crate::types::IL1Block;
use crate::create_provider;
use serde::{Serialize, Deserialize};
use sequencer::database::{Database, EventStatus, EventUpdate};
lazy_static! {
    pub static ref ETHEREUM_BLOCK_NUMBER: AtomicU64 = AtomicU64::new(0);
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

pub struct EventProcessor {
    event_receiver: mpsc::Receiver<RawEvent>,
    processed_sender: mpsc::Sender<ProcessedEvent>,
    db: Database,
}

impl EventProcessor {
    pub fn new(
        event_receiver: mpsc::Receiver<RawEvent>,
        processed_sender: mpsc::Sender<ProcessedEvent>,
        db: Database,
    ) -> Self {
        // Start the background task to update Ethereum block number
        task::spawn(async {
            let mut interval = interval(Duration::from_secs(6));
            let provider = create_provider(Url::parse(RPC_URL_OPTIMISM_SEPOLIA).unwrap(), "0xbd0974bec39a17e36ba2a6b4d238ff944bacb481cbed5efcae784d7bf4a2ff80").await.map_err(|e| eyre::eyre!("Failed to create provider: {}", e)).unwrap();
            let l1_block_contract = IL1Block::new(L1_BLOCK_ADDRESS_OPTIMISM, provider);

            loop {
                interval.tick().await;
                match l1_block_contract.number().call().await {
                    Ok(number_return) => {
                        let block_number = number_return._0;
                        ETHEREUM_BLOCK_NUMBER.store(block_number, Ordering::SeqCst);
                        // debug!("Updated Ethereum block number to {}", block_number);
                    },
                    Err(e) => {
                        error!("Failed to fetch Ethereum block number: {}", e);
                    }
                }
            }
        });

        Self {
            event_receiver,
            processed_sender,
            db,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting event processor...");
        
        while let Some(raw_event) = self.event_receiver.recv().await {
            match self.process_raw_event(raw_event).await {
                Ok(processed) => {
                    if let Err(e) = self.processed_sender.send(processed).await {
                        error!("Failed to send processed event: {:?}", e);
                    }
                }
                Err(e) => {
                    error!("Failed to process event: {:?}", e);
                }
            }
        }

        Ok(())
    }

    async fn process_raw_event(&self, raw_event: RawEvent) -> Result<ProcessedEvent> {
        let tx_hash = raw_event.log.transaction_hash
            .ok_or_else(|| eyre!("No transaction hash"))?;

        // Initial event receipt
        let update = EventUpdate {
            tx_hash,
            event_type: Some(raw_event.market.to_string()),
            src_chain_id: Some(raw_event.chain_id.try_into().unwrap()),
            status: EventStatus::Received,
            ..Default::default()
        };

        if let Err(e) = self.db.update_event(update).await {
            error!("Failed to write event to database: {:?}", e);
        }

        // Process the event
        let processed = self.process_event(raw_event).await?;

        // Extract fields based on ProcessedEvent variant
        let (dst_chain_id, msg_sender, amount) = match &processed {
            ProcessedEvent::HostWithdraw { dst_chain_id, sender, amount, .. } => {
                (Some(*dst_chain_id), Some(*sender), Some(*amount))
            },
            ProcessedEvent::HostBorrow { dst_chain_id, sender, amount, .. } => {
                (Some(*dst_chain_id), Some(*sender), Some(*amount))
            },
            ProcessedEvent::ExtensionSupply { dst_chain_id, from, amount, .. } => {
                (Some(*dst_chain_id), Some(*from), Some(*amount))
            },
        };

        // Update with processed information
        let update = EventUpdate {
            tx_hash,
            dst_chain_id: dst_chain_id.map(|id| id.try_into().unwrap()),
            msg_sender,
            amount,
            status: EventStatus::Processed,
            ..Default::default()
        };

        if let Err(e) = self.db.update_event(update).await {
            error!("Failed to update processed status: {:?}", e);
        }

        Ok(processed)
    }

    async fn process_event(&self, raw_event: RawEvent) -> Result<ProcessedEvent> {
        // Log when event is received
        info!("Attempting to write received event to database: {:?}", raw_event.log.transaction_hash);
        
        if let Some(tx_hash) = &raw_event.log.transaction_hash {
            if let Err(e) = self.db.update_event(EventUpdate {
                tx_hash: *tx_hash,
                status: EventStatus::Received,
                ..Default::default()
            }).await {
                error!("Failed to write received event to database: {:?}", e);
            } else {
                info!("Successfully wrote received event to database");
            }
        }

        let chain_id = raw_event.chain_id;
        let market = raw_event.market;
        let log = raw_event.log;
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
            let event = parse_withdraw_on_extension_chain_event(&log);
            ProcessedEvent::HostWithdraw {
                tx_hash,
                sender: event.sender,
                dst_chain_id: event.dst_chain_id,
                amount: event.amount,
                market,
            }
        } else {
            // Process extension chain events
            let event = parse_supplied_event(&log);
            
            // Validate method selector before processing
            if event.linea_method_selector != MINT_EXTERNAL_SELECTOR && 
               event.linea_method_selector != REPAY_EXTERNAL_SELECTOR {
                return Err(eyre::eyre!("Invalid method selector: {}", event.linea_method_selector));
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
        if let Some(tx_hash) = log.transaction_hash {
            if let Err(e) = self.db.update_event(EventUpdate {
                tx_hash: tx_hash,
                status: EventStatus::Processed,
                ..Default::default()
            }).await {
                error!("Failed to update processed status in database: {:?}", e);
            } else {
                info!("Successfully updated processed status in database");
            }
        }

        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_event_processor() -> Result<()> {
        let (_raw_tx, raw_rx) = mpsc::channel(100);
        let (processed_tx, _processed_rx) = mpsc::channel(100);
        let db = Database::new();
        
        let _processor = EventProcessor::new(raw_rx, processed_tx, db);
        Ok(())
    }
} 