//! Error types for the signer library.

use thiserror::Error;

/// Errors that can occur during signing operations.
#[derive(Error, Debug)]
pub enum SignerError {
    /// The signer actor was dropped unexpectedly.
    #[error("signer actor was dropped")]
    ActorDropped,

    /// Failed to initialize the signer backend.
    #[error("failed to initialize signer: {0}")]
    Initialization(String),

    /// Signing operation failed.
    #[error("signing failed: {0}")]
    Signing(#[from] alloy_signer::Error),
}
