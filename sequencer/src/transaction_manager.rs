use alloy::{
    primitives::{TxHash, U256, FixedBytes},
    transports::http::reqwest::Url,
    providers::Provider,
    rpc::types::TransactionReceipt,
};
use eyre::{Result, WrapErr};
use tokio::sync::mpsc;
use tracing::{info, error, warn, debug};
use std::time::Duration;
use tokio::task;
use futures::future::join_all;
use sequencer::logger::{PipelineLogger, PipelineStep};
use chrono;
use hex;

type Bytes4 = FixedBytes<4>; 

use crate::{
    proof_generator::ProofReadyEvent,
    ProviderType,
    create_provider,
    constants::{
        TX_TIMEOUT,
        SEQUENCER_ADDRESS,
        SEQUENCER_PRIVATE_KEY,
        BATCH_SUBMITTER,
    },
    types::{IBatchSubmitter, BatchProcessMsg},
    events::{
        MINT_EXTERNAL_SELECTOR_FB4,
        REPAY_EXTERNAL_SELECTOR_FB4,
        OUT_HERE_SELECTOR_FB4,
    },
};

#[derive(Debug, Clone)]
pub struct TransactionConfig {
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub rpc_urls: Vec<(u32, String)>, // (chain_id, url)
}

pub struct TransactionManager {
    event_receiver: mpsc::Receiver<ProofReadyEvent>,
    config: TransactionConfig,
    logger: PipelineLogger,
}

