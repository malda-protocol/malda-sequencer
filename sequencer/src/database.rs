use sqlx::{Pool, Postgres, query};
use eyre::Result;
use alloy::primitives::{Address, TxHash, U256};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventStatus {
    Received,
    Processed,
    IncludedInBatch,
    ProofRequested,
    ProofReceived,
    BatchSubmitStarted,
    BatchSubmitted,
    Failed { error: String },
}

// Add Default implementation for EventStatus
impl Default for EventStatus {
    fn default() -> Self {
        EventStatus::Received  // Default status is Received
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventUpdate {
    pub tx_hash: TxHash,
    pub event_type: Option<String>,
    pub src_chain_id: Option<u32>,
    pub dst_chain_id: Option<u32>,
    pub msg_sender: Option<Address>,
    pub amount: Option<U256>,
    pub batch_id: Option<String>,
    pub proof_data: Option<Vec<u8>>,
    pub proof_index: Option<i32>,
    pub batch_tx_hash: Option<String>,
    pub status: EventStatus,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Database {
    pool: Pool<Postgres>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect(database_url).await?;
        
        // Use the imported query function
        query(
            r#"
            CREATE TABLE IF NOT EXISTS events (
                tx_hash TEXT PRIMARY KEY,
                event_type TEXT,
                src_chain_id INTEGER,
                dst_chain_id INTEGER,
                msg_sender TEXT,
                amount TEXT,
                status JSONB NOT NULL,
                received_at TIMESTAMP WITH TIME ZONE,
                processed_at TIMESTAMP WITH TIME ZONE,
                included_in_batch_at TIMESTAMP WITH TIME ZONE,
                proof_requested_at TIMESTAMP WITH TIME ZONE,
                proof_received_at TIMESTAMP WITH TIME ZONE,
                batch_submit_started_at TIMESTAMP WITH TIME ZONE,
                batch_submitted_at TIMESTAMP WITH TIME ZONE,
                batch_id TEXT,
                proof_data BYTEA,
                proof_index INTEGER,
                batch_tx_hash TEXT,
                error TEXT,
                created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW()
            )"#
        )
        .execute(&pool)
        .await?;

        // Verify table was created
        sqlx::query("SELECT 1 FROM events LIMIT 1")
            .execute(&pool)
            .await?;

        info!("Database initialized successfully");
        
        Ok(Self { pool })
    }

    pub async fn update_event(&self, update: EventUpdate) -> Result<()> {
        // Clone the values we need for logging before they're moved
        let batch_id = update.batch_id.clone();
        let batch_tx_hash = update.batch_tx_hash.clone();
        let status_json = serde_json::to_value(&update.status)?;
        
        query(
            r#"
            INSERT INTO events (
                tx_hash, event_type, src_chain_id, dst_chain_id, 
                msg_sender, amount, status, 
                received_at, processed_at, included_in_batch_at,
                proof_requested_at, proof_received_at,
                batch_submit_started_at, batch_submitted_at,
                batch_id, proof_data, proof_index, batch_tx_hash,
                error, created_at, updated_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7,
                CASE WHEN $8::bool THEN NOW() ELSE NULL END,
                CASE WHEN $9::bool THEN NOW() ELSE NULL END,
                CASE WHEN $10::bool THEN NOW() ELSE NULL END,
                CASE WHEN $11::bool THEN NOW() ELSE NULL END,
                CASE WHEN $12::bool THEN NOW() ELSE NULL END,
                CASE WHEN $13::bool THEN NOW() ELSE NULL END,
                CASE WHEN $14::bool THEN NOW() ELSE NULL END,
                $15, $16, $17, $18, $19, NOW(), NOW()
            )
            ON CONFLICT (tx_hash) 
            DO UPDATE SET
                status = $7,
                processed_at = CASE WHEN $9::bool THEN NOW() ELSE events.processed_at END,
                included_in_batch_at = CASE WHEN $10::bool THEN NOW() ELSE events.included_in_batch_at END,
                proof_requested_at = CASE WHEN $11::bool THEN NOW() ELSE events.proof_requested_at END,
                proof_received_at = CASE WHEN $12::bool THEN NOW() ELSE events.proof_received_at END,
                batch_submit_started_at = CASE WHEN $13::bool THEN NOW() ELSE events.batch_submit_started_at END,
                batch_submitted_at = CASE WHEN $14::bool THEN NOW() ELSE events.batch_submitted_at END,
                batch_id = COALESCE($15, events.batch_id),
                proof_data = COALESCE($16, events.proof_data),
                proof_index = COALESCE($17, events.proof_index),
                batch_tx_hash = COALESCE($18, events.batch_tx_hash),
                error = COALESCE($19, events.error),
                updated_at = NOW()
            "#,
        )
        .bind(update.tx_hash.to_string())
        .bind(update.event_type)
        .bind(update.src_chain_id.map(|id| id as i32))
        .bind(update.dst_chain_id.map(|id| id as i32))
        .bind(update.msg_sender.map(|addr| addr.to_string()))
        .bind(update.amount.map(|amt| amt.to_string()))
        .bind(status_json)
        // Timestamp flags based on status
        .bind(matches!(update.status, EventStatus::Received))
        .bind(matches!(update.status, EventStatus::Processed))
        .bind(matches!(update.status, EventStatus::IncludedInBatch))
        .bind(matches!(update.status, EventStatus::ProofRequested))
        .bind(matches!(update.status, EventStatus::ProofReceived))
        .bind(matches!(update.status, EventStatus::BatchSubmitStarted))
        .bind(matches!(update.status, EventStatus::BatchSubmitted))
        // Other fields
        .bind(update.batch_id)
        .bind(update.proof_data)
        .bind(update.proof_index)
        .bind(update.batch_tx_hash)
        .bind(match update.status {
            EventStatus::Failed { ref error } => Some(error),
            _ => None,
        })
        .execute(&self.pool)
        .await?;

        // Use the cloned values for logging
        info!(
            "Updated event {} status to {:?}, batch_id: {:?}, tx_hash: {:?}",
            update.tx_hash,
            update.status,
            batch_id,
            batch_tx_hash
        );
        Ok(())
    }

    pub async fn get_event_status(&self, tx_hash: &TxHash) -> Result<Option<EventStatus>> {
        // Use query_as instead of query! macro
        let record = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT status FROM events WHERE tx_hash = $1"
        )
        .bind(tx_hash.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match record {
            Some((status,)) => Ok(Some(serde_json::from_value(status)?)),
            None => Ok(None),
        }
    }
} 