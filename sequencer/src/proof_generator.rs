use alloy::primitives::{Address, Bytes, U256};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};
use std::time::Duration;

use crate::event_processor::ProcessedEvent;
use malda_rs::viewcalls::get_proof_data_prove;

#[derive(Debug)]
pub struct ProofReadyEvent {
    pub market: Address,
    pub journal: Bytes,
    pub seal: Bytes,
    pub amount: Vec<U256>,
    pub receiver: Address,
    pub method: String,
    pub dst_chain_id: u32,
}

pub struct ProofGenerator {
    event_receiver: mpsc::Receiver<ProcessedEvent>,
    proof_sender: mpsc::Sender<ProofReadyEvent>,
    max_retries: u32,
    retry_delay: Duration,
}

impl ProofGenerator {
    pub fn new(
        event_receiver: mpsc::Receiver<ProcessedEvent>,
        proof_sender: mpsc::Sender<ProofReadyEvent>,
        max_retries: u32,
        retry_delay: Duration,
    ) -> Self {
        Self {
            event_receiver,
            proof_sender,
            max_retries,
            retry_delay,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting proof generator, waiting for events...");
        
        while let Some(event) = self.event_receiver.recv().await {
            info!(
                "Proof generator received event: type={}", 
                match &event {
                    ProcessedEvent::HostWithdraw { .. } => "HostWithdraw",
                    ProcessedEvent::HostBorrow { .. } => "HostBorrow",
                    ProcessedEvent::ExtensionSupply { .. } => "ExtensionSupply",
                }
            );

            // Get the relevant fields based on event type
            let (market, chain_id, event_type) = match &event {
                ProcessedEvent::HostWithdraw { market, dst_chain_id, .. } => {
                    (market, dst_chain_id, "HostWithdraw")
                },
                ProcessedEvent::HostBorrow { market, dst_chain_id, .. } => {
                    (market, dst_chain_id, "HostBorrow")
                },
                ProcessedEvent::ExtensionSupply { market, dst_chain_id, .. } => {
                    (market, dst_chain_id, "ExtensionSupply")
                }
            };

            debug!(
                "Received event for proof generation: market={:?}, type={}, chain={}",
                market, event_type, chain_id
            );

            match self.process_event(event).await {
                Ok(proof_event) => {
                    info!(
                        "Successfully generated proof for market={:?}, method={}, chain={}",
                        proof_event.market, proof_event.method, proof_event.dst_chain_id
                    );
                    debug!("Proof details - journal size: {}, seal size: {}", 
                        proof_event.journal.len(), proof_event.seal.len()
                    );

                    if let Err(e) = self.proof_sender.send(proof_event).await {
                        error!("Failed to send proof ready event: {}", e);
                    } else {
                        debug!("Sent proof to transaction manager");
                    }
                }
                Err(e) => {
                    error!("Failed to generate proof: {}", e);
                }
            }
        }

        warn!("Proof generator channel closed");
        Ok(())
    }

    async fn process_event(&self, event: ProcessedEvent) -> Result<ProofReadyEvent> {
        match &event {
            ProcessedEvent::HostWithdraw { .. } => {
                info!("Starting proof generation for HostWithdraw event");
            },
            ProcessedEvent::HostBorrow { .. } => {
                info!("Starting proof generation for HostBorrow event");
            },
            ProcessedEvent::ExtensionSupply { .. } => {
                info!("Starting proof generation for ExtensionSupply event");
            }
        }

        let result = match event {
            ProcessedEvent::HostWithdraw { sender, dst_chain_id, amount, market } => {
                info!("Preparing proof data for HostWithdraw");
                let users = vec![vec![sender]];
                let markets = vec![vec![market]];
                let dst_chain_ids = vec![vec![dst_chain_id as u64]];
                let src_chain_ids = vec![malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID];

                debug!(
                    "Calling generate_proof_with_retry with: users={:?}, markets={:?}, dst_chains={:?}, src_chains={:?}",
                    users, markets, dst_chain_ids, src_chain_ids
                );

                let (journal, seal) = self.generate_proof_with_retry(
                    users,
                    markets,
                    dst_chain_ids,
                    src_chain_ids,
                ).await?;

                Ok(ProofReadyEvent {
                    market,
                    journal,
                    seal,
                    amount: vec![amount],
                    receiver: Address::ZERO,
                    method: "outHere".to_string(),
                    dst_chain_id,
                })
            },
            ProcessedEvent::HostBorrow { sender, dst_chain_id, amount, market } => {
                info!("Preparing proof data for HostBorrow");
                let users = vec![vec![sender]];
                let markets = vec![vec![market]];
                let dst_chain_ids = vec![vec![dst_chain_id as u64]];
                let src_chain_ids = vec![malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID];

                debug!(
                    "Calling generate_proof_with_retry with: users={:?}, markets={:?}, dst_chains={:?}, src_chains={:?}",
                    users, markets, dst_chain_ids, src_chain_ids
                );

                let (journal, seal) = self.generate_proof_with_retry(
                    users,
                    markets,
                    dst_chain_ids,
                    src_chain_ids,
                ).await?;

                Ok(ProofReadyEvent {
                    market,
                    journal,
                    seal,
                    amount: vec![amount],
                    receiver: Address::ZERO,
                    method: "outHere".to_string(),
                    dst_chain_id,
                })
            },
            ProcessedEvent::ExtensionSupply { from, amount, src_chain_id, dst_chain_id, market, method_selector } => {
                info!("Preparing proof data for ExtensionSupply");
                let users = vec![vec![from]];
                let markets = vec![vec![market]];
                let dst_chain_ids = vec![vec![dst_chain_id as u64]];
                let src_chain_ids = vec![src_chain_id as u64];

                debug!(
                    "Calling generate_proof_with_retry with: users={:?}, markets={:?}, dst_chains={:?}, src_chains={:?}",
                    users, markets, dst_chain_ids, src_chain_ids
                );

                let (journal, seal) = self.generate_proof_with_retry(
                    users,
                    markets,
                    dst_chain_ids,
                    src_chain_ids,
                ).await?;

                let method = if method_selector == crate::events::MINT_EXTERNAL_SELECTOR {
                    "mintExternal"
                } else if method_selector == crate::events::REPAY_EXTERNAL_SELECTOR {
                    "repayExternal"
                } else {
                    return Err(eyre::eyre!("Invalid method selector: {}", method_selector));
                };

                Ok(ProofReadyEvent {
                    market,
                    journal,
                    seal,
                    amount: vec![amount],
                    receiver: Address::ZERO,
                    method: method.to_string(),
                    dst_chain_id,
                })
            }
        };

        match &result {
            Ok(_) => info!("Successfully completed proof generation"),
            Err(e) => error!("Failed to generate proof: {}", e),
        }
        result
    }

    async fn generate_proof_with_retry(
        &self,
        users: Vec<Vec<Address>>,
        markets: Vec<Vec<Address>>,
        dst_chain_ids: Vec<Vec<u64>>,
        src_chain_ids: Vec<u64>,
    ) -> Result<(Bytes, Bytes)> {
        let mut attempts = 0;
        debug!(
            "Starting proof generation attempt for markets={:?}, src_chains={:?}, dst_chains={:?}",
            markets, src_chain_ids, dst_chain_ids
        );

        loop {
            match get_proof_data_prove(
                users.clone(),
                markets.clone(),
                dst_chain_ids.clone(),
                src_chain_ids.clone()
            ).await {
                Ok(proof_info) => {
                    info!("Successfully generated proof data");
                    let receipt = proof_info.receipt;
                    let seal = match risc0_ethereum_contracts::encode_seal(&receipt) {
                        Ok(seal_data) => {
                            debug!("Successfully encoded seal");
                            Bytes::from(seal_data)
                        },
                        Err(e) => {
                            error!("Failed to encode seal: {}", e);
                            return Err(eyre::eyre!("Failed to encode seal: {}", e));
                        }
                    };
                    let journal = Bytes::from(receipt.journal.bytes);
                    
                    info!(
                        "Generated proof - journal size: {}, seal size: {}", 
                        journal.len(), seal.len()
                    );
                    debug!(
                        "Proof details - journal: 0x{}, seal: 0x{}",
                        hex::encode(&journal),
                        hex::encode(&seal)
                    );
                    
                    return Ok((journal, seal));
                }
                Err(e) if attempts < self.max_retries => {
                    attempts += 1;
                    warn!(
                        "Proof generation attempt {} failed: {}. Retrying...",
                        attempts, e
                    );
                    tokio::time::sleep(self.retry_delay).await;
                }
                Err(e) => {
                    error!("Failed to generate proof after {} attempts: {}", attempts, e);
                    return Err(eyre::eyre!("Failed to generate proof after {} attempts: {}", attempts, e));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{PROOF_CHANNEL_CAPACITY, MAX_PROOF_RETRIES, PROOF_RETRY_DELAY};

    #[tokio::test]
    async fn test_proof_generator_creation() {
        let (event_tx, event_rx) = mpsc::channel(PROOF_CHANNEL_CAPACITY);
        let (proof_tx, _proof_rx) = mpsc::channel(PROOF_CHANNEL_CAPACITY);
        
        let _generator = ProofGenerator::new(
            event_rx,
            proof_tx,
            MAX_PROOF_RETRIES,
            PROOF_RETRY_DELAY,
        );
    }
} 