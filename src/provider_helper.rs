// Copyright (c) 2026 Merge Layers Inc.
//
// This source code is licensed under the Business Source License 1.1
// (the "License"); you may not use this file except in compliance with the
// License. You may obtain a copy of the License at
//
//     https://github.com/malda-protocol/malda-sequencer/blob/main/LICENSE-BSL.md
//
// See the License for the specific language governing permissions and
// limitations under the License.

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
//! - **Multi-Protocol Support**: HTTP and WebSocket connections

use alloy::{
    eips::BlockNumberOrTag,
    network::EthereumWallet,
    providers::{Provider, ProviderBuilder, WsConnect},
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
    /// Whether to use WebSocket connections (true) or HTTP connections (false)
    pub use_websocket: bool,
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
    /// 2. If cached not fresh, tries primary provider
    /// 3. If primary is fresh, uses it and caches it
    /// 4. If primary not fresh, uses fallback (but doesn't cache it)
    /// 5. Cached provider is always the primary, never the fallback
    ///
    /// # Returns
    /// * `Result<(ProviderType, bool)>` - Provider and whether fallback was used
    pub async fn get_fresh_provider(&mut self) -> Result<(ProviderType, bool)> {
        // Step 1: Try cached provider if available
        if let Some(cached_provider) = &self.cached_primary {
            if Self::is_provider_fresh(cached_provider, self.config.max_block_delay_secs).await {
                debug!(
                    "Reusing cached primary provider for chain {}",
                    self.config.chain_id
                );
                return Ok((cached_provider.clone(), false));
            } else {
                debug!(
                    "Cached primary provider for chain {} is stale, clearing cache",
                    self.config.chain_id
                );
                self.cached_primary = None;
            }
        }

        // Step 2: Try primary provider
        match Self::create_provider(&self.config.primary_url, self.config.use_websocket).await {
            Ok(provider) => {
                if Self::is_provider_fresh(&provider, self.config.max_block_delay_secs).await {
                    debug!(
                        "Primary provider for chain {} is fresh, caching and using it",
                        self.config.chain_id
                    );
                    self.cached_primary = Some(provider.clone());
                    self.used_fallback = false;
                    return Ok((provider, false));
                } else {
                    warn!(
                        "Primary provider for chain {} is not fresh, trying fallback",
                        self.config.chain_id
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to create primary provider for chain {}: {:?}, trying fallback",
                    self.config.chain_id, e
                );
            }
        }

        // Step 3: Try fallback provider (never cache it)
        match Self::create_provider(&self.config.fallback_url, self.config.use_websocket).await {
            Ok(fallback_provider) => {
                if Self::is_provider_fresh(&fallback_provider, self.config.max_block_delay_secs).await {
                    info!("Using fallback provider for chain {}", self.config.chain_id);
                    // Don't cache fallback provider - keep primary cache clear
                    self.used_fallback = true;
                    return Ok((fallback_provider, true));
                } else {
                    warn!(
                        "Fallback provider for chain {} is also not fresh",
                        self.config.chain_id
                    );
                }
            }
            Err(e) => {
                warn!(
                    "Failed to create fallback provider for chain {}: {:?}",
                    self.config.chain_id, e
                );
            }
        }

        Err(eyre!(
            "Both primary and fallback providers are unavailable for chain {}",
            self.config.chain_id
        ))
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
    pub async fn is_provider_fresh(provider: &ProviderType, max_delay: u64) -> bool {
        match provider.get_block_by_number(BlockNumberOrTag::Latest).await {
            Ok(Some(block)) => {
                let block_timestamp: u64 = block.header.inner.timestamp.into();
                let current_time = chrono::Utc::now().timestamp() as u64;

                // Provider is fresh if block time is current or within max delay
                // Also allow for slight time drift (up to 60 seconds ahead)
                let time_diff = if current_time > block_timestamp {
                    current_time - block_timestamp
                } else {
                    block_timestamp - current_time
                };
                
                time_diff <= max_delay || time_diff <= 60 // Allow 60 seconds of time drift
            }
            Ok(None) => {
                warn!("Provider returned None for latest block");
                false
            }
            Err(e) => {
                warn!("Failed to get latest block from provider: {:?}", e);
                false
            }
        }
    }

    /// Creates a new provider connection to the specified URL
    ///
    /// This method creates a new connection to the specified URL using either
    /// HTTP or WebSocket protocol based on the configuration. It configures
    /// the provider with all necessary fillers for blockchain interaction.
    ///
    /// # Arguments
    /// * `url` - RPC URL to connect to
    /// * `use_websocket` - Whether to use WebSocket (true) or HTTP (false) connection
    ///
    /// # Returns
    /// * `Result<ProviderType>` - Connected provider
    pub async fn create_provider(url: &str, use_websocket: bool) -> Result<ProviderType> {
        // Use a dummy private key since we only need view calls
        let dummy_key = "0000000000000000000000000000000000000000000000000000000000000001";
        let signer: PrivateKeySigner = dummy_key.parse().expect("should parse private key");
        let wallet = EthereumWallet::from(signer);

        if use_websocket {
            // Create WebSocket connection
            let ws_url: Url = url
                .parse()
                .wrap_err_with(|| format!("Invalid WebSocket URL: {}", url))?;

            let ws_connect = WsConnect::new(ws_url);
            ProviderBuilder::new()
                .wallet(wallet)
                .connect_ws(ws_connect)
                .await
                .wrap_err("Failed to connect to WebSocket")
        } else {
            // Create HTTP connection
            let rpc_url: Url = url
                .parse()
                .wrap_err_with(|| format!("Invalid HTTP URL: {}", url))?;

            Ok(ProviderBuilder::new().wallet(wallet).connect_http(rpc_url))
        }
    }
}
