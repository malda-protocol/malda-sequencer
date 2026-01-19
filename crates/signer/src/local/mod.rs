//! Local private key signer configuration.

use alloy_signer::Signer;

use crate::error::SignerError;
use crate::service::SignerService;

/// Configuration for a local private key signer.
///
/// Use this for development and testing. For production, use [`super::aws::AwsConfig`].
///
/// # Example
///
/// ```ignore
/// use malda_signer::{SignerConfig, LocalConfig};
///
/// let config = SignerConfig::Local(LocalConfig {
///     private_key: key,
///     chain_id: Some(1),
/// });
/// let handle = config.build().await?;
/// ```
pub struct LocalConfig {
    /// Private key bytes (32 bytes).
    pub private_key: alloy_primitives::B256,
    /// Optional chain ID.
    pub chain_id: Option<u64>,
}

impl LocalConfig {
    /// Build a [`SignerService`] from this configuration.
    pub async fn build(self) -> Result<SignerService, SignerError> {
        let mut signer = alloy_signer_local::PrivateKeySigner::from_bytes(&self.private_key)
            .map_err(|e| SignerError::Initialization(e.to_string()))?;

        if let Some(chain_id) = self.chain_id {
            signer.set_chain_id(Some(chain_id));
        }

        Ok(SignerService::new(signer))
    }
}
