use sqlx::{Pool, Postgres, query};
use eyre::Result;
use alloy::primitives::{Address, TxHash, U256, Bytes};
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventStatus {
    Received,
    Processed,
    IncludedInBatch,
    ProofRequested,
    ProofReceived,
    BatchSubmitted,
    BatchIncluded,
    BatchFailed { error: String },
    TxProcessSuccess,
    TxProcessFail,
    Failed { error: String },
}

impl EventStatus {
    // Convert to database string representation
    fn to_db_string(&self) -> String {
        match self {
            EventStatus::Received => "Received",
            EventStatus::Processed => "Processed",
            EventStatus::IncludedInBatch => "IncludedInBatch",
            EventStatus::ProofRequested => "ProofRequested",
            EventStatus::ProofReceived => "ProofReceived",
            EventStatus::BatchSubmitted => "BatchSubmitted",
            EventStatus::BatchIncluded => "BatchIncluded",
            EventStatus::BatchFailed { .. } => "BatchFailed",
            EventStatus::TxProcessSuccess => "TxProcessSuccess",
            EventStatus::TxProcessFail => "TxProcessFail",
            EventStatus::Failed { .. } => "Failed",
        }.to_string()
    }
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
    pub target_function: Option<String>,
    pub market: Option<Address>,
    pub journal_index: Option<i32>,
    pub journal: Option<Bytes>,
    pub seal: Option<Bytes>,
    pub batch_tx_hash: Option<String>,
    pub status: EventStatus,
    pub resubmitted: Option<i32>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Database {
    pool: Pool<Postgres>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect(database_url).await?;
        
        // Run migrations
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await?;

        info!("Database initialized successfully");
        
        Ok(Self { pool })
    }

    pub async fn update_event(&self, update: EventUpdate) -> Result<()> {
        // Clone the values we need for logging before they're moved
        let batch_tx_hash = update.batch_tx_hash.clone();
        
        query(
            r#"
            INSERT INTO events (
                tx_hash, event_type, src_chain_id, dst_chain_id, 
                msg_sender, amount, target_function, market,
                journal_index, journal, seal, batch_tx_hash, 
                status, resubmitted, error,
                received_at, processed_at, 
                proof_requested_at, proof_received_at,
                batch_submitted_at, batch_included_at, tx_finished_at
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13::event_status, $14, $15,
                CASE WHEN $16::bool THEN NOW() ELSE NULL END,
                CASE WHEN $17::bool THEN NOW() ELSE NULL END,
                CASE WHEN $18::bool THEN NOW() ELSE NULL END,
                CASE WHEN $19::bool THEN NOW() ELSE NULL END,
                CASE WHEN $20::bool THEN NOW() ELSE NULL END,
                CASE WHEN $21::bool THEN NOW() ELSE NULL END,
                CASE WHEN $22::bool THEN NOW() ELSE NULL END
            )
            ON CONFLICT (tx_hash) 
            DO UPDATE SET
                status = $13::event_status,
                event_type = COALESCE($2, events.event_type),
                src_chain_id = COALESCE($3, events.src_chain_id),
                dst_chain_id = COALESCE($4, events.dst_chain_id),
                msg_sender = COALESCE($5, events.msg_sender),
                amount = COALESCE($6, events.amount),
                target_function = COALESCE($7, events.target_function),
                market = COALESCE($8, events.market),
                journal_index = COALESCE($9, events.journal_index),
                journal = COALESCE($10, events.journal),
                seal = COALESCE($11, events.seal),
                batch_tx_hash = COALESCE($12, events.batch_tx_hash),
                resubmitted = COALESCE($14, events.resubmitted),
                error = COALESCE($15, events.error),
                received_at = CASE WHEN $16::bool THEN NOW() ELSE events.received_at END,
                processed_at = CASE WHEN $17::bool THEN NOW() ELSE events.processed_at END,
                proof_requested_at = CASE WHEN $18::bool THEN NOW() ELSE events.proof_requested_at END,
                proof_received_at = CASE WHEN $19::bool THEN NOW() ELSE events.proof_received_at END,
                batch_submitted_at = CASE WHEN $20::bool THEN NOW() ELSE events.batch_submitted_at END,
                batch_included_at = CASE WHEN $21::bool THEN NOW() ELSE events.batch_included_at END,
                tx_finished_at = CASE WHEN $22::bool THEN NOW() ELSE events.tx_finished_at END
            "#,
        )
        .bind(update.tx_hash.to_string())
        .bind(update.event_type)
        .bind(update.src_chain_id.map(|id| id as i32))
        .bind(update.dst_chain_id.map(|id| id as i32))
        .bind(update.msg_sender.map(|addr| addr.to_string()))
        .bind(update.amount.map(|amt| amt.to_string()))
        .bind(update.target_function)
        .bind(update.market.map(|addr| addr.to_string()))
        .bind(update.journal_index)
        .bind(update.journal.as_ref().map(|j| j.as_ref()))
        .bind(update.seal.as_ref().map(|s| s.as_ref()))
        .bind(update.batch_tx_hash)
        .bind(update.status.to_db_string())
        .bind(update.resubmitted)
        .bind(match update.status {
            EventStatus::Failed { ref error } => Some(error),
            EventStatus::BatchFailed { ref error } => Some(error),
            _ => update.error.as_ref(),
        })
        // Timestamp flags based on status
        .bind(matches!(update.status, EventStatus::Received))
        .bind(matches!(update.status, EventStatus::Processed))
        .bind(matches!(update.status, EventStatus::ProofRequested))
        .bind(matches!(update.status, EventStatus::ProofReceived))
        .bind(matches!(update.status, EventStatus::BatchSubmitted))
        .bind(matches!(update.status, EventStatus::BatchIncluded))
        .bind(matches!(update.status, EventStatus::TxProcessSuccess) || 
              matches!(update.status, EventStatus::TxProcessFail) || 
              matches!(update.status, EventStatus::BatchFailed { .. }))
        .execute(&self.pool)
        .await?;

        // Use the cloned values for logging
        info!(
            "Updated event {} status to {:?}, batch_tx_hash: {:?}",
            update.tx_hash,
            update.status,
            batch_tx_hash
        );
        Ok(())
    }

