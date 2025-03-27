use alloy::primitives::TxHash;
use eyre::Result;
use sequencer::database::Database;
use std::{env, str::FromStr};
use sqlx::query;
use sqlx::Row;

#[tokio::main]
async fn main() -> Result<()> {
    // Get database URL from environment or use default
    let database_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
        "postgres://postgres:postgres@localhost:5432/sequencer".to_string()
    });

    // Initialize database connection
    let db = Database::new(&database_url).await?;

    // Get the first event from the database
    let record = query(
        r#"
        SELECT tx_hash 
        FROM events 
        LIMIT 1
        "#
    )
    .fetch_optional(&db.pool)
    .await?;

    match record {
        Some(row) => {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;

            // Get the full event
            let event = db.get_event(&tx_hash).await?;
            
            match event {
                Some(event_update) => {
                    println!("Successfully parsed event:");
                    println!("Transaction Hash: {}", event_update.tx_hash);
                    println!("Status: {:?}", event_update.status);
                    println!("Event Type: {:?}", event_update.event_type);
                    println!("Source Chain ID: {:?}", event_update.src_chain_id);
                    println!("Destination Chain ID: {:?}", event_update.dst_chain_id);
                    println!("Message Sender: {:?}", event_update.msg_sender);
                    println!("Amount: {:?}", event_update.amount);
                    println!("Target Function: {:?}", event_update.target_function);
                    println!("Market: {:?}", event_update.market);
                    println!("Journal Index: {:?}", event_update.journal_index);
                    println!("Journal: {:?}", event_update.journal);
                    println!("Seal: {:?}", event_update.seal);
                    println!("Batch Transaction Hash: {:?}", event_update.batch_tx_hash);
                    println!("Received At: {:?}", event_update.received_at);
                    println!("Processed At: {:?}", event_update.processed_at);
                    println!("Proof Requested At: {:?}", event_update.proof_requested_at);
                    println!("Proof Received At: {:?}", event_update.proof_received_at);
                    println!("Batch Submitted At: {:?}", event_update.batch_submitted_at);
                    println!("Batch Included At: {:?}", event_update.batch_included_at);
                    println!("Transaction Finished At: {:?}", event_update.tx_finished_at);
                    println!("Resubmitted: {:?}", event_update.resubmitted);
                    println!("Error: {:?}", event_update.error);
                }
                None => println!("No event found for transaction hash: {}", tx_hash),
            }
        }
        None => println!("No events found in the database"),
    }

    Ok(())
} 