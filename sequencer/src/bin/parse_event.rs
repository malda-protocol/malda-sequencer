use alloy::primitives::TxHash;
use eyre::Result;
use sequencer::database::Database;
use std::{env, str::FromStr};
use sqlx::query;

#[tokio::main]
async fn main() -> Result<()> {
    // Get database URL from environment or use default
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/sequencer".to_string()
    });

    // Initialize database connection
    let db = Database::new(&database_url).await?;

    // Get all processed events
    let processed_events = db.get_processed_events().await?;
    
    println!("Found {} processed events:", processed_events.len());
    
    for (i, event) in processed_events.iter().enumerate() {
        println!("\nEvent #{}:", i + 1);
        println!("Transaction Hash: {}", event.tx_hash);
        println!("Status: {:?}", event.status);
        println!("Event Type: {:?}", event.event_type);
        println!("Source Chain ID: {:?}", event.src_chain_id);
        println!("Destination Chain ID: {:?}", event.dst_chain_id);
        println!("Message Sender: {:?}", event.msg_sender);
        println!("Amount: {:?}", event.amount);
        println!("Target Function: {:?}", event.target_function);
        println!("Market: {:?}", event.market);
        println!("Journal Index: {:?}", event.journal_index);
        println!("Batch Transaction Hash: {:?}", event.batch_tx_hash);
        println!("Received At: {:?}", event.received_at);
        println!("Processed At: {:?}", event.processed_at);
        println!("Proof Requested At: {:?}", event.proof_requested_at);
        println!("Proof Received At: {:?}", event.proof_received_at);
        println!("Batch Submitted At: {:?}", event.batch_submitted_at);
        println!("Batch Included At: {:?}", event.batch_included_at);
        println!("Transaction Finished At: {:?}", event.tx_finished_at);
        println!("Resubmitted: {:?}", event.resubmitted);
        println!("Error: {:?}", event.error);
    }

    Ok(())
} 