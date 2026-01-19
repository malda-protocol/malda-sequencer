//! Minimal AWS KMS signer example using instance/task roles.
//!
//! This example shows the simplest setup when running on AWS infrastructure
//! (EC2, ECS, Lambda, EKS) where credentials come from the instance/task role.
//!
//! # Prerequisites
//!
//! 1. Deploy to AWS with an IAM role attached (instance role, task role, etc.)
//! 2. The role must have `kms:Sign` and `kms:GetPublicKey` permissions
//! 3. Set environment variables:
//!    - `AWS_KMS_KEY_ID`: Your KMS key ID or ARN
//!    - `CHAIN_ID`: Ethereum chain ID (optional, defaults to 1)
//!
//! # Running
//!
//! ```bash
//! cargo run --example aws_simple --features aws
//! ```
//!
//! For AssumeRole with explicit credentials, see `aws_assume_role_loop.rs` example.

use malda_signer::{AwsConfig, B256, SignerConfig, SignerService};

#[tokio::main]
async fn main() {
    // === KMS Client Creation: Just 2 lines ===
    let config = aws_config::from_env().load().await;
    let kms_client = aws_sdk_kms::Client::new(&config);
    // =========================================

    // Read configuration.
    let key_id = std::env::var("AWS_KMS_KEY_ID").expect("AWS_KMS_KEY_ID not set");
    let chain_id: u64 = std::env::var("CHAIN_ID")
        .unwrap_or_else(|_| "1".to_string())
        .parse()
        .expect("CHAIN_ID must be a valid u64");

    // Create signer.
    let config = SignerConfig::Aws(AwsConfig {
        kms_client,
        key_id,
        chain_id: Some(chain_id),
    });

    let handle: SignerService = config.build().await.expect("Failed to create signer");

    println!("AWS KMS Signer (Simple Mode)");
    println!("Address: {:?}", handle.address());
    println!("Chain ID: {:?}", handle.chain_id());

    // Sign a test hash.
    let hash = B256::repeat_byte(0x42);
    println!("\nSigning hash: {:?}", hash);

    match handle.sign_hash(hash).await {
        Ok(signature) => {
            println!("Success!");
            println!("Signature: {:?}", signature);
        }
        Err(e) => {
            eprintln!("Error: {:?}", e);
        }
    }
}
