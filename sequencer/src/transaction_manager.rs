use alloy::{
    primitives::{TxHash, address},
    transports::http::reqwest::Url,
    providers::Provider,
};
use eyre::{Result, WrapErr};
use tokio::sync::mpsc;
use tracing::{info, error, warn, debug};
use std::time::Duration;

use crate::{
    proof_generator::ProofReadyEvent,
    ProviderType,
    create_provider,
    PRIVATE_KEY_SENDER,
    TX_TIMEOUT,
    types::IMaldaMarket,
};

#[derive(Debug)]
pub struct TransactionConfig {
    pub max_retries: u32,
    pub retry_delay: Duration,
    pub rpc_urls: Vec<(u32, String)>, // (chain_id, url)
}

#[derive(Debug)]
pub struct TransactionManager {
    event_receiver: mpsc::Receiver<ProofReadyEvent>,
    config: TransactionConfig,
}

impl TransactionManager {
    pub fn new(
        event_receiver: mpsc::Receiver<ProofReadyEvent>,
        config: TransactionConfig,
    ) -> Self {
        Self {
            event_receiver,
            config,
        }
    }

    pub async fn start(&mut self) -> Result<()> {
        info!("Starting transaction manager");
        
        while let Some(event) = self.event_receiver.recv().await {
            match self.process_transaction(event).await {
                Ok(tx_hash) => {
                    info!("Transaction submitted successfully: {:?}", tx_hash);
                }
                Err(e) => {
                    error!("Failed to process transaction: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn get_provider_for_chain(&self, chain_id: u32) -> Result<ProviderType> {
        let rpc_url = self.config.rpc_urls
            .iter()
            .find(|(id, _)| *id == chain_id)
            .map(|(_, url)| url.clone())
            .ok_or_else(|| eyre::eyre!("No RPC URL configured for chain {}", chain_id))?;

        let url = Url::parse(&rpc_url)?;
        create_provider(url, PRIVATE_KEY_SENDER).await
            .map_err(|e| eyre::eyre!("Failed to create provider: {}", e))
    }

    async fn submit_with_retry(
        &self,
        provider: &ProviderType,
        event: &ProofReadyEvent,
    ) -> Result<TxHash> {
        let mut attempts = 0;
        loop {
            match self.submit_transaction(provider, event).await {
                Ok(tx_hash) => return Ok(tx_hash),
                Err(e) if attempts < self.config.max_retries => {
                    attempts += 1;
                    warn!(
                        "Transaction attempt {} failed: {}. Retrying...",
                        attempts, e
                    );
                    tokio::time::sleep(self.config.retry_delay).await;
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
        &self,
        provider: &ProviderType,
        tx_hash: TxHash,
        event: &ProofReadyEvent,
    ) -> Result<()> {
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
        Ok(())
    }

    async fn submit_transaction(
        &self,
        provider: &ProviderType,
        event: &ProofReadyEvent,
    ) -> Result<TxHash> {
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
                    .from(address!("2693946791da99dA78Ac441abA6D5Ce2Bccd96D3"));

                info!("Broadcasting outHere transaction with params: journal_size={}, seal_size={}, amount={:?}, receiver={:?}",
                    event.journal.len(), event.seal.len(), event.amount, event.receiver);
                
                let pending_tx = action.send().await
                    .wrap_err("Failed to send outHere transaction")?;
                info!("Transaction sent with hash {}", pending_tx.tx_hash());

                debug!("Waiting for transaction confirmation with timeout {:?}", TX_TIMEOUT);
                let tx_hash = pending_tx
                    .with_timeout(Some(TX_TIMEOUT))
                    .watch()
                    .await
                    .wrap_err("Failed to confirm outHere transaction")?;

                info!("Transaction confirmed with hash {:?}", tx_hash);

                // Add validation
                self.validate_transaction_receipt(provider, tx_hash, event).await?;
                tx_hash
            },
            "mintExternal" => {
                let action = market
                    .mintExternal(
                        event.journal.clone(),
                        event.seal.clone(),
                        event.amount.clone(),
                        event.receiver,
                    )
                    .from(address!("2693946791da99dA78Ac441abA6D5Ce2Bccd96D3"));

                info!("Broadcasting mintExternal transaction");
                let pending_tx = action.send().await
                    .wrap_err("Failed to send mintExternal transaction")?;
                info!("Sent tx {}", pending_tx.tx_hash());

                let tx_hash = pending_tx
                    .with_timeout(Some(TX_TIMEOUT))
                    .watch()
                    .await
                    .wrap_err("Failed to confirm mintExternal transaction")?;

                // Add validation
                self.validate_transaction_receipt(provider, tx_hash, event).await?;
                tx_hash
            },
            "repayExternal" => {
                let action = market
                    .repayExternal(
                        event.journal.clone(),
                        event.seal.clone(),
                        event.amount.clone(),
                        event.receiver,
                    );

                info!("Broadcasting repayExternal transaction");
                let pending_tx = action.send().await
                    .wrap_err("Failed to send repayExternal transaction")?;
                info!("Sent tx {}", pending_tx.tx_hash());

                let tx_hash = pending_tx
                    .with_timeout(Some(TX_TIMEOUT))
                    .watch()
                    .await
                    .wrap_err("Failed to confirm repayExternal transaction")?;

                // Add validation
                self.validate_transaction_receipt(provider, tx_hash, event).await?;
                tx_hash
            },
            _ => return Err(eyre::eyre!("Invalid method: {}", event.method)),
        };

        info!("Transaction {:?} confirmed and validated", tx_hash);
        Ok(tx_hash)
    }

    async fn process_transaction(&self, event: ProofReadyEvent) -> Result<TxHash> {
        info!(
            "Processing transaction for market {:?}, method: {}, chain: {}", 
            event.market, event.method, event.dst_chain_id
        );

        let provider = self.get_provider_for_chain(event.dst_chain_id).await?;
        self.submit_with_retry(&provider, &event).await
    }
} 