use alloy::primitives::{Address, Bytes, TxHash, U256};
use eyre::Result;
use serde::{Deserialize, Serialize};
use sqlx::{query, Pool, Postgres, Row};
use chrono::{DateTime, Utc};
use tracing::info;
use std::str::FromStr;
use std::fs;

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
        }
        .to_string()
    }
}

// Add Default implementation for EventStatus
impl Default for EventStatus {
    fn default() -> Self {
        EventStatus::Received // Default status is Received
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventUpdate {
    pub tx_hash: TxHash,
    pub status: EventStatus,
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
    pub received_at: Option<DateTime<Utc>>,
    pub processed_at: Option<DateTime<Utc>>,
    pub proof_requested_at: Option<DateTime<Utc>>,
    pub proof_received_at: Option<DateTime<Utc>>,
    pub batch_submitted_at: Option<DateTime<Utc>>,
    pub batch_included_at: Option<DateTime<Utc>>,
    pub tx_finished_at: Option<DateTime<Utc>>,
    pub resubmitted: Option<i32>,
    pub error: Option<String>,
}

#[derive(Clone)]
pub struct Database {
    pub pool: Pool<Postgres>,
}

impl Database {
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect_with(
            sqlx::postgres::PgConnectOptions::from_str(database_url)?
                .ssl_mode(sqlx::postgres::PgSslMode::Require)
        ).await?;

        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        info!("Database initialized successfully");

        Ok(Self { pool })
    }

    pub async fn migrate_to_finished_events(&self, tx_hash: TxHash, status: EventStatus) -> Result<()> {
        // First, get all data from the events table
        let record = query(
            r#"
            SELECT 
                event_type, src_chain_id, dst_chain_id, msg_sender, amount,
                target_function, market, batch_tx_hash, received_at, processed_at,
                proof_requested_at, proof_received_at, batch_submitted_at,
                batch_included_at, resubmitted, error
            FROM events 
            WHERE tx_hash = $1
            "#
        )
        .bind(tx_hash.to_string())
        .fetch_one(&self.pool)
        .await?;

        // Insert into finished_events
        query(
            r#"
            INSERT INTO finished_events (
                tx_hash, status, event_type, src_chain_id, dst_chain_id,
                msg_sender, amount, target_function, market, batch_tx_hash,
                received_at, processed_at, proof_requested_at, proof_received_at,
                batch_submitted_at, batch_included_at, tx_finished_at,
                resubmitted, error
            )
            VALUES (
                $1, $2::event_status, $3, $4, $5, $6, $7, $8, $9, $10,
                $11, $12, $13, $14, $15, $16, $17, $18, $19
            )
            "#
        )
        .bind(tx_hash.to_string())
        .bind(status.to_db_string())
        .bind(record.try_get::<Option<String>, _>("event_type")?)
        .bind(record.try_get::<Option<i32>, _>("src_chain_id")?)
        .bind(record.try_get::<Option<i32>, _>("dst_chain_id")?)
        .bind(record.try_get::<Option<String>, _>("msg_sender")?)
        .bind(record.try_get::<Option<String>, _>("amount")?)
        .bind(record.try_get::<Option<String>, _>("target_function")?)
        .bind(record.try_get::<Option<String>, _>("market")?)
        .bind(record.try_get::<Option<String>, _>("batch_tx_hash")?)
        .bind(record.try_get::<Option<DateTime<Utc>>, _>("received_at")?)
        .bind(record.try_get::<Option<DateTime<Utc>>, _>("processed_at")?)
        .bind(record.try_get::<Option<DateTime<Utc>>, _>("proof_requested_at")?)
        .bind(record.try_get::<Option<DateTime<Utc>>, _>("proof_received_at")?)
        .bind(record.try_get::<Option<DateTime<Utc>>, _>("batch_submitted_at")?)
        .bind(record.try_get::<Option<DateTime<Utc>>, _>("batch_included_at")?)
        .bind(Utc::now())
        .bind(record.try_get::<Option<i32>, _>("resubmitted")?)
        .bind(record.try_get::<Option<String>, _>("error")?)
        .execute(&self.pool)
        .await?;

        // Delete from events
        query("DELETE FROM events WHERE tx_hash = $1")
            .bind(tx_hash.to_string())
            .execute(&self.pool)
            .await?;

        info!(
            "Successfully migrated event {} to finished_events with status {:?}",
            tx_hash, status
        );

        Ok(())
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
                $16, $17, $18, $19, $20, $21, $22
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
                received_at = COALESCE($16, events.received_at),
                processed_at = COALESCE($17, events.processed_at),
                proof_requested_at = COALESCE($18, events.proof_requested_at),
                proof_received_at = COALESCE($19, events.proof_received_at),
                batch_submitted_at = COALESCE($20, events.batch_submitted_at),
                batch_included_at = COALESCE($21, events.batch_included_at),
                tx_finished_at = COALESCE($22, events.tx_finished_at)
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
        .bind(update.received_at)
        .bind(update.processed_at)
        .bind(update.proof_requested_at)
        .bind(update.proof_received_at)
        .bind(update.batch_submitted_at)
        .bind(update.batch_included_at)
        .bind(update.tx_finished_at)
        .execute(&self.pool)
        .await?;

        // Use the cloned values for logging
        info!(
            "Updated event {} status to {:?}, batch_tx_hash: {:?}",
            update.tx_hash, update.status, batch_tx_hash
        );
        Ok(())
    }

    pub async fn get_event_status(&self, tx_hash: &TxHash) -> Result<Option<EventStatus>> {
        // Use query_as to handle the event_status enum type
        let record =
            sqlx::query_as::<_, (String,)>("SELECT status::text FROM events WHERE tx_hash = $1")
                .bind(tx_hash.to_string())
                .fetch_optional(&self.pool)
                .await?;

        match record {
            Some((status,)) => match status.as_str() {
                "Received" => Ok(Some(EventStatus::Received)),
                "Processed" => Ok(Some(EventStatus::Processed)),
                "IncludedInBatch" => Ok(Some(EventStatus::IncludedInBatch)),
                "ProofRequested" => Ok(Some(EventStatus::ProofRequested)),
                "ProofReceived" => Ok(Some(EventStatus::ProofReceived)),
                "BatchSubmitted" => Ok(Some(EventStatus::BatchSubmitted)),
                "BatchIncluded" => Ok(Some(EventStatus::BatchIncluded)),
                "BatchFailed" => {
                    let error = sqlx::query_scalar::<_, String>(
                        "SELECT error FROM events WHERE tx_hash = $1",
                    )
                    .bind(tx_hash.to_string())
                    .fetch_optional(&self.pool)
                    .await?
                    .unwrap_or_else(|| "Unknown batch failure".to_string());
                    Ok(Some(EventStatus::BatchFailed { error }))
                }
                "TxProcessSuccess" => Ok(Some(EventStatus::TxProcessSuccess)),
                "TxProcessFail" => Ok(Some(EventStatus::TxProcessFail)),
                "Failed" => {
                    let error = sqlx::query_scalar::<_, String>(
                        "SELECT error FROM events WHERE tx_hash = $1",
                    )
                    .bind(tx_hash.to_string())
                    .fetch_optional(&self.pool)
                    .await?
                    .unwrap_or_else(|| "Unknown error".to_string());
                    Ok(Some(EventStatus::Failed { error }))
                }
                _ => Err(eyre::eyre!("Unknown status: {}", status)),
            },
            None => Ok(None),
        }
    }

    pub async fn get_event(&self, tx_hash: &TxHash) -> Result<Option<EventUpdate>> {
        let record = query(
            r#"
            SELECT 
                status::text, event_type, src_chain_id, dst_chain_id, msg_sender,
                amount, target_function, market, journal_index, journal,
                seal, batch_tx_hash, received_at, processed_at,
                proof_requested_at, proof_received_at, batch_submitted_at,
                batch_included_at, tx_finished_at, resubmitted, error
            FROM events 
            WHERE tx_hash = $1
            "#
        )
        .bind(tx_hash.to_string())
        .fetch_optional(&self.pool)
        .await?;

        match record {
            Some(row) => {
                let status_str: String = row.try_get("status")?;
                let status = match status_str.as_str() {
                    "Received" => EventStatus::Received,
                    "Processed" => EventStatus::Processed,
                    "IncludedInBatch" => EventStatus::IncludedInBatch,
                    "ProofRequested" => EventStatus::ProofRequested,
                    "ProofReceived" => EventStatus::ProofReceived,
                    "BatchSubmitted" => EventStatus::BatchSubmitted,
                    "BatchIncluded" => EventStatus::BatchIncluded,
                    "BatchFailed" => EventStatus::BatchFailed {
                        error: row.try_get("error")?,
                    },
                    "TxProcessSuccess" => EventStatus::TxProcessSuccess,
                    "TxProcessFail" => EventStatus::TxProcessFail,
                    "Failed" => EventStatus::Failed {
                        error: row.try_get("error")?,
                    },
                    _ => return Err(eyre::eyre!("Unknown status: {}", status_str)),
                };

                Ok(Some(EventUpdate {
                    tx_hash: *tx_hash,
                    status,
                    event_type: row.try_get("event_type")?,
                    src_chain_id: row.try_get::<Option<i32>, _>("src_chain_id")?.map(|id| id as u32),
                    dst_chain_id: row.try_get::<Option<i32>, _>("dst_chain_id")?.map(|id| id as u32),
                    msg_sender: row.try_get::<Option<String>, _>("msg_sender")?.map(|addr| addr.parse().unwrap()),
                    amount: row.try_get::<Option<String>, _>("amount")?.map(|amt| amt.parse().unwrap()),
                    target_function: row.try_get("target_function")?,
                    market: row.try_get::<Option<String>, _>("market")?.map(|addr| addr.parse().unwrap()),
                    journal_index: row.try_get("journal_index")?,
                    journal: row.try_get::<Option<Vec<u8>>, _>("journal")?.map(Bytes::from),
                    seal: row.try_get::<Option<Vec<u8>>, _>("seal")?.map(Bytes::from),
                    batch_tx_hash: row.try_get("batch_tx_hash")?,
                    received_at: row.try_get("received_at")?,
                    processed_at: row.try_get("processed_at")?,
                    proof_requested_at: row.try_get("proof_requested_at")?,
                    proof_received_at: row.try_get("proof_received_at")?,
                    batch_submitted_at: row.try_get("batch_submitted_at")?,
                    batch_included_at: row.try_get("batch_included_at")?,
                    tx_finished_at: row.try_get("tx_finished_at")?,
                    resubmitted: row.try_get("resubmitted")?,
                    error: row.try_get("error")?,
                }))
            }
            None => Ok(None),
        }
    }

    pub async fn get_processed_events(&self, delay_seconds: i64) -> Result<Vec<EventUpdate>> {
        // First check if enough time has passed since the last proof request
        let should_proceed = query(
            r#"
            SELECT 
                CASE 
                    WHEN last_proof_requested_at IS NULL THEN true
                    WHEN EXTRACT(EPOCH FROM (NOW() - last_proof_requested_at)) >= $1 THEN true
                    ELSE false
                END as should_proceed
            FROM sync_timestamps
            WHERE id = 1
            "#
        )
        .bind(delay_seconds)
        .fetch_one(&self.pool)
        .await?
        .try_get::<bool, _>("should_proceed")?;

        if !should_proceed {
            return Ok(Vec::new());
        }

        // Use a transaction to ensure atomicity
        let mut tx = self.pool.begin().await?;
        
        // Update the last_proof_requested_at timestamp
        query(
            r#"
            UPDATE sync_timestamps
            SET last_proof_requested_at = NOW()
            WHERE id = 1
            "#
        )
        .execute(tx.as_mut())
        .await?;

        // Get and update the processed events
        let records = query(
            r#"
            WITH claimed_events AS (
                UPDATE events 
                SET status = 'ProofRequested'::event_status,
                    proof_requested_at = NOW()
                WHERE tx_hash IN (
                    SELECT tx_hash 
                    FROM events 
                    WHERE status = 'Processed'::event_status
                    LIMIT 1
                    FOR UPDATE SKIP LOCKED
                )
                RETURNING 
                    tx_hash, status::text, event_type, src_chain_id, dst_chain_id, msg_sender,
                    amount, target_function, market, journal_index, journal,
                    seal, batch_tx_hash, received_at, processed_at,
                    proof_requested_at, proof_received_at, batch_submitted_at,
                    batch_included_at, tx_finished_at, resubmitted, error
            )
            SELECT * FROM claimed_events
            "#
        )
        .fetch_all(tx.as_mut())
        .await?;

        // Commit the transaction
        tx.commit().await?;

        let mut events = Vec::new();

        for row in records {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;
            
            let status_str: String = row.try_get("status")?;
            let status = match status_str.as_str() {
                "Received" => EventStatus::Received,
                "Processed" => EventStatus::Processed,
                "IncludedInBatch" => EventStatus::IncludedInBatch,
                "ProofRequested" => EventStatus::ProofRequested,
                "ProofReceived" => EventStatus::ProofReceived,
                "BatchSubmitted" => EventStatus::BatchSubmitted,
                "BatchIncluded" => EventStatus::BatchIncluded,
                "BatchFailed" => EventStatus::BatchFailed {
                    error: row.try_get("error")?,
                },
                "TxProcessSuccess" => EventStatus::TxProcessSuccess,
                "TxProcessFail" => EventStatus::TxProcessFail,
                "Failed" => EventStatus::Failed {
                    error: row.try_get("error")?,
                },
                _ => return Err(eyre::eyre!("Unknown status: {}", status_str)),
            };

            events.push(EventUpdate {
                tx_hash,
                status,
                event_type: row.try_get("event_type")?,
                src_chain_id: row.try_get::<Option<i32>, _>("src_chain_id")?.map(|id| id as u32),
                dst_chain_id: row.try_get::<Option<i32>, _>("dst_chain_id")?.map(|id| id as u32),
                msg_sender: row.try_get::<Option<String>, _>("msg_sender")?.map(|addr| addr.parse().unwrap()),
                amount: row.try_get::<Option<String>, _>("amount")?.map(|amt| amt.parse().unwrap()),
                target_function: row.try_get("target_function")?,
                market: row.try_get::<Option<String>, _>("market")?.map(|addr| addr.parse().unwrap()),
                journal_index: row.try_get("journal_index")?,
                journal: row.try_get::<Option<Vec<u8>>, _>("journal")?.map(Bytes::from),
                seal: row.try_get::<Option<Vec<u8>>, _>("seal")?.map(Bytes::from),
                batch_tx_hash: row.try_get("batch_tx_hash")?,
                received_at: row.try_get("received_at")?,
                processed_at: row.try_get("processed_at")?,
                proof_requested_at: row.try_get("proof_requested_at")?,
                proof_received_at: row.try_get("proof_received_at")?,
                batch_submitted_at: row.try_get("batch_submitted_at")?,
                batch_included_at: row.try_get("batch_included_at")?,
                tx_finished_at: row.try_get("tx_finished_at")?,
                resubmitted: row.try_get("resubmitted")?,
                error: row.try_get("error")?,
            });
        }

        Ok(events)
    }

    pub async fn get_proven_events(&self, delay_seconds: i64) -> Result<Vec<EventUpdate>> {
        // First check if enough time has passed since the last batch submission
        let should_proceed = query(
            r#"
            SELECT 
                CASE 
                    WHEN last_batch_submitted_at IS NULL THEN true
                    WHEN EXTRACT(EPOCH FROM (NOW() - last_batch_submitted_at)) >= $1 THEN true
                    ELSE false
                END as should_proceed
            FROM sync_timestamps
            WHERE id = 1
            "#
        )
        .bind(delay_seconds)
        .fetch_one(&self.pool)
        .await?
        .try_get::<bool, _>("should_proceed")?;

        if !should_proceed {
            return Ok(Vec::new());
        }

        // Use a transaction to ensure atomicity
        let mut tx = self.pool.begin().await?;
        
        // Update the last_batch_submitted_at timestamp
        query(
            r#"
            UPDATE sync_timestamps
            SET last_batch_submitted_at = NOW()
            WHERE id = 1
            "#
        )
        .execute(tx.as_mut())
        .await?;

        // First, get the journal with the earliest proof_received_at timestamp
        let first_event = query(
            r#"
            SELECT journal, proof_received_at
            FROM events 
            WHERE status = 'ProofReceived'::event_status
            AND journal IS NOT NULL
            ORDER BY proof_received_at ASC
            LIMIT 1
            FOR UPDATE SKIP LOCKED
            "#
        )
        .fetch_optional(tx.as_mut())
        .await?;

        // If no events found, return empty vector
        if first_event.is_none() {
            tx.commit().await?;
            return Ok(Vec::new());
        }

        let first_journal = first_event.unwrap().try_get::<Option<Vec<u8>>, _>("journal")?;

        // If no journal found, return empty vector
        if first_journal.is_none() {
            tx.commit().await?;
            return Ok(Vec::new());
        }

        // Get all events with the same journal and update their status
        let records = query(
            r#"
            WITH updated_events AS (
                UPDATE events 
                SET status = 'BatchSubmitted'::event_status,
                    batch_submitted_at = NOW()
                WHERE status = 'ProofReceived'::event_status
                AND journal = $1
                RETURNING 
                    tx_hash, status::text, event_type, src_chain_id, dst_chain_id, msg_sender,
                    amount, target_function, market, journal_index, journal,
                    seal, batch_tx_hash, received_at, processed_at,
                    proof_requested_at, proof_received_at, batch_submitted_at,
                    batch_included_at, tx_finished_at, resubmitted, error
            )
            SELECT * FROM updated_events
            ORDER BY journal_index ASC
            "#
        )
        .bind(first_journal)
        .fetch_all(tx.as_mut())
        .await?;

        // Commit the transaction
        tx.commit().await?;

        let mut events = Vec::new();

        for row in records {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;
            
            let status_str: String = row.try_get("status")?;
            let status = match status_str.as_str() {
                "Received" => EventStatus::Received,
                "Processed" => EventStatus::Processed,
                "IncludedInBatch" => EventStatus::IncludedInBatch,
                "ProofRequested" => EventStatus::ProofRequested,
                "ProofReceived" => EventStatus::ProofReceived,
                "BatchSubmitted" => EventStatus::BatchSubmitted,
                "BatchIncluded" => EventStatus::BatchIncluded,
                "BatchFailed" => EventStatus::BatchFailed {
                    error: row.try_get("error")?,
                },
                "TxProcessSuccess" => EventStatus::TxProcessSuccess,
                "TxProcessFail" => EventStatus::TxProcessFail,
                "Failed" => EventStatus::Failed {
                    error: row.try_get("error")?,
                },
                _ => return Err(eyre::eyre!("Unknown status: {}", status_str)),
            };

            events.push(EventUpdate {
                tx_hash,
                status,
                event_type: row.try_get("event_type")?,
                src_chain_id: row.try_get::<Option<i32>, _>("src_chain_id")?.map(|id| id as u32),
                dst_chain_id: row.try_get::<Option<i32>, _>("dst_chain_id")?.map(|id| id as u32),
                msg_sender: row.try_get::<Option<String>, _>("msg_sender")?.map(|addr| addr.parse().unwrap()),
                amount: row.try_get::<Option<String>, _>("amount")?.map(|amt| amt.parse().unwrap()),
                target_function: row.try_get("target_function")?,
                market: row.try_get::<Option<String>, _>("market")?.map(|addr| addr.parse().unwrap()),
                journal_index: row.try_get("journal_index")?,
                journal: row.try_get::<Option<Vec<u8>>, _>("journal")?.map(Bytes::from),
                seal: row.try_get::<Option<Vec<u8>>, _>("seal")?.map(Bytes::from),
                batch_tx_hash: row.try_get("batch_tx_hash")?,
                received_at: row.try_get("received_at")?,
                processed_at: row.try_get("processed_at")?,
                proof_requested_at: row.try_get("proof_requested_at")?,
                proof_received_at: row.try_get("proof_received_at")?,
                batch_submitted_at: row.try_get("batch_submitted_at")?,
                batch_included_at: row.try_get("batch_included_at")?,
                tx_finished_at: row.try_get("tx_finished_at")?,
                resubmitted: row.try_get("resubmitted")?,
                error: row.try_get("error")?,
            });
        }

        Ok(events)
    }
}
