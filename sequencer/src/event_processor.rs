use alloy::{
    primitives::{Address, U256, TxHash},
};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};
use tokio::task;
use futures::future::join_all;
use tokio::time::sleep;
use std::time::Duration;
use hex;

use crate::{
    event_listener::RawEvent,
    events::{parse_withdraw_on_extension_chain_event, parse_supplied_event},
};

// Import the chain ID constant from malda_rs
use malda_rs::constants::{LINEA_SEPOLIA_CHAIN_ID, ETHEREUM_SEPOLIA_CHAIN_ID};

use crate::constants::ETHEREUM_BLOCK_DELAY;
use sequencer::logger::{PipelineLogger, PipelineStep};

#[derive(Debug)]
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
    logger: PipelineLogger,
}

impl EventProcessor {
    pub fn new(
        event_receiver: mpsc::Receiver<RawEvent>,
        processed_sender: mpsc::Sender<ProcessedEvent>,
        logger: PipelineLogger,
    ) -> Self {
        Self {
            event_receiver,
            processed_sender,
            logger,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting event processor");
        
        let mut processing_tasks = Vec::new();
        const MAX_CONCURRENT_TASKS: usize = 10;

        while let Some(raw_event) = self.event_receiver.recv().await {
            debug!(
                "Processing event from chain {} for market {:?}",
                raw_event.chain_id, raw_event.market
            );

            let processed_sender = self.processed_sender.clone();
            let logger = self.logger.clone();
            
            // Spawn a new task for processing this event
            let task = task::spawn(async move {
                match Self::process_event(raw_event, &logger).await {
                    Ok(processed) => {
                        // Log the processed event
                        match &processed {
                            ProcessedEvent::HostWithdraw { tx_hash: _, sender, dst_chain_id, amount, market } => {
                                info!(
                                    "Processed host withdraw: sender={:?} dst_chain={} amount={} market={:?}",
                                    sender, dst_chain_id, amount, market
                                );
                            }
                            ProcessedEvent::HostBorrow { tx_hash: _, sender, dst_chain_id, amount, market } => {
                                info!(
                                    "Processed host borrow: sender={:?} dst_chain={} amount={} market={:?}",
                                    sender, dst_chain_id, amount, market
                                );
                            }
                            ProcessedEvent::ExtensionSupply { tx_hash: _, from, amount, src_chain_id, dst_chain_id, market, method_selector } => {
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

            if processing_tasks.len() >= MAX_CONCURRENT_TASKS {
                join_all(processing_tasks).await;
                processing_tasks = Vec::new();
            }
        }

        if !processing_tasks.is_empty() {
            join_all(processing_tasks).await;
        }

        warn!("Event processor channel closed");
        Ok(())
    }

    async fn process_event(raw_event: RawEvent, logger: &PipelineLogger) -> Result<ProcessedEvent> {
        let chain_id = raw_event.chain_id;
        let market = raw_event.market;
        let log = raw_event.log;
        let tx_hash = log.transaction_hash.expect("Log should have tx hash");
        debug!("Processing event with tx_hash: {}", hex::encode(tx_hash.0));

        // Add delay for ETH Sepolia events
        if chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
            debug!("ETH Sepolia event detected, waiting {} seconds", ETHEREUM_BLOCK_DELAY);
            sleep(Duration::from_secs(ETHEREUM_BLOCK_DELAY)).await;
        }

        let processed = if chain_id == LINEA_SEPOLIA_CHAIN_ID {
            // Process host chain events
            let event = parse_withdraw_on_extension_chain_event(&log);
            let result = ProcessedEvent::HostWithdraw {
                tx_hash,
                sender: event.sender,
                dst_chain_id: event.dst_chain_id,
                amount: event.amount,
                market,
            };

            // Log the processed event
            logger.log_step(
                tx_hash,
                PipelineStep::EventProcessed {
                    chain_id: chain_id as u32,
                    dst_chain_id: event.dst_chain_id,
                    market,
                    event_type: "HostWithdraw".to_string(),
                    amount: event.amount,
                }
            ).await?;

            result
        } else {
            // Process extension chain events
            let event = parse_supplied_event(&log);
            let result = ProcessedEvent::ExtensionSupply {
                tx_hash,
                from: event.from,
                amount: event.amount,
                src_chain_id: event.src_chain_id,
                dst_chain_id: event.dst_chain_id,
                market,
                method_selector: event.linea_method_selector,
            };

            // Log the processed event
            logger.log_step(
                tx_hash,
                PipelineStep::EventProcessed {
                    chain_id: chain_id as u32,
                    dst_chain_id: event.dst_chain_id,
                    market,
                    event_type: "ExtensionSupply".to_string(),
                    amount: event.amount,
                }
            ).await?;

            result
        };

        Ok(processed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::sync::mpsc;
    use std::path::PathBuf;
    use sequencer::logger::PipelineLogger;

    #[tokio::test]
    async fn test_event_processor() -> Result<()> {
        let (_raw_tx, raw_rx) = mpsc::channel(100);
        let (processed_tx, _processed_rx) = mpsc::channel(100);
        let logger = PipelineLogger::new(PathBuf::from("test_pipeline.log")).await?;
        
        let _processor = EventProcessor::new(raw_rx, processed_tx, logger);
        Ok(())
    }
} 