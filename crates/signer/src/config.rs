//! Configuration types for creating signers.

use crate::error::SignerError;
use crate::service::SignerService;

#[cfg(feature = "local")]
pub use crate::local::LocalConfig;

#[cfg(feature = "aws")]
pub use crate::aws::AwsConfig;

/// Configuration for creating a signer.
///
/// Use this enum to create a [`SignerService`] from configuration.
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
pub enum SignerConfig {
    /// Local private key signer (for development).
    #[cfg(feature = "local")]
    Local(LocalConfig),

    /// AWS KMS signer (for production).
    #[cfg(feature = "aws")]
    Aws(AwsConfig),
}

impl SignerConfig {
    /// Build a [`SignerService`] from this configuration.
    ///
    /// # Errors
    ///
    /// Returns [`SignerError::Initialization`] if the signer cannot be created.
    pub async fn build(self) -> Result<SignerService, SignerError> {
        match self {
            #[cfg(feature = "local")]
            SignerConfig::Local(config) => config.build().await,

            #[cfg(feature = "aws")]
            SignerConfig::Aws(config) => config.build().await,
        }
    }
}
