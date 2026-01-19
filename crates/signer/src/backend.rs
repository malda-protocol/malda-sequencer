//! Backend type definitions for different signer implementations.

use alloy_primitives::{Address, B256, ChainId};
use alloy_signer::{Signature, Signer};
use async_trait::async_trait;

#[cfg(feature = "local")]
pub use alloy_signer_local::PrivateKeySigner;

#[cfg(feature = "aws")]
pub use alloy_signer_aws::AwsSigner;

/// Unified signer backend supporting multiple implementations.
///
/// The available variants depend on enabled features:
/// - `local`: [`SignerBackend::Local`] using [`PrivateKeySigner`]
/// - `aws`: [`SignerBackend::Aws`] using [`AwsSigner`]
pub enum SignerBackend {
    /// Local private key signer (for development).
    #[cfg(feature = "local")]
    Local(PrivateKeySigner),

    /// AWS KMS signer (for production).
    #[cfg(feature = "aws")]
    Aws(AwsSigner),
}

// Compile-time check that at least one backend is enabled.
#[cfg(not(any(feature = "aws", feature = "local")))]
compile_error!("At least one of 'aws' or 'local' features must be enabled");

#[async_trait]
impl Signer for SignerBackend {
    async fn sign_hash(&self, hash: &B256) -> Result<Signature, alloy_signer::Error> {
        match self {
            #[cfg(feature = "local")]
            Self::Local(s) => s.sign_hash(hash).await,
            #[cfg(feature = "aws")]
            Self::Aws(s) => s.sign_hash(hash).await,
        }
    }

    fn address(&self) -> Address {
        match self {
            #[cfg(feature = "local")]
            Self::Local(s) => s.address(),
            #[cfg(feature = "aws")]
            Self::Aws(s) => s.address(),
        }
    }

    fn chain_id(&self) -> Option<ChainId> {
        match self {
            #[cfg(feature = "local")]
            Self::Local(s) => s.chain_id(),
            #[cfg(feature = "aws")]
            Self::Aws(s) => s.chain_id(),
        }
    }

    fn set_chain_id(&mut self, chain_id: Option<ChainId>) {
        match self {
            #[cfg(feature = "local")]
            Self::Local(s) => s.set_chain_id(chain_id),
            #[cfg(feature = "aws")]
            Self::Aws(s) => s.set_chain_id(chain_id),
        }
    }
}

#[cfg(feature = "local")]
impl From<PrivateKeySigner> for SignerBackend {
    fn from(signer: PrivateKeySigner) -> Self {
        Self::Local(signer)
    }
}

#[cfg(feature = "aws")]
impl From<AwsSigner> for SignerBackend {
    fn from(signer: AwsSigner) -> Self {
        Self::Aws(signer)
    }
}
