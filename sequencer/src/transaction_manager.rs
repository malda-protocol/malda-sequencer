use alloy::{
    primitives::{TxHash, U256},
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

use crate::{
    proof_generator::ProofReadyEvent,
    ProviderType,
    create_provider,
    types::IMaldaMarket,
    constants::{
        TX_TIMEOUT,
        SEQUENCER_ADDRESS,
        SEQUENCER_PRIVATE_KEY,
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
                    return Err(e).wrap_err(format!(
                        "Failed to submit transaction after {} attempts",
                        attempts
                    ));
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
        let market = IMaldaMarket::new(event.market, provider.clone());
        
        let tx_hash = match event.method.as_str() {
            "outHere" => {
                info!("Preparing outHere transaction for market {:?}", event.market);
                let action = market
                    .outHere(
                        event.journal.clone(),
                        event.seal.clone(),
                        event.amount.clone(),
                        event.receiver,
                    )
                    .from(SEQUENCER_ADDRESS);

                info!("Broadcasting outHere transaction with params: journal_size={}, seal_size={}, amount={:?}, receiver={:?}",
                    event.journal.len(), event.seal.len(), event.amount, event.receiver);
                
                let pending_tx = action.send().await
                    .wrap_err("Failed to send outHere transaction")?;
                let tx_hash = pending_tx.tx_hash();
                
                // Log transaction submission
                logger.log_step(
                    *tx_hash,
                    PipelineStep::TransactionSubmitted {
                        tx_hash: *tx_hash,
                        method: event.method.clone(),
                        gas_used: U256::from(0u64),
                        gas_price: U256::from(provider.get_gas_price().await?),
                    }
                ).await?;

                info!("Transaction sent with hash {}", tx_hash);

                debug!("Waiting for transaction confirmation with timeout {:?}", TX_TIMEOUT);
                match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
                    Ok(hash) => {
                        info!("Transaction confirmed with hash {:?}", hash);
                        hash
                    },
                    Err(e) => {
                        error!("outHere transaction failed: {}", e);
                        return Err(e).wrap_err("Failed to confirm outHere transaction");
                    }
                }
            },
            "mintExternal" => {
                info!("Preparing mintExternal transaction for market {:?}", event.market);
                let action = market
                    .mintExternal(
                        event.journal.clone(),
                        event.seal.clone(),
                        event.amount.clone(),
                        event.receiver,
                    )
                    .from(SEQUENCER_ADDRESS);

                info!("Broadcasting mintExternal transaction with params: journal_size={}, seal_size={}, amount={:?}, receiver={:?}",
                    event.journal.len(), event.seal.len(), event.amount, event.receiver);
                
                let pending_tx = action.send().await
                    .wrap_err("Failed to send mintExternal transaction")?;
                let tx_hash = pending_tx.tx_hash();

                // Log transaction submission
                logger.log_step(
                    *tx_hash,
                    PipelineStep::TransactionSubmitted {
                        tx_hash: *tx_hash,
                        method: event.method.clone(),
                        gas_used: U256::from(0u64),
                        gas_price: U256::from(provider.get_gas_price().await?),
                    }
                ).await?;

                info!("Transaction sent with hash {}", tx_hash);

                match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
                    Ok(hash) => {
                        info!("Transaction confirmed with hash {:?}", hash);
                        hash
                    },
                    Err(e) => {
                        error!("mintExternal transaction failed: {}", e);
                        return Err(e).wrap_err("Failed to confirm mintExternal transaction");
                    }
                }
            },
            "repayExternal" => {
                info!("Preparing repayExternal transaction for market {:?}", event.market);
                let action = market
                    .repayExternal(
                        event.journal.clone(),
                        event.seal.clone(),
                        event.amount.clone(),
                        event.receiver,
                    )
                    .from(SEQUENCER_ADDRESS);

                info!("Broadcasting repayExternal transaction with params: journal_size={}, seal_size={}, amount={:?}, receiver={:?}",
                    event.journal.len(), event.seal.len(), event.amount, event.receiver);
                
                let pending_tx = action.send().await
                    .wrap_err("Failed to send repayExternal transaction")?;
                let tx_hash = pending_tx.tx_hash();

                // Log transaction submission
                logger.log_step(
                    *tx_hash,
                    PipelineStep::TransactionSubmitted {
                        tx_hash: *tx_hash,
                        method: event.method.clone(),
                        gas_used: U256::from(0u64),
                        gas_price: U256::from(provider.get_gas_price().await?),
                    }
                ).await?;

                info!("Transaction sent with hash {}", tx_hash);

                match pending_tx.with_timeout(Some(TX_TIMEOUT)).watch().await {
                    Ok(hash) => {
                        info!("Transaction confirmed with hash {:?}", hash);
                        hash
                    },
                    Err(e) => {
                        error!("repayExternal transaction failed: {}", e);
                        return Err(e).wrap_err("Failed to confirm repayExternal transaction");
                    }
                }
            },
            method => {
                error!("Invalid transaction method: {}", method);
                return Err(eyre::eyre!("Invalid method: {}", method));
            }
        };

        // Validate the transaction
        match Self::validate_transaction_receipt(provider, tx_hash, event).await {
            Ok(receipt) => {
                info!("Transaction {:?} confirmed and validated", tx_hash);
                
                // Log transaction verification
                logger.log_step(
                    tx_hash,
                    PipelineStep::TransactionVerified {
                        tx_hash,
                        method: event.method.clone(),
                        block_number: receipt.block_number.unwrap_or_default(),
                        status: if receipt.status() { 1 } else { 0 },
                    }
                ).await?;

                Ok(tx_hash)
            },
            Err(e) => {
                error!("Transaction validation failed for hash {:?}: {}", tx_hash, e);
                Err(e)
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
    use crate::constants::{PROOF_CHANNEL_CAPACITY, MAX_TX_RETRIES, TX_RETRY_DELAY};

    #[tokio::test]
    async fn test_transaction_manager_creation() {
        let (_tx, rx) = mpsc::channel(PROOF_CHANNEL_CAPACITY);
        let config = TransactionConfig {
            max_retries: MAX_TX_RETRIES,
            retry_delay: TX_RETRY_DELAY,
            rpc_urls: vec![],
        };

        let _manager = TransactionManager::new(rx, config);
    }
} 