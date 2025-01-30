use alloy::{
    primitives::{Address, U256},
};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};

use crate::{
    event_listener::RawEvent,
    events::{parse_withdraw_on_extension_chain_event, parse_supplied_event},
};

// Import the chain ID constant from malda_rs
use malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID;

#[derive(Debug)]
pub enum ProcessedEvent {
    HostWithdraw {
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
        
        while let Some(raw_event) = self.event_receiver.recv().await {
            debug!(
                "Processing event from chain {} for market {:?}",
                raw_event.chain_id, raw_event.market
            );

            match self.process_event(raw_event).await {
                Ok(processed) => {
                    match &processed {
                        ProcessedEvent::HostWithdraw { sender, dst_chain_id, amount, market } => {
                            info!(
                                "Processed host withdraw: sender={:?} dst_chain={} amount={} market={:?}",
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
                            ProcessedEvent::ExtensionSupply { .. } => "ExtensionSupply",
                        }
                    );

                    if let Err(e) = self.processed_sender.send(processed).await {
                        error!("Failed to send to proof generator: {}", e);
                    } else {
                        info!("Successfully sent event to proof generator");
                    }
                }
                Err(e) => {
                    error!("Failed to process event: {}", e);
                }
            }
        }

        warn!("Event processor channel closed");
        Ok(())
    }

    async fn process_event(&self, raw_event: RawEvent) -> Result<ProcessedEvent> {
        let chain_id = raw_event.chain_id;
        let market = raw_event.market;
        let log = raw_event.log;

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