//! # Provider Helper Module
//! 
//! This module provides common provider management functionality that can be shared
//! across different components that need to interact with blockchain providers.
//! 
//! ## Features:
//! - **Provider Creation**: Standardized provider creation with all necessary fillers
//! - **Provider Freshness Validation**: Check if providers are up-to-date
//! - **Primary/Fallback Logic**: Automatic fallback when primary provider fails
//! - **Connection Management**: Efficient provider connection handling

use alloy::{
    eips::BlockNumberOrTag,
    network::EthereumWallet,
    providers::{Provider, ProviderBuilder},
    signers::local::PrivateKeySigner,
    transports::http::reqwest::Url,
};
use eyre::{eyre, Result, WrapErr};
use tracing::{debug, info, warn};

/// Standard provider type with all necessary fillers for blockchain interaction
/// 
/// This type includes:
/// - Identity filler for basic provider functionality
/// - Gas filler for transaction gas estimation
/// - Blob gas filler for EIP-4844 support
/// - Nonce filler for transaction nonce management
/// - Chain ID filler for network identification
/// - Wallet filler for signing capabilities
pub type ProviderType = alloy::providers::fillers::FillProvider<
    alloy::providers::fillers::JoinFill<
        alloy::providers::fillers::JoinFill<
            alloy::providers::Identity,
            alloy::providers::fillers::JoinFill<
                alloy::providers::fillers::GasFiller,
                alloy::providers::fillers::JoinFill<
                    alloy::providers::fillers::BlobGasFiller,
                    alloy::providers::fillers::JoinFill<
                        alloy::providers::fillers::NonceFiller,
                        alloy::providers::fillers::ChainIdFiller,
                    >,
                >,
            >,
        >,
        alloy::providers::fillers::WalletFiller<alloy::network::EthereumWallet>,
    >,
    alloy::providers::RootProvider<alloy::network::Ethereum>,
>;

/// Configuration for provider management
/// 
/// This struct contains the necessary parameters for managing
/// primary and fallback providers with freshness validation.
#[derive(Clone)]
pub struct ProviderConfig {
    /// Primary RPC URL for blockchain connection
    pub primary_url: String,
    /// Fallback RPC URL in case primary fails
    pub fallback_url: String,
    /// Maximum allowed delay for block freshness in seconds
    pub max_block_delay_secs: u64,
    /// Chain ID for error context and logging
    pub chain_id: u64,
}

/// Manages provider state for a single chain
/// 
/// This struct encapsulates the provider management state including
/// cached providers and fallback usage tracking.
pub struct ProviderState {
    /// Cached primary provider connection
    cached_primary: Option<ProviderType>,
    /// Whether fallback was used in the last attempt
    used_fallback: bool,
    /// Provider configuration
    config: ProviderConfig,
}

impl ProviderState {
    /// Creates a new provider state with the given configuration
    /// 
    /// # Arguments
    /// * `config` - Provider configuration
    /// 
    /// # Returns
    /// A new ProviderState instance
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            cached_primary: None,
            used_fallback: false,
            config,
        }
    }

    /// Gets a fresh provider connection with caching and fallback logic
    /// 
    /// This method implements the complete provider selection logic:
    /// 1. Checks if cached primary provider is still fresh
    /// 2. Reuses cached provider if valid
    /// 3. Creates new primary provider if needed
    /// 4. Falls back to secondary provider if primary fails
    /// 5. Updates cache and state accordingly
    /// 
    /// # Returns
    /// * `Result<(ProviderType, bool)>` - Provider and whether fallback was used
    pub async fn get_fresh_provider(&mut self) -> Result<(ProviderType, bool)> {
        // Try to reuse cached primary provider if fresh
        if !self.used_fallback && self.cached_primary.is_some() {
            let provider = self.cached_primary.as_ref().unwrap();
            if Self::is_provider_fresh(provider, self.config.max_block_delay_secs).await {
                debug!("Reusing cached primary provider for chain {}", self.config.chain_id);
                return Ok((provider.clone(), false));
            }
        }

        // Try primary provider
        match Self::create_provider(&self.config.primary_url).await {
            Ok(provider) => {
                if Self::is_provider_fresh(&provider, self.config.max_block_delay_secs).await {
                    debug!("Using new primary provider for chain {}", self.config.chain_id);
                    self.cached_primary = Some(provider.clone());
                    self.used_fallback = false;
                    return Ok((provider, false));
                } else {
                    warn!("Primary provider for chain {} is not fresh, trying fallback", self.config.chain_id);
                }
            }
            Err(e) => {
                warn!("Failed to create primary provider for chain {}: {:?}, trying fallback", self.config.chain_id, e);
            }
        }

        // Try fallback provider
        let fallback_provider = Self::create_provider(&self.config.fallback_url).await?;
        if Self::is_provider_fresh(&fallback_provider, self.config.max_block_delay_secs).await {
            info!("Using fallback provider for chain {}", self.config.chain_id);
            self.cached_primary = None; // Clear primary cache when using fallback
            self.used_fallback = true;
            return Ok((fallback_provider, true));
        }

        Err(eyre!("Both primary and fallback providers are unavailable for chain {}", self.config.chain_id))
    }

    /// Validates if a provider is fresh (not too far behind current time)
    /// 
    /// This method checks if the provider's latest block is within the acceptable
    /// delay window. This prevents using stale providers that might be out of sync.
    /// 
    /// # Arguments
    /// * `provider` - Provider to validate
    /// * `max_delay` - Maximum allowed delay in seconds
    /// 
    /// # Returns
    /// * `bool` - True if provider is fresh, false otherwise
    async fn is_provider_fresh(provider: &ProviderType, max_delay: u64) -> bool {
        match provider.get_block_by_number(BlockNumberOrTag::Latest).await {
            Ok(Some(block)) => {
                let block_timestamp: u64 = block.header.inner.timestamp.into();
                let current_time = chrono::Utc::now().timestamp() as u64;
                
                // Provider is fresh if block time is current or within max delay
                current_time <= block_timestamp || 
                (current_time - block_timestamp) <= max_delay
            }
            _ => false
        }
    }

    /// Creates a new provider connection to the specified URL
    /// 
    /// This method creates a new HTTP connection to the specified URL
    /// and configures it with all necessary fillers for blockchain interaction.
    /// 
    /// # Arguments
    /// * `url` - RPC URL to connect to
    /// 
    /// # Returns
    /// * `Result<ProviderType>` - Connected provider
    async fn create_provider(url: &str) -> Result<ProviderType> {
        let rpc_url: Url = url.parse()
            .wrap_err_with(|| format!("Invalid RPC URL: {}", url))?;
        
        // Use a dummy private key since we only need view calls
        let dummy_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let signer: PrivateKeySigner = dummy_key.parse().expect("should parse private key");
        let wallet = EthereumWallet::from(signer);

        Ok(ProviderBuilder::new()
            .wallet(wallet)
            .connect_http(rpc_url))
    }
} 