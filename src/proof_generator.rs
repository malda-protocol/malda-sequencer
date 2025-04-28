use alloy::primitives::{Address, Bytes, TxHash, U256};
use eyre::Result;
use sequencer::database::{Database, EventStatus, EventUpdate};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};
use tracing::{debug, error, info, warn};
use chrono::Utc;

use malda_rs::viewcalls::get_proof_data_prove_sdk;

pub struct ProofGenerator {
    max_retries: u32,
    retry_delay: Duration,
    last_proof_time: Arc<Mutex<Instant>>,
    db: Database,
    batch_size: usize,
}

impl ProofGenerator {
    pub fn new(
        max_retries: u32,
        retry_delay: Duration,
        db: Database,
        batch_size: usize,
    ) -> Self {
        Self {
            max_retries,
            retry_delay,
            last_proof_time: Arc::new(Mutex::new(Instant::now())),
            db,
            batch_size,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting proof generator, reading processed events from database...");

        let proof_delay = Duration::from_secs(crate::constants::PROOF_REQUEST_DELAY);

        loop {
            // Get processed events from database - they are claimed atomically now
            let claimed_events = self.db.get_ready_to_request_proof_events(
                proof_delay.as_secs() as i64, 
                self.batch_size as i64
            ).await?;
            
            if claimed_events.is_empty() {
                // info!("No events ready for proof request, waiting...");
                sleep(proof_delay).await;
                continue;
            }

            // Use claimed_events directly
            let events_to_process = claimed_events;

            info!("Processing batch of {} events for proof generation", events_to_process.len());

            // Process the batch in a spawned task
            let max_retries = self.max_retries;
            let retry_delay = self.retry_delay;
            let db = self.db.clone();

            tokio::spawn(async move {
                let proof_generator = ProofGeneratorWorker {
                    max_retries,
                    retry_delay,
                    db,
                };

                if let Err(e) = proof_generator.process_batch(events_to_process).await { // Use events_to_process
                    error!("Failed to generate proofs for batch: {}", e);
                }
            });

            // Wait for the proof delay before checking for new events
            sleep(proof_delay).await;
        }
    }
}

struct ProofGeneratorWorker {
    max_retries: u32,
    retry_delay: Duration,
    db: Database,
}

impl ProofGeneratorWorker {
    async fn process_batch(&self, events: Vec<EventUpdate>) -> Result<()> {
        // Input `events` already have status = ProofRequested
        // Sort events by src_chain (Linea first) and dst_chain for consistent journal indexing
        let mut sorted_events = events;
        sorted_events.sort_by(|a, b| {
            let a_src = a.src_chain_id.unwrap_or(malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID as u32) as u64;
            let b_src = b.src_chain_id.unwrap_or(malda_rs::constants::LINEA_SEPOLIA_CHAIN_ID as u32) as u64;
            let a_dst = a.dst_chain_id.unwrap_or(0) as u64;
            let b_dst = b.dst_chain_id.unwrap_or(0) as u64;

            // Sort by src_chain first (Linea first), then by dst_chain
            match a_src.cmp(&b_src) {
                std::cmp::Ordering::Equal => a_dst.cmp(&b_dst),
                other => other,
            }
        });

        // Initialize vectors for proof generation
        let mut users: Vec<Vec<Address>> = Vec::new();
        let mut markets: Vec<Vec<Address>> = Vec::new();
        let mut dst_chain_ids: Vec<Vec<u64>> = Vec::new();
        let mut src_chain_ids: Vec<u64> = Vec::new();

        // Group events by source chain
        let mut current_src_chain: Option<u64> = None;
        let mut current_users: Vec<Address> = Vec::new();
        let mut current_markets: Vec<Address> = Vec::new();
        let mut current_dst_chains: Vec<u64> = Vec::new();

        // Process sorted events
        for event in sorted_events.iter() {
            let src_chain = event.src_chain_id.unwrap_or(0) as u64;
            let dst_chain = event.dst_chain_id.unwrap_or(0) as u64;
            let user = event.msg_sender.unwrap_or_default();
            let market = event.market.unwrap_or_default();

            // If we encounter a new source chain, push the current batch and start a new one
            if current_src_chain != Some(src_chain) {
                if !current_users.is_empty() {
                    users.push(current_users);
                    markets.push(current_markets);
                    dst_chain_ids.push(current_dst_chains);
                    src_chain_ids.push(current_src_chain.unwrap());
                }
                current_users = Vec::new();
                current_markets = Vec::new();
                current_dst_chains = Vec::new();
                current_src_chain = Some(src_chain);
            }

            // Add to current batch
            current_users.push(user);
            current_markets.push(market);
            current_dst_chains.push(dst_chain);
        }

        // Push the last batch
        if !current_users.is_empty() {
            users.push(current_users);
            markets.push(current_markets);
            dst_chain_ids.push(current_dst_chains);
            src_chain_ids.push(current_src_chain.unwrap());
        }

        let start_time = Instant::now();
        debug!(
            "Starting batch proof generation for {} source chains at {:?}",
            src_chain_ids.len(),
            start_time
        );

        // Generate single proof for all events
        let (journal, seal) = self.generate_proof_with_retry(
            users.clone(),
            markets.clone(),
            dst_chain_ids.clone(),
            src_chain_ids.clone(),
        ).await?;

        let duration_ms = start_time.elapsed().as_millis() as u64;
        debug!("Batch proof generation completed in {}ms", duration_ms);

        info!("Source chains included in the proof: {:?}", src_chain_ids);
        info!("Destination chains included in the proof: {:?}", dst_chain_ids);

        // Create the mapping of TxHash to its final journal_index
        let updates_with_index: Vec<(TxHash, i32)> = sorted_events
            .iter()
            .enumerate()
            .map(|(idx, event)| (event.tx_hash, idx as i32))
            .collect();
        
        // Update database with proof data and journal indices using the specific function
        if !updates_with_index.is_empty() {
            if let Err(e) = self.db.set_events_proof_received_with_index(
                &updates_with_index, 
                &journal, 
                &seal
            ).await {
                error!("Failed to update events with proof data and index: {:?}", e);
                // Consider if we should return error here or just log
            }
        }

        Ok(())
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
            let start_time = Instant::now();
            match get_proof_data_prove_sdk(
                users.clone(),
                markets.clone(),
                dst_chain_ids.clone(),
                src_chain_ids.clone(),
                false,
            )
            .await
            {
                Ok(proof_info) => {
                    let duration = start_time.elapsed();
                    let receipt = proof_info.receipt;
                    let seal = match risc0_ethereum_contracts::encode_seal(&receipt) {
                        Ok(seal_data) => {
                            debug!("Successfully encoded seal");
                            Bytes::from(seal_data)
                        }
                        Err(e) => {
                            error!("Failed to encode seal: {}", e);
                            return Err(eyre::eyre!("Failed to encode seal: {}", e));
                        }
                    };
                    let journal = Bytes::from(receipt.journal.bytes);

                    let cycles = proof_info.stats.total_cycles;
                    let tx_num = users.iter().flatten().count();
                    info!(
                        "Generated proof for {} transactions with {} cycles in {:?}",
                        tx_num, cycles, duration
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
                    error!(
                        "Failed to generate proof after {} attempts: {}",
                        attempts, e
                    );
                    return Err(eyre::eyre!(
                        "Failed to generate proof after {} attempts: {}",
                        attempts,
                        e
                    ));
                }
            }
        }
    }
}