impl std::fmt::Debug for TransactionManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TransactionManager")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl TransactionManager {
    pub fn new(
        event_receiver: mpsc::Receiver<ProofReadyEvent>,
        config: TransactionConfig,
        logger: PipelineLogger,
    ) -> Self {
        Self {
            event_receiver,
            config,
            logger,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting transaction manager");
        
        let mut processing_tasks = Vec::new();
        const MAX_CONCURRENT_TASKS: usize = 10;

        // Clone these before the loop to avoid self reference issues
        let config = self.config.clone();
        let logger = self.logger.clone();

        while let Some(event) = self.event_receiver.recv().await {
            let config = config.clone();
            let logger = logger.clone();
            
            let task = task::spawn(async move {
                match Self::process_transaction(event, &config, &logger).await {
                    Ok(tx_hash) => {
                        info!("Transaction submitted successfully: {:?}", tx_hash);
                    }
                    Err(e) => {
                        error!("Failed to process transaction: {}", e);
                    }
                }
            });

            processing_tasks.push(task);

            // When we hit the concurrent task limit, wait for all tasks to complete
            if processing_tasks.len() >= MAX_CONCURRENT_TASKS {
                join_all(processing_tasks).await;
                processing_tasks = Vec::new();
            }
        }

        // Wait for any remaining tasks to complete
        if !processing_tasks.is_empty() {
            join_all(processing_tasks).await;
        }

        warn!("Transaction manager channel closed");
        Ok(())
    }

    async fn get_provider_for_chain(chain_id: u32, config: &TransactionConfig) -> Result<ProviderType> {
        let rpc_url = config.rpc_urls
            .iter()
            .find(|(id, _)| *id == chain_id)
            .map(|(_, url)| url.clone())
            .ok_or_else(|| eyre::eyre!("No RPC URL configured for chain {}", chain_id))?;

        let url = Url::parse(&rpc_url)?;
        create_provider(url, SEQUENCER_PRIVATE_KEY).await
            .map_err(|e| eyre::eyre!("Failed to create provider: {}", e))
    }

    async fn submit_with_retry(
        provider: &ProviderType,
        event: &ProofReadyEvent,
        config: &TransactionConfig,
        logger: &PipelineLogger,
    ) -> Result<TxHash> {
        let mut attempts = 0;
        loop {
            match Self::submit_transaction(provider, event, logger).await {
                Ok(tx_hash) => return Ok(tx_hash),
                Err(e) if attempts < config.max_retries => {
                    attempts += 1;
                    warn!(
                        "Transaction attempt {} failed: {}. Retrying...",
                        attempts, e
                    );
                    tokio::time::sleep(config.retry_delay).await;
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
    }

    async fn validate_transaction_receipt(
        provider: &ProviderType,
        tx_hash: TxHash,
        event: &ProofReadyEvent,
    ) -> Result<TransactionReceipt> {
        debug!("Waiting for receipt for transaction {:?}", tx_hash);
        let receipt = provider
            .get_transaction_receipt(tx_hash)
            .await
            .wrap_err("Failed to get transaction receipt")?
            .ok_or_else(|| eyre::eyre!("Transaction receipt not found"))?;

        debug!("Received receipt: status={:?}, block={:?}, gas_used={:?}",
            receipt.status(), receipt.block_number, receipt.gas_used);

        // Check transaction status
        if !receipt.status() {
            error!("Transaction failed: {:?}", tx_hash);
            return Err(eyre::eyre!("Transaction failed"));
        }

        // Check if transaction was mined in the correct chain
        let chain_id = provider
            .get_chain_id()
            .await
            .wrap_err("Failed to get chain ID")?;
        if chain_id != event.dst_chain_id as u64 {
            return Err(eyre::eyre!(
                "Transaction mined in wrong chain. Expected {}, got {}",
                event.dst_chain_id,
                chain_id
            ));
        }

        info!(
            "Transaction validated successfully: hash={:?}, block={:?}", 
            tx_hash, receipt.block_number
        );
        Ok(receipt)
    }

    async fn submit_transaction(provider: &ProviderType, event: &ProofReadyEvent, logger: &PipelineLogger) -> Result<TxHash> {
        info!("Preparing batch submission for market {:?}", event.market);
        
        // Create batch submitter contract instance
        let batch_submitter = IBatchSubmitter::new(BATCH_SUBMITTER, provider.clone());

        // Construct BatchProcessMsg
        let msg = BatchProcessMsg {
            receiver: event.receiver,
            journalData: event.journal.clone(),
            seal: event.seal.clone(),
            mTokens: vec![event.market], // Single market for now
            amounts: event.amount.clone(),
            selectors: vec![
                // Match method name to selector
                match event.method.as_str() {
                    "outHere" => Bytes4::from_slice(OUT_HERE_SELECTOR_FB4),
                    "mintExternal" => Bytes4::from_slice(MINT_EXTERNAL_SELECTOR_FB4),
                    "repayExternal" => Bytes4::from_slice(REPAY_EXTERNAL_SELECTOR_FB4),
                    method => {
                        error!("Invalid transaction method: {}", method);
                        return Err(eyre::eyre!("Invalid method: {}", method));
                    }
                }
            ],
            startIndex: U256::from(0u64),
            endIndex: U256::from(1u64), // Single transaction batch
        };

        info!(
            "Broadcasting batch transaction with params: journal_size={}, seal_size={}, market={:?}, amount={:?}, receiver={:?}",
            msg.journalData.len(),
            msg.seal.len(),
            msg.mTokens,
            msg.amounts,
            msg.receiver
        );

        // Submit the batch
        let action = batch_submitter
            .batchProcess(msg)
            .from(SEQUENCER_ADDRESS);

        let pending_tx = action.send().await?;
        let tx_hash = pending_tx.tx_hash();

        // Log transaction submission
        logger.log_step(
            *tx_hash,
            PipelineStep::TransactionSubmitted {
                tx_hash: *tx_hash,
                method: "submitBatch".to_string(),
                gas_used: U256::from(0u64),
                gas_price: U256::from(provider.get_gas_price().await?),
            }
        ).await?;

        info!("Batch transaction sent with hash {}", tx_hash);

        match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
            Ok(hash) => {
                info!("Batch transaction confirmed with hash {:?}", hash);
                Ok(hash)
            },
            Err(e) => {
                error!("Batch transaction failed: {}", e);
                Err(e.into())
            }
        }
    }

    async fn process_transaction(event: ProofReadyEvent, config: &TransactionConfig, logger: &PipelineLogger) -> Result<TxHash> {
        info!(
            "Processing transaction for market={:?}, method={}, chain={}",
            event.market, event.method, event.dst_chain_id
        );

        let provider = Self::get_provider_for_chain(event.dst_chain_id, config).await?;
        let tx_hash = Self::submit_with_retry(&provider, &event, config, logger).await?;

        // Log successful transaction completion
        let current_time = chrono::Utc::now();
        logger.write_to_log(&format!(
            "{}, TxHash: {}, Transaction: Finished, tx={}\n",
            current_time.format("%Y-%m-%d %H:%M:%S"),
            hex::encode(event.tx_hash),  // Changed from original_tx_hash to tx_hash
            hex::encode(tx_hash),        // New transaction hash
        )).await?;

        Ok(tx_hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::{
        PROOF_CHANNEL_CAPACITY,
        MAX_TX_RETRIES,
        TX_RETRY_DELAY,
    };
    use std::path::PathBuf;

    #[tokio::test]
    async fn test_transaction_manager_creation() {
        let (_tx, rx) = mpsc::channel(PROOF_CHANNEL_CAPACITY);
        let config = TransactionConfig {
            max_retries: MAX_TX_RETRIES,
            retry_delay: TX_RETRY_DELAY,
            rpc_urls: vec![(1, "http://localhost:8545".to_string())],
        };

        // Create a temporary logger for testing
        let logger = PipelineLogger::new(PathBuf::from("test_pipeline.log"))
            .await
            .expect("Failed to create test logger");

        let manager = TransactionManager::new(rx, config, logger);
        
        // Basic assertions to ensure the manager was created correctly
        assert_eq!(manager.config.max_retries, MAX_TX_RETRIES);
        assert_eq!(manager.config.retry_delay, TX_RETRY_DELAY);
        assert!(!manager.config.rpc_urls.is_empty());
    }
} 