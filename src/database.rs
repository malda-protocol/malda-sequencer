use alloy::primitives::{Address, Bytes, TxHash, U256};
use eyre::Result;
use serde::{Deserialize, Serialize};
use sqlx::{query, Pool, Postgres, Row};
use chrono::{DateTime, Utc};
use tracing::info;
use std::str::FromStr;
use std::collections::HashMap;
use tracing::{debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventStatus {
    Received,
    Processed,
    IncludedInBatch,
    ReadyToRequestProof,
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
            EventStatus::ReadyToRequestProof => "ReadyToRequestProof",
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
    pub received_at_block: Option<i32>,
    pub should_request_proof_at_block: Option<i32>,
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

    pub async fn add_new_events(&self, events: &Vec<EventUpdate>) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        for event in events {
            // Only insert fields relevant to a newly processed event
            query(
                r#"
                INSERT INTO events (
                    tx_hash, status, event_type, src_chain_id, dst_chain_id, 
                    msg_sender, amount, target_function, market,
                    received_at_block, should_request_proof_at_block,
                    received_at, processed_at
                )
                VALUES (
                    $1, $2::event_status, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13
                )
                ON CONFLICT (tx_hash) DO NOTHING
                "#
            )
            .bind(event.tx_hash.to_string())
            .bind(event.status.to_db_string()) // Always Processed from event_listener
            .bind(event.event_type.as_ref())
            .bind(event.src_chain_id.map(|id| id as i32))
            .bind(event.dst_chain_id.map(|id| id as i32))
            .bind(event.msg_sender.map(|addr| addr.to_string()))
            .bind(event.amount.map(|amt| amt.to_string()))
            .bind(event.target_function.as_ref())
            .bind(event.market.map(|addr| addr.to_string()))
            .bind(event.received_at_block)
            .bind(event.should_request_proof_at_block)
            .bind(event.received_at) // Set in event_listener
            .bind(event.processed_at) // Set in event_listener
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        info!("Attempted to add batch of {} new events to database", events.len());

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
                "ReadyToRequestProof" => Ok(Some(EventStatus::ReadyToRequestProof)),
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
                    "ReadyToRequestProof" => EventStatus::ReadyToRequestProof,
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
                    received_at_block: row.try_get("received_at_block")?,
                    should_request_proof_at_block: row.try_get("should_request_proof_at_block")?,
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

    pub async fn get_ready_to_request_proof_events(
        &self, 
        delay_seconds: i64, 
        batch_limit_per_dst: i64
    ) -> Result<Vec<EventUpdate>> { 
        // Re-added rate limiting check using sync_timestamps
        let should_proceed = query(
            r#"
            SELECT 
                CASE 
                    WHEN last_proof_requested_at IS NULL THEN true
                    WHEN EXTRACT(EPOCH FROM (NOW() - last_proof_requested_at)) >= $1 THEN true
                    ELSE false
                END as should_proceed
            FROM sync_timestamps
            WHERE id = 1 -- Assuming a single row for global proof request timestamp
            "#
        )
        .bind(delay_seconds)
        .fetch_one(&self.pool) // Use fetch_one, assuming the row always exists
        .await?
        .try_get::<bool, _>("should_proceed")?;

        if !should_proceed {
            debug!("Proof request delay ({}s) not met, skipping claim.", delay_seconds);
            return Ok(Vec::new()); // Return empty if delay not met
        }

        // Use a transaction to ensure atomicity
        let mut tx = self.pool.begin().await?;
        
        // Re-added UPDATE sync_timestamps set last_proof_requested_at = NOW()
        // This acts as a lock for this cycle
        query(
            r#"
            UPDATE sync_timestamps
            SET last_proof_requested_at = NOW()
            WHERE id = 1
            "#
        )
        .execute(tx.as_mut())
        .await?;

        // Claim events and set status/timestamp atomically, returning only needed fields
        let records = query(
            r#"
            WITH ready_events AS (
                SELECT tx_hash, dst_chain_id, received_at
                FROM events 
                WHERE status = 'ReadyToRequestProof'::event_status
            ),
            ranked_events AS (
                SELECT tx_hash, 
                       ROW_NUMBER() OVER (PARTITION BY dst_chain_id ORDER BY received_at ASC, tx_hash ASC) as row_num -- Order by received time first
                FROM ready_events
            ),
            claim_targets AS (
                SELECT tx_hash
                FROM ranked_events
                WHERE row_num <= $1 -- Use parameter for batch size limit
            ),
            claimed_events AS (
                UPDATE events 
                SET status = 'ProofRequested'::event_status,
                    proof_requested_at = NOW() -- Set timestamp here
                WHERE tx_hash IN (SELECT tx_hash FROM claim_targets)
                RETURNING -- Only return necessary fields
                    tx_hash, src_chain_id, dst_chain_id, msg_sender, market
            )
            SELECT * FROM claimed_events
            "#
        )
        .bind(batch_limit_per_dst) // Bind the batch limit parameter
        .fetch_all(tx.as_mut())
        .await?;

        // Commit the transaction
        tx.commit().await?;

        let mut events = Vec::new();
        if records.is_empty() {
            return Ok(events);
        }

        info!("Claimed {} events for proof generation.", records.len());

        for row in records {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;
            
            // We know the status is ProofRequested because we just set it.
            // Only populate fields returned by the query.
            events.push(EventUpdate {
                tx_hash,
                status: EventStatus::ProofRequested, // Set status explicitly
                src_chain_id: row.try_get::<Option<i32>, _>("src_chain_id")?.map(|id| id as u32),
                dst_chain_id: row.try_get::<Option<i32>, _>("dst_chain_id")?.map(|id| id as u32),
                msg_sender: row.try_get::<Option<String>, _>("msg_sender")?.map(|addr| addr.parse().unwrap()),
                market: row.try_get::<Option<String>, _>("market")?.map(|addr| addr.parse().unwrap()),
                // Other fields are Default::default()
                ..Default::default()
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

        // Get and update the proven events
        let records = query(
            r#"
            WITH claimed_events AS (
                UPDATE events 
                SET status = 'BatchSubmitted'::event_status,
                    batch_submitted_at = NOW()
                WHERE tx_hash IN (
                    SELECT tx_hash 
                    FROM events 
                    WHERE status = 'ProofReceived'::event_status
                    AND journal IS NOT NULL
                    AND journal = $1
                )
                RETURNING 
                    tx_hash, status::text, event_type, src_chain_id, dst_chain_id, msg_sender,
                    amount, target_function, market, received_at_block, should_request_proof_at_block,
                    journal_index, journal, seal, batch_tx_hash, received_at, processed_at,
                    proof_requested_at, proof_received_at, batch_submitted_at,
                    batch_included_at, tx_finished_at, resubmitted, error
            )
            SELECT * FROM claimed_events
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
                "ReadyToRequestProof" => EventStatus::ReadyToRequestProof,
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
                received_at_block: row.try_get("received_at_block")?,
                should_request_proof_at_block: row.try_get("should_request_proof_at_block")?,
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

    pub async fn get_processed_events(&self) -> Result<Vec<EventUpdate>> {
        // Get all events with status "Processed"
        let records = query(
            r#"
            SELECT 
                tx_hash, status::text, event_type, src_chain_id, dst_chain_id, msg_sender,
                amount, target_function, market, received_at_block, should_request_proof_at_block,
                journal_index, journal, seal, batch_tx_hash, received_at, processed_at,
                proof_requested_at, proof_received_at, batch_submitted_at,
                batch_included_at, tx_finished_at, resubmitted, error
            FROM events 
            WHERE status = 'Processed'::event_status
            "#
        )
        .fetch_all(&self.pool)
        .await?;

        let mut events = Vec::new();

        for row in records {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;
            
            let status_str: String = row.try_get("status")?;
            let status = match status_str.as_str() {
                "Received" => EventStatus::Received,
                "Processed" => EventStatus::Processed,
                "IncludedInBatch" => EventStatus::IncludedInBatch,
                "ReadyToRequestProof" => EventStatus::ReadyToRequestProof,
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
                received_at_block: row.try_get("received_at_block")?,
                should_request_proof_at_block: row.try_get("should_request_proof_at_block")?,
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

    pub async fn update_finished_events(&self, updates: &Vec<EventUpdate>) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Store the length before the loop
        let updates_len = updates.len();

        // Start a transaction
        let mut tx = self.pool.begin().await?;

        for update in updates {
            // Insert into finished_events with ON CONFLICT DO UPDATE
            query(
                r#"
                INSERT INTO finished_events (
                    tx_hash, status, event_type, src_chain_id, dst_chain_id,
                    msg_sender, amount, target_function, market, received_at_block, should_request_proof_at_block,
                    journal_index, seal, batch_tx_hash, received_at, processed_at, proof_requested_at, proof_received_at,
                    batch_submitted_at, batch_included_at, tx_finished_at,
                    resubmitted, error
                )
                VALUES (
                    $1, $2::event_status, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14,
                    $15, $16, $17, $18, $19, $20, $21, $22, $23
                )
                ON CONFLICT (tx_hash) DO UPDATE SET
                    status = EXCLUDED.status,
                    event_type = COALESCE(EXCLUDED.event_type, finished_events.event_type),
                    src_chain_id = COALESCE(EXCLUDED.src_chain_id, finished_events.src_chain_id),
                    dst_chain_id = COALESCE(EXCLUDED.dst_chain_id, finished_events.dst_chain_id),
                    msg_sender = COALESCE(EXCLUDED.msg_sender, finished_events.msg_sender),
                    amount = COALESCE(EXCLUDED.amount, finished_events.amount),
                    target_function = COALESCE(EXCLUDED.target_function, finished_events.target_function),
                    market = COALESCE(EXCLUDED.market, finished_events.market),
                    received_at_block = COALESCE(EXCLUDED.received_at_block, finished_events.received_at_block),
                    should_request_proof_at_block = COALESCE(EXCLUDED.should_request_proof_at_block, finished_events.should_request_proof_at_block),
                    journal_index = COALESCE(EXCLUDED.journal_index, finished_events.journal_index),
                    seal = COALESCE(EXCLUDED.seal, finished_events.seal),
                    batch_tx_hash = COALESCE(EXCLUDED.batch_tx_hash, finished_events.batch_tx_hash),
                    received_at = COALESCE(EXCLUDED.received_at, finished_events.received_at),
                    processed_at = COALESCE(EXCLUDED.processed_at, finished_events.processed_at),
                    proof_requested_at = COALESCE(EXCLUDED.proof_requested_at, finished_events.proof_requested_at),
                    proof_received_at = COALESCE(EXCLUDED.proof_received_at, finished_events.proof_received_at),
                    batch_submitted_at = COALESCE(EXCLUDED.batch_submitted_at, finished_events.batch_submitted_at),
                    batch_included_at = COALESCE(EXCLUDED.batch_included_at, finished_events.batch_included_at),
                    tx_finished_at = COALESCE(EXCLUDED.tx_finished_at, finished_events.tx_finished_at),
                    resubmitted = COALESCE(EXCLUDED.resubmitted, finished_events.resubmitted),
                    error = COALESCE(EXCLUDED.error, finished_events.error)
                "#
            )
            .bind(update.tx_hash.to_string())
            .bind(update.status.to_db_string())
            .bind(update.event_type.clone())
            .bind(update.src_chain_id.map(|id| id as i32))
            .bind(update.dst_chain_id.map(|id| id as i32))
            .bind(update.msg_sender.map(|addr| addr.to_string()))
            .bind(update.amount.map(|amt| amt.to_string()))
            .bind(update.target_function.clone())
            .bind(update.market.map(|addr| addr.to_string()))
            .bind(update.received_at_block)
            .bind(update.should_request_proof_at_block)
            .bind(update.journal_index)
            .bind(update.seal.clone().map(|s| s.to_vec()))
            .bind(update.batch_tx_hash.clone())
            .bind(update.received_at)
            .bind(update.processed_at)
            .bind(update.proof_requested_at)
            .bind(update.proof_received_at)
            .bind(update.batch_submitted_at)
            .bind(update.batch_included_at)
            .bind(update.tx_finished_at.unwrap_or_else(Utc::now))
            .bind(update.resubmitted)
            .bind(update.error.clone())
            .execute(&mut *tx)
            .await?;

            // Delete from events
            query("DELETE FROM events WHERE tx_hash = $1")
                .bind(update.tx_hash.to_string())
                .execute(&mut *tx)
                .await?;
        }

        // Commit the transaction
        tx.commit().await?;

        info!("Successfully migrated {} events to finished_events", updates_len);

        Ok(())
    }

    pub async fn migrate_to_finished_events(&self, tx_hash: TxHash, status: EventStatus) -> Result<()> {
        // First, get all data from the events table
        let record = query(
            r#"
            SELECT 
                event_type, src_chain_id, dst_chain_id, msg_sender, amount,
                target_function, market, received_at_block, should_request_proof_at_block,
                batch_tx_hash, received_at, processed_at,
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
                msg_sender, amount, target_function, market, received_at_block, should_request_proof_at_block,
                batch_tx_hash, received_at, processed_at, proof_requested_at, proof_received_at,
                batch_submitted_at, batch_included_at, tx_finished_at,
                resubmitted, error
            )
            VALUES (
                $1, $2::event_status, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12,
                $13, $14, $15, $16, $17, $18, $19, $20, $21
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
        .bind(record.try_get::<Option<i32>, _>("received_at_block")?)
        .bind(record.try_get::<Option<i32>, _>("should_request_proof_at_block")?)
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


    // New function to update events based on current block numbers
    pub async fn update_events_to_ready_status(&self, current_block_map: &HashMap<u64, i32>) -> Result<()> {
        if current_block_map.is_empty() {
            info!("No current block numbers provided, skipping update.");
            return Ok(());
        }

        // Convert HashMap to vectors for binding
        let chain_ids: Vec<i64> = current_block_map.keys().map(|&k| k as i64).collect();
        let block_numbers: Vec<i32> = current_block_map.values().cloned().collect();

        let rows_affected = query(
            r#"
            WITH current_blocks (chain_id, block_num) AS (
                -- Unnest the arrays of chain IDs and corresponding block numbers
                SELECT * FROM UNNEST($1::bigint[], $2::int[])
            )
            UPDATE events e
            SET status = 'ReadyToRequestProof'::event_status
            FROM current_blocks cb
            WHERE e.status = 'Processed'::event_status                -- Only consider 'Processed' events
              AND e.src_chain_id = cb.chain_id                     -- Match event's chain ID (assuming src_chain_id is BIGINT or compatible)
              AND e.should_request_proof_at_block <= cb.block_num; -- Check if the block number condition is met
            "#
        )
        .bind(&chain_ids)
        .bind(&block_numbers)
        .execute(&self.pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!("Updated {} events to ReadyToRequestProof status based on current block numbers.", rows_affected);
        } else {
            debug!("No events updated to ReadyToRequestProof status in this cycle.");
        }

        Ok(())
    }

    // New function to update events after proof is received, including journal index
    pub async fn set_events_proof_received_with_index(
        &self,
        updates: &Vec<(TxHash, i32)>, // Vec of (tx_hash, journal_index)
        journal: &Bytes,
        seal: &Bytes,
    ) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        let tx_hashes: Vec<String> = updates.iter().map(|(h, _)| h.to_string()).collect();
        let journal_indices: Vec<i32> = updates.iter().map(|(_, idx)| *idx).collect();
        let count = updates.len();

        // Use UNNEST and JOIN to update journal_index based on tx_hash
        let rows_affected = query(
            r#"
            WITH index_data (tx_hash_text, journal_idx) AS (
                SELECT * FROM UNNEST($1::text[], $2::int[])
            )
            UPDATE events e
            SET 
                status = 'ProofReceived'::event_status,
                journal = $3,
                seal = $4,
                proof_received_at = NOW(),
                journal_index = id.journal_idx
            FROM index_data id
            WHERE e.tx_hash = id.tx_hash_text
              AND e.status = 'ProofRequested'::event_status; -- Ensure we only update events we claimed
            "#
        )
        .bind(&tx_hashes)
        .bind(&journal_indices)
        .bind(journal.to_vec()) // Bind journal bytes
        .bind(seal.to_vec())     // Bind seal bytes
        .execute(&self.pool)
        .await?
        .rows_affected();

        if rows_affected == count as u64 {
            info!("Successfully updated {} events with proof data and journal indices.", count);
        } else {
            warn!("Attempted to update {} events with proof data, but {} rows were affected. Some events might not have been in 'ProofRequested' state.", count, rows_affected);
        }

        Ok(())
    }
}


