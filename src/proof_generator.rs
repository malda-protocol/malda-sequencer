use alloy::primitives::{Address, Bytes, TxHash};
use alloy::providers::{Provider, ProviderBuilder};
use alloy::transports::http::reqwest::Url;
use alloy::eips::BlockNumberOrTag;
use eyre::Result;
use sequencer::database::{Database, EventUpdate};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tokio::time::{sleep, Instant};
use tracing::{debug, error, info, warn};
use std::env;

use malda_rs::viewcalls::get_proof_data_prove_sdk;
use malda_rs::constants::*;

#[derive(Debug)]
pub struct ProofInfo {
    pub journal: Bytes,
    pub seal: Bytes,
    pub uuid: String,
    pub stark_time: i32,
    pub snark_time: i32,
    pub total_cycles: i64,
}

pub struct ProofGenerator {
    max_retries: u32,
    retry_delay: Duration,
    last_proof_time: Arc<Mutex<Instant>>,
    db: Database,
    batch_size: usize,
    ethereum_max_block_delay_secs: u64,
    l2_max_block_delay_secs: u64,
}

impl ProofGenerator {
    pub fn new(
        max_retries: u32,
        retry_delay: Duration,
        db: Database,
        batch_size: usize,
        ethereum_max_block_delay_secs: u64,
        l2_max_block_delay_secs: u64,
    ) -> Self {
        Self {
            max_retries,
            retry_delay,
            last_proof_time: Arc::new(Mutex::new(Instant::now())),
            db,
            batch_size,
            ethereum_max_block_delay_secs,
            l2_max_block_delay_secs,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting proof generator, reading processed events from database...");

        let proof_delay = Duration::from_secs(env::var("PROOF_GENERATOR_PROOF_REQUEST_DELAY").expect("PROOF_GENERATOR_PROOF_REQUEST_DELAY must be set in .env").parse::<u64>().unwrap());

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
            let ethereum_max_block_delay_secs = self.ethereum_max_block_delay_secs;
            let l2_max_block_delay_secs = self.l2_max_block_delay_secs;

            tokio::spawn(async move {
                let proof_generator = ProofGeneratorWorker {
                    max_retries,
                    retry_delay,
                    db,
                    ethereum_max_block_delay_secs,
                    l2_max_block_delay_secs,
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
    ethereum_max_block_delay_secs: u64,
    l2_max_block_delay_secs: u64,
}

impl ProofGeneratorWorker {
    async fn process_batch(&self, events: Vec<EventUpdate>) -> Result<()> {
        // Input `events` already have status = ProofRequested
        // Sort events by src_chain (Linea first) and dst_chain for consistent journal indexing
        let mut sorted_events = events;
        sorted_events.sort_by(|a, b| {
            let a_src = a.src_chain_id.unwrap_or(LINEA_CHAIN_ID as u32) as u64;
            let b_src = b.src_chain_id.unwrap_or(LINEA_CHAIN_ID as u32) as u64;
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
        let proof_info = self.generate_proof_with_retry(
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
                &proof_info.journal, 
                &proof_info.seal,
                &proof_info.uuid,
                proof_info.stark_time,
                proof_info.snark_time,
                proof_info.total_cycles,
            ).await {
                error!("Failed to update events with proof data and index: {:?}", e);
                // Consider if we should return error here or just log
            }
        }

        Ok(())
    }

    // Check provider for block freshness
    async fn is_provider_fresh(&self, rpc_url: &str, chain_id: u64, max_block_delay_secs: u64) -> bool {
        // Parse URL
        let url = match Url::parse(rpc_url) {
            Ok(url) => url,
            Err(e) => {
                warn!("Failed to parse RPC URL {}: {}", rpc_url, e);
                return false;
            }
        };
        
        // Create provider
        let provider = ProviderBuilder::new().on_http(url);
        
        // Get latest block
        let block = match provider.get_block_by_number(BlockNumberOrTag::Latest, false.into()).await {
            Ok(Some(block)) => block,
            Ok(None) => {
                warn!("Provider at {} returned no latest block", rpc_url);
                return false;
            }
            Err(e) => {
                warn!("Failed to get latest block from provider at {}: {}", rpc_url, e);
                return false;
            }
        };
        
        // Check block timestamp
        let block_timestamp: u64 = block.header.inner.timestamp.into();
        let current_time = chrono::Utc::now().timestamp() as u64;
        
        if current_time > block_timestamp && (current_time - block_timestamp) > max_block_delay_secs {
            warn!(
                "Provider's latest block for chain {} is too old: block_time={}, current_time={}, diff={}s, max_delay={}s",
                chain_id, block_timestamp, current_time, current_time - block_timestamp, max_block_delay_secs
            );
            return false;
        }
        
        // Provider is fresh
        true
    }

    async fn generate_proof_with_retry(
        &self,
        users: Vec<Vec<Address>>,
        markets: Vec<Vec<Address>>,
        dst_chain_ids: Vec<Vec<u64>>,
        src_chain_ids: Vec<u64>,
    ) -> Result<ProofInfo> {
        let mut attempts = 0;
        debug!(
            "Starting proof generation attempt for markets={:?}, src_chains={:?}, dst_chains={:?}",
            markets, src_chain_ids, dst_chain_ids
        );

        loop {
            // Check providers for all source chains involved
            let should_use_fallback = if attempts >= self.max_retries / 2 {
                // If we've retried many times, always use fallback
                true
            } else {
                // Otherwise check provider freshness
                let mut use_fallback = false;
                
                for &chain_id in &src_chain_ids {
                    // Determine RPC URL for this chain
                    let (rpc_url, max_block_delay_secs) = match chain_id {
                        // Ethereum mainnet
                        id if id == ETHEREUM_CHAIN_ID => {
                            (rpc_url_ethereum().to_string(), self.ethereum_max_block_delay_secs)
                        }
                        // Ethereum Sepolia testnet
                        id if id == ETHEREUM_SEPOLIA_CHAIN_ID => {
                            (rpc_url_ethereum_sepolia().to_string(), self.ethereum_max_block_delay_secs)
                        }
                        // Optimism mainnet
                        id if id == OPTIMISM_CHAIN_ID => {
                            (rpc_url_optimism().to_string(), self.l2_max_block_delay_secs)
                        }
                        // Optimism Sepolia testnet
                        id if id == OPTIMISM_SEPOLIA_CHAIN_ID => {
                            (rpc_url_optimism_sepolia().to_string(), self.l2_max_block_delay_secs)
                        }
                        // Linea mainnet
                        id if id == LINEA_CHAIN_ID => {
                            (rpc_url_linea().to_string(), self.l2_max_block_delay_secs)
                        }
                        // Linea Sepolia testnet
                        id if id == LINEA_SEPOLIA_CHAIN_ID => {
                            (rpc_url_linea_sepolia().to_string(), self.l2_max_block_delay_secs)
                        }
                        // Base mainnet
                        id if id == BASE_CHAIN_ID => {
                            (rpc_url_base().to_string(), self.l2_max_block_delay_secs)
                        }
                        // Base Sepolia testnet
                        id if id == BASE_SEPOLIA_CHAIN_ID => {
                            (rpc_url_base_sepolia().to_string(), self.l2_max_block_delay_secs)
                        }
                        _ => {
                            warn!("Unknown chain ID: {}, defaulting to fallback mode", chain_id);
                            use_fallback = true;
                            break;
                        }
                    };
                    
                    // Check if provider is fresh
                    if !self.is_provider_fresh(&rpc_url, chain_id, max_block_delay_secs).await {
                        use_fallback = true;
                        break;
                    }
                }
                
                use_fallback
            };
            
            if should_use_fallback {
                info!("Using fallback mode for proof generation due to provider issues or after {} failed attempts", attempts);
            } else {
                info!("Using primary mode for proof generation");
            }
            
            let start_time = Instant::now();
            
            match get_proof_data_prove_sdk(
                users.clone(),
                markets.clone(),
                dst_chain_ids.clone(),
                src_chain_ids.clone(),
                false,
                should_use_fallback
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
                        "Generated proof for {} transactions with {} cycles in {:?}{}",
                        tx_num, cycles, duration,
                        if should_use_fallback { " (fallback mode)" } else { "" }
                    );
                    debug!(
                        "Proof details - journal: 0x{}, seal: 0x{}",
                        hex::encode(&journal),
                        hex::encode(&seal)
                    );

                    return Ok(ProofInfo {
                        journal,
                        seal,
                        uuid: proof_info.uuid,
                        stark_time: proof_info.stark_time as i32,
                        snark_time: proof_info.snark_time as i32,
                        total_cycles: cycles as i64,
                    });
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
