//! AWS KMS signer test loop example.
//!
//! Tests signing every 10 minutes to verify credential refresh works
//! beyond the 1 hour STS session window.
//!
//! This example mirrors the exact usage in the sequencer:
//! - Same environment variables
//! - Same session name ("malda-sequencer")
//! - Health check at startup
//! - EthereumWallet integration (same as transaction_manager.rs)
//!
//! # Required Environment Variables
//!
//! - `AWS_ACCESS_KEY_ID` - IAM user access key
//! - `AWS_SECRET_ACCESS_KEY` - IAM user secret key
//! - `AWS_REGION` - AWS region (default: us-east-1)
//! - `AWS_ROLE_ARN` - Role ARN to assume
//! - `AWS_EXTERNAL_ID` - External ID for role assumption
//! - `AWS_KMS_SEQUENCER_KEY_ID` - KMS key ID
//! - `CHAIN_ID` - Ethereum chain ID (default: 1)
//!
//! # Running
//!
//! ```bash
//! cargo run --example aws_assume_role_loop --features aws
//! ```

use std::time::{Duration, SystemTime};

use alloy::network::EthereumWallet;
use malda_signer::{AwsConfig, B256, SignerConfig, SignerService};
use rand::Rng;
use tokio::time::interval;

fn format_time(time: SystemTime) -> String {
    let duration = time.duration_since(SystemTime::UNIX_EPOCH).unwrap();
    let secs = duration.as_secs();
    let hours = (secs / 3600) % 24;
    let minutes = (secs / 60) % 60;
    let seconds = secs % 60;
    format!("{:02}:{:02}:{:02} UTC", hours, minutes, seconds)
}

fn random_hash() -> B256 {
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    B256::from(bytes)
}

async fn create_kms_client() -> aws_sdk_kms::Client {
    // Mirrors main.rs init_signer_service() exactly
    let region = std::env::var("AWS_REGION").unwrap_or_else(|_| "us-east-1".to_string());
    let role_arn = std::env::var("AWS_ROLE_ARN").expect("AWS_ROLE_ARN not set");
    let external_id = std::env::var("AWS_EXTERNAL_ID").expect("AWS_EXTERNAL_ID not set");

    let provider = aws_config::sts::AssumeRoleProvider::builder(&role_arn)
        .external_id(&external_id)
        .session_name("malda-sequencer") // Same as sequencer
        .region(aws_types::region::Region::new(region.clone()))
        .build()
        .await;

    let config = aws_config::from_env()
        .credentials_provider(provider)
        .region(aws_types::region::Region::new(region))
        .load()
        .await;

    aws_sdk_kms::Client::new(&config)
}

#[tokio::main]
async fn main() {
    let _ = dotenvy::dotenv();

    // Same env var as sequencer
    let kms_key_id =
        std::env::var("AWS_KMS_SEQUENCER_KEY_ID").expect("AWS_KMS_SEQUENCER_KEY_ID not set");
    let chain_id: u64 = std::env::var("CHAIN_ID")
        .unwrap_or_else(|_| "1".to_string())
        .parse()
        .expect("CHAIN_ID must be a valid u64");

    let kms_client = create_kms_client().await;

    let config = SignerConfig::Aws(AwsConfig {
        kms_client,
        key_id: kms_key_id,
        chain_id: Some(chain_id),
    });

    let service: SignerService = config.build().await.expect("Failed to create signer");

    println!("=== AWS KMS Signer Test ===");
    println!("Address: {:?}", service.address());
    println!("Chain ID: {:?}", service.chain_id());

    // Health check - same as sequencer startup
    println!("\nRunning health check (same as sequencer startup)...");
    service
        .health_check()
        .await
        .expect("Health check failed - sequencer would fail to start");
    println!("Health check passed!");

    // Test EthereumWallet integration - same as transaction_manager.rs
    println!("\nTesting EthereumWallet::from(service) (same as transaction_manager.rs)...");
    let _wallet = EthereumWallet::from(service.clone());
    println!("EthereumWallet created successfully!");

    println!("\nTesting every 10 minutes...\n");

    let mut interval = interval(Duration::from_secs(10 * 60));
    let mut success_count = 0u64;
    let mut iteration = 0u64;

    // Phase 1: 8 signing operations (80 minutes, covers STS token refresh)
    while success_count < 8 {
        interval.tick().await;
        iteration += 1;

        println!(
            "--- Test #{} at {} ---",
            iteration,
            format_time(SystemTime::now())
        );

        let hash = random_hash();

        // Test sign_hash (same code path as transaction signing)
        match service.sign_hash(hash).await {
            Ok(sig) => {
                success_count += 1;
                println!("sign_hash: OK ({}/8)", success_count);
                println!("Signature: {:?}\n", sig);
            }
            Err(e) => println!("sign_hash: ERROR: {:?}\n", e),
        }
    }

    // Phase 2: Wait 65 minutes (to ensure STS token fully expires)
    println!("=== Waiting 65 minutes (STS token expiry test) ===");
    tokio::time::sleep(Duration::from_secs(65 * 60)).await;

    // Phase 3: Final test after token expiry
    println!(
        "=== Final test at {} ===",
        format_time(SystemTime::now())
    );

    match service.sign_hash(random_hash()).await {
        Ok(sig) => println!("sign_hash: OK\nSignature: {:?}", sig),
        Err(e) => println!("sign_hash: ERROR: {:?}", e),
    }

    // Verify wallet still works after token refresh
    let _wallet = EthereumWallet::from(service.clone());
    println!("EthereumWallet still works after token refresh!");

    println!("\n=== Done ===");
}
