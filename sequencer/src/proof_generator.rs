use alloy::primitives::{Address, Bytes, U256, TxHash};
use eyre::Result;
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};
use std::time::Duration;
use tokio::task;
use tokio::time::{sleep, Instant};
use std::sync::Arc;
use tokio::sync::Mutex;
use futures::future::join_all;

use crate::event_processor::ProcessedEvent;
use malda_rs::viewcalls::get_proof_data_prove;
use sequencer::logger::{PipelineLogger, PipelineStep};

#[derive(Debug)]
pub struct ProofReadyEvent {
    pub tx_hash: TxHash,
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
    last_proof_time: Arc<Mutex<Instant>>,
    logger: PipelineLogger,
}

impl ProofGenerator {
    pub fn new(
        event_receiver: mpsc::Receiver<ProcessedEvent>,
        proof_sender: mpsc::Sender<ProofReadyEvent>,
        max_retries: u32,
        retry_delay: Duration,
        logger: PipelineLogger,
    ) -> Self {
        Self {
            event_receiver,
            proof_sender,
            max_retries,
            retry_delay,
            last_proof_time: Arc::new(Mutex::new(Instant::now())),
            logger,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting proof generator, waiting for events...");
        
        let mut processing_tasks = Vec::new();
        const MAX_CONCURRENT_TASKS: usize = 10;

        while let Some(event) = self.event_receiver.recv().await {
            info!(
                "Proof generator received event: type={}", 
                match &event {
                    ProcessedEvent::HostWithdraw { .. } => "HostWithdraw",
                    ProcessedEvent::HostBorrow { .. } => "HostBorrow",
                    ProcessedEvent::ExtensionSupply { .. } => "ExtensionSupply",
                }
            );

            let proof_sender = self.proof_sender.clone();
            let max_retries = self.max_retries;
            let retry_delay = self.retry_delay;
            let last_proof_time = Arc::clone(&self.last_proof_time);
            let logger = self.logger.clone();

            let task = task::spawn(async move {
                let mut last_time = last_proof_time.lock().await;
                let elapsed = last_time.elapsed();
                if elapsed < Duration::from_secs(crate::constants::PROOF_REQUEST_DELAY) {
                    let wait_time = Duration::from_secs(crate::constants::PROOF_REQUEST_DELAY) - elapsed;
                    debug!("Waiting {}s before generating next proof", wait_time.as_secs());
                    sleep(wait_time).await;
                }
                *last_time = Instant::now();
                drop(last_time);

                let proof_generator = ProofGeneratorWorker {
                    max_retries,
                    retry_delay,
                };

                match proof_generator.process_event(event, &logger).await {
                    Ok(proof_event) => {
                        info!(
                            "Successfully generated proof for market={:?}, method={}, chain={}",
                            proof_event.market, proof_event.method, proof_event.dst_chain_id
                        );
                        
                        if let Err(e) = proof_sender.send(proof_event).await {
                            error!("Failed to send proof ready event: {}", e);
                        }
                    }
                    Err(e) => {
                        error!("Failed to generate proof: {}", e);
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

        warn!("Proof generator channel closed");
        Ok(())
    }
}

struct ProofGeneratorWorker {
    max_retries: u32,
    retry_delay: Duration,
}

impl ProofGeneratorWorker {
    async fn process_event(&self, event: ProcessedEvent, logger: &PipelineLogger) -> Result<ProofReadyEvent> {
        let result = match event {
            ProcessedEvent::HostWithdraw { tx_hash, sender, dst_chain_id, amount, market } => {
                info!("Preparing proof data for HostWithdraw");
                let users = vec![vec![sender]];
                let markets = vec![vec![market]];
                let dst_chain_ids = vec![vec![dst_chain_id as u64]];
                let src_chain_ids = vec![malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID];

                debug!(
                    "Calling generate_proof_with_retry with: users={:?}, markets={:?}, dst_chains={:?}, src_chains={:?}",
                    users, markets, dst_chain_ids, src_chain_ids
                );

                let start_time = Instant::now();
                debug!("Starting proof generation at {:?}", start_time);

                let (journal, seal) = self.generate_proof_with_retry(
                    users,
                    markets,
                    dst_chain_ids,
                    src_chain_ids,
                ).await?;

                let duration_ms = start_time.elapsed().as_millis() as u64;
                debug!("Proof generation completed in {}ms", duration_ms);

                // Log the proof generation
                logger.log_step(
                    tx_hash,
                    PipelineStep::ProofGenerated {
                        duration_ms,
                        journal: hex::encode(&journal),
                        seal: hex::encode(&seal),
                    }
                ).await?;

                info!("Successfully completed proof generation");

                ProofReadyEvent {
                    tx_hash,
                    market,
                    journal,
                    seal,
                    amount: vec![amount],
                    receiver: Address::ZERO,
                    method: "outHere".to_string(),
                    dst_chain_id,
                }
            },
            ProcessedEvent::HostBorrow { tx_hash, sender, dst_chain_id, amount, market } => {
                info!("Preparing proof data for HostBorrow");
                let users = vec![vec![sender]];
                let markets = vec![vec![market]];
                let dst_chain_ids = vec![vec![dst_chain_id as u64]];
                let src_chain_ids = vec![malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID];

                debug!(
                    "Calling generate_proof_with_retry with: users={:?}, markets={:?}, dst_chains={:?}, src_chains={:?}",
                    users, markets, dst_chain_ids, src_chain_ids
                );

                let start_time = Instant::now();
                debug!("Starting proof generation at {:?}", start_time);

                let (journal, seal) = self.generate_proof_with_retry(
                    users,
                    markets,
                    dst_chain_ids,
                    src_chain_ids,
                ).await?;

                let duration_ms = start_time.elapsed().as_millis() as u64;
                debug!("Proof generation completed in {}ms", duration_ms);

                // Log the proof generation
                logger.log_step(
                    tx_hash,
                    PipelineStep::ProofGenerated {
                        duration_ms,
                        journal: hex::encode(&journal),
                        seal: hex::encode(&seal),
                    }
                ).await?;

                info!("Successfully completed proof generation");

                ProofReadyEvent {
                    tx_hash,
                    market,
                    journal,
                    seal,
                    amount: vec![amount],
                    receiver: Address::ZERO,
                    method: "outHere".to_string(),
                    dst_chain_id,
                }
            },
            ProcessedEvent::ExtensionSupply { tx_hash, from, amount, src_chain_id, dst_chain_id, market, method_selector } => {
                info!("Preparing proof data for ExtensionSupply");
                let users = vec![vec![from]];
                let markets = vec![vec![market]];
                let dst_chain_ids = vec![vec![dst_chain_id as u64]];
                let src_chain_ids = vec![src_chain_id as u64];

                debug!(
                    "Calling generate_proof_with_retry with: users={:?}, markets={:?}, dst_chains={:?}, src_chains={:?}",
                    users, markets, dst_chain_ids, src_chain_ids
                );

                let start_time = Instant::now();
                debug!("Starting proof generation at {:?}", start_time);

                let (journal, seal) = self.generate_proof_with_retry(
                    users,
                    markets,
                    dst_chain_ids,
                    src_chain_ids,
                ).await?;

                let duration_ms = start_time.elapsed().as_millis() as u64;
                debug!("Proof generation completed in {}ms", duration_ms);

                // Log the proof generation
                logger.log_step(
                    tx_hash,
                    PipelineStep::ProofGenerated {
                        duration_ms,
                        journal: hex::encode(&journal),
                        seal: hex::encode(&seal),
                    }
                ).await?;

                info!("Successfully completed proof generation");

                let method = if method_selector == crate::events::MINT_EXTERNAL_SELECTOR {
                    "mintExternal"
                } else if method_selector == crate::events::REPAY_EXTERNAL_SELECTOR {
                    "repayExternal"
                } else {
                    return Err(eyre::eyre!("Invalid method selector: {}", method_selector));
                };

                ProofReadyEvent {
                    tx_hash,
                    market,
                    journal,
                    seal,
                    amount: vec![amount],
                    receiver: Address::ZERO,
                    method: method.to_string(),
                    dst_chain_id,
                }
            }
        };

        Ok(result)
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
    async fn test_proof_generator_creation() -> Result<()> {
        let (_event_tx, event_rx) = mpsc::channel(PROOF_CHANNEL_CAPACITY);
        let (proof_tx, _proof_rx) = mpsc::channel(PROOF_CHANNEL_CAPACITY);
        
        let logger = PipelineLogger::new(PathBuf::from("test_pipeline.log")).await?;
        
        let _generator = ProofGenerator::new(
            event_rx,
            proof_tx,
            MAX_PROOF_RETRIES,
            PROOF_RETRY_DELAY,
            logger,
        );
        Ok(())
    }
} 