    pub async fn get_event_status(&self, tx_hash: &TxHash) -> Result<Option<EventStatus>> {
        // Use query_as to handle the event_status enum type
        let record = sqlx::query_as::<_, (String,)>(
            "SELECT status::text FROM events WHERE tx_hash = $1"
        )
        .bind(tx_hash.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match record {
            Some((status,)) => {
                match status.as_str() {
                    "Received" => Ok(Some(EventStatus::Received)),
                    "Processed" => Ok(Some(EventStatus::Processed)),
                    "IncludedInBatch" => Ok(Some(EventStatus::IncludedInBatch)),
                    "ProofRequested" => Ok(Some(EventStatus::ProofRequested)),
                    "ProofReceived" => Ok(Some(EventStatus::ProofReceived)),
                    "BatchSubmitted" => Ok(Some(EventStatus::BatchSubmitted)),
                    "BatchIncluded" => Ok(Some(EventStatus::BatchIncluded)),
                    "BatchFailed" => {
                        let error = sqlx::query_scalar::<_, String>("SELECT error FROM events WHERE tx_hash = $1")
                            .bind(tx_hash.to_string())
                            .fetch_optional(&self.pool)
                            .await?
                            .unwrap_or_else(|| "Unknown batch failure".to_string());
                        Ok(Some(EventStatus::BatchFailed { error }))
                    },
                    "TxProcessSuccess" => Ok(Some(EventStatus::TxProcessSuccess)),
                    "TxProcessFail" => Ok(Some(EventStatus::TxProcessFail)),
                    "Failed" => {
                        let error = sqlx::query_scalar::<_, String>("SELECT error FROM events WHERE tx_hash = $1")
                            .bind(tx_hash.to_string())
                            .fetch_optional(&self.pool)
                            .await?
                            .unwrap_or_else(|| "Unknown error".to_string());
                        Ok(Some(EventStatus::Failed { error }))
                    },
                    _ => Err(eyre::eyre!("Unknown status: {}", status)),
                }
            },
            None => Ok(None),
        }
    }
} 