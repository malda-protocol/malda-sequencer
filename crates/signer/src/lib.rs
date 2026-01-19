//! # malda-signer
//!
//! A flexible Ethereum signer library supporting both local private key
//! signing (for development) and AWS KMS signing (for production).
//!
//! ## Features
//!
//! - `local` (default): Enable local private key signing via [`alloy_signer_local::PrivateKeySigner`]
//! - `aws`: Enable AWS KMS signing via [`alloy_signer_aws::AwsSigner`]
//! - `full`: Enable both backends with runtime selection
//!
//! ## Quick Start
//!
//! ### Local Development
//!
//! ```ignore
//! use malda_signer::{SignerConfig, LocalConfig, SignerService};
//!
//! let config = SignerConfig::Local(LocalConfig {
//!     private_key: my_key,
//!     chain_id: Some(1),
//! });
//! let handle = config.build().await?;
//! let signature = handle.sign_hash(hash).await?;
//! ```
//!
//! ### AWS KMS
//!
//! ```ignore
//! use malda_signer::{SignerConfig, AwsConfig, SignerService};
//!
//! // Create KMS client - SDK handles credentials automatically
//! let aws_config = aws_config::from_env().load().await;
//! let kms_client = aws_sdk_kms::Client::new(&aws_config);
//!
//! let config = SignerConfig::Aws(AwsConfig {
//!     kms_client,
//!     key_id: "alias/my-signing-key".into(),
//!     chain_id: Some(1),
//! });
//! let handle = config.build().await?;
//! ```
//!
//! ## AWS Authentication
//!
//! The library takes a pre-configured `aws_sdk_kms::Client`. How you create
//! that client determines authentication:
//!
//! ### On AWS (EC2, ECS, Lambda, EKS) - Recommended
//!
//! ```ignore
//! // Instance/task role - no credentials needed
//! let config = aws_config::from_env().load().await;
//! let kms_client = aws_sdk_kms::Client::new(&config);
//! ```
//!
//! ### Local Development with AssumeRole
//!
//! ```ignore
//! // SDK's built-in AssumeRoleProvider handles refresh automatically
//! let provider = aws_config::sts::AssumeRoleProvider::builder("arn:aws:iam::123:role/MyRole")
//!     .external_id("my-external-id")
//!     .session_name("my-session")
//!     .build()
//!     .await;
//!
//! let config = aws_config::from_env()
//!     .credentials_provider(provider)
//!     .load()
//!     .await;
//!
//! let kms_client = aws_sdk_kms::Client::new(&config);
//! ```
//!
//! ## Thread Safety
//!
//! [`SignerService`] is `Clone` and can be shared across tasks. It uses an
//! actor pattern internally to serialize signing operations.

mod backend;
mod config;
mod error;
mod service;

#[cfg(feature = "local")]
mod local;
#[cfg(feature = "aws")]
mod aws;

// Re-export main types.
pub use config::SignerConfig;
pub use error::SignerError;
pub use service::SignerService;

#[cfg(feature = "local")]
pub use config::LocalConfig;

#[cfg(feature = "aws")]
pub use config::AwsConfig;

// Re-export commonly used alloy types for convenience.
pub use alloy_primitives::{Address, B256};
pub use alloy_signer::Signature;

// Re-export backend types.
pub use backend::SignerBackend;

#[cfg(feature = "local")]
pub use alloy_signer_local::PrivateKeySigner;

#[cfg(feature = "aws")]
pub use alloy_signer_aws::AwsSigner;

// Re-export AWS SDK crates for client creation.
#[cfg(feature = "aws")]
pub use aws_config;
#[cfg(feature = "aws")]
pub use aws_sdk_kms;
#[cfg(feature = "aws")]
pub use aws_types;
