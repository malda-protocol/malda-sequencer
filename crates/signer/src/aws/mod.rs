//! AWS KMS signer configuration.

use crate::error::SignerError;
use crate::service::SignerService;

/// Configuration for an AWS KMS signer.
///
/// Use this for production deployments. The KMS client should be pre-configured
/// with appropriate credentials (instance role, AssumeRole, etc.).
///
/// # Example
///
/// ```ignore
/// use malda_signer::{SignerConfig, AwsConfig};
///
/// // Create KMS client (SDK handles credentials)
/// let aws_config = aws_config::from_env().load().await;
/// let kms_client = aws_sdk_kms::Client::new(&aws_config);
///
/// let config = SignerConfig::Aws(AwsConfig {
///     kms_client,
///     key_id: "alias/my-signing-key".into(),
///     chain_id: Some(1),
/// });
/// let handle = config.build().await?;
/// ```
pub struct AwsConfig {
    /// Pre-configured KMS client.
    pub kms_client: aws_sdk_kms::Client,
    /// KMS key ID or alias.
    pub key_id: String,
    /// Optional chain ID.
    pub chain_id: Option<u64>,
}

impl AwsConfig {
    /// Build a [`SignerService`] from this configuration.
    pub async fn build(self) -> Result<SignerService, SignerError> {
        let signer =
            alloy_signer_aws::AwsSigner::new(self.kms_client, self.key_id, self.chain_id)
                .await
                .map_err(|e| SignerError::Initialization(e.to_string()))?;

        Ok(SignerService::new(signer))
    }
}
