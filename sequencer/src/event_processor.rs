use alloy::{
    primitives::{Address, U256},
};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};
use tokio::task;
use futures::future::join_all;
use tokio::time::sleep;
use std::time::Duration;

use crate::{
    event_listener::RawEvent,
    events::{parse_withdraw_on_extension_chain_event, parse_supplied_event},
};

// Import the chain ID constant from malda_rs
use malda_rs::constants::{LINEA_SEPOLIA_CHAIN_ID, ETHEREUM_SEPOLIA_CHAIN_ID};

use crate::constants::ETHEREUM_BLOCK_DELAY;

#[derive(Debug)]
pub enum ProcessedEvent {
    HostWithdraw {
        sender: Address,
        dst_chain_id: u32,
        amount: U256,
        market: Address,
    },
    HostBorrow {
        sender: Address,
        dst_chain_id: u32,
        amount: U256,
        market: Address,
    },
    ExtensionSupply {
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
}

impl EventProcessor {
    pub fn new(
        event_receiver: mpsc::Receiver<RawEvent>,
        processed_sender: mpsc::Sender<ProcessedEvent>,
    ) -> Self {
        Self {
            event_receiver,
            processed_sender,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting event processor");
        
        // Create a buffer to store processing tasks
        let mut processing_tasks = Vec::new();
        const MAX_CONCURRENT_TASKS: usize = 10;

        while let Some(raw_event) = self.event_receiver.recv().await {
            debug!(
                "Processing event from chain {} for market {:?}",
                raw_event.chain_id, raw_event.market
            );

            let processed_sender = self.processed_sender.clone();
            
            // Spawn a new task for processing this event
            let task = task::spawn(async move {
                match Self::process_event(raw_event).await {
                    Ok(processed) => {
                        // Log the processed event
                        match &processed {
                            ProcessedEvent::HostWithdraw { sender, dst_chain_id, amount, market } => {
                                info!(
                                    "Processed host withdraw: sender={:?} dst_chain={} amount={} market={:?}",
                                    sender, dst_chain_id, amount, market
                                );
                            }
                            ProcessedEvent::HostBorrow { sender, dst_chain_id, amount, market } => {
                                info!(
                                    "Processed host borrow: sender={:?} dst_chain={} amount={} market={:?}",
                                    sender, dst_chain_id, amount, market
                                );
                            }
                            ProcessedEvent::ExtensionSupply { from, amount, src_chain_id, dst_chain_id, market, method_selector } => {
                                info!(
                                    "Processed extension supply: from={:?} amount={} src_chain={} dst_chain={} market={:?} method={}",
                                    from, amount, src_chain_id, dst_chain_id, market, method_selector
                                );
                            }
                        }

                        info!(
                            "Attempting to send processed event to proof generator: type={}", 
                            match &processed {
                                ProcessedEvent::HostWithdraw { .. } => "HostWithdraw",
                                ProcessedEvent::HostBorrow { .. } => "HostBorrow",
                                ProcessedEvent::ExtensionSupply { .. } => "ExtensionSupply",
                            }
                        );

                        if let Err(e) = processed_sender.send(processed).await {
                            error!("Failed to send to proof generator: {}", e);
                        } else {
                            info!("Successfully sent event to proof generator");
                        }
                    }
                    Err(e) => {
                        error!("Failed to process event: {}", e);
                    }
                }
            });

            processing_tasks.push(task);

            // If we have too many pending tasks, wait for some to complete
            if processing_tasks.len() >= MAX_CONCURRENT_TASKS {
                // Wait for all current tasks to complete before continuing
                join_all(processing_tasks).await;
                processing_tasks = Vec::new();
            }
        }

        // Wait for any remaining tasks to complete
        if !processing_tasks.is_empty() {
            join_all(processing_tasks).await;
        }

        warn!("Event processor channel closed");
        Ok(())
    }

    async fn process_event(raw_event: RawEvent) -> Result<ProcessedEvent> {
        let chain_id = raw_event.chain_id;
        let market = raw_event.market;
        let log = raw_event.log;

        // Add delay for ETH Sepolia events
        if chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
            debug!("ETH Sepolia event detected, waiting {} seconds", ETHEREUM_BLOCK_DELAY);
            sleep(Duration::from_secs(ETHEREUM_BLOCK_DELAY)).await;
        }

        let processed = if chain_id == LINEA_SEPOLIA_CHAIN_ID {
            // Process host chain events
            let event = parse_withdraw_on_extension_chain_event(&log);
            ProcessedEvent::HostWithdraw {
                sender: event.sender,
                dst_chain_id: event.dst_chain_id,
                amount: event.amount,
                market,
            }
        } else {
            // Process extension chain events
            let event = parse_supplied_event(&log);
            ProcessedEvent::ExtensionSupply {
                from: event.from,
                amount: event.amount,
                src_chain_id: event.src_chain_id,
                dst_chain_id: event.dst_chain_id,
                market,
                method_selector: event.linea_method_selector,
            }
        };

        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn test_event_processor() {
        let (raw_tx, raw_rx) = mpsc::channel(100);
        let (processed_tx, _processed_rx) = mpsc::channel(100);
        
        let _processor = EventProcessor::new(raw_rx, processed_tx);
    }
} 