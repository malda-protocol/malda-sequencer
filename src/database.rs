use alloy::primitives::{Address, Bytes, TxHash, U256};
use eyre::Result;
use serde::{Deserialize, Serialize};
use sqlx::{query, query_as, query_scalar, Pool, Postgres, Row};
use chrono::{DateTime, Utc};
use tracing::info;
use std::str::FromStr;
use std::collections::HashMap;
use tracing::{debug, warn};
use hex; // Ensure hex is imported if used in logs

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

    // Renamed and refactored to fetch events for a specific destination chain
    pub async fn get_proven_events_for_chain(
        &self,
        delay_seconds: i64,
        target_dst_chain_id: u32,
        max_tx: i64, // Added max_tx limit parameter
    ) -> Result<Vec<EventUpdate>> {
    
        // 1. & 2. Get last submission time for the target chain and check delay
        let last_submission_time: Option<DateTime<Utc>> = query_scalar(
                "SELECT last_batch_submitted_at FROM chain_batch_sync WHERE dst_chain_id = $1"
            )
            .bind(target_dst_chain_id as i32)
            .fetch_optional(&self.pool)
            .await?;
    
        let now = Utc::now();
        let required_delay = chrono::Duration::seconds(delay_seconds);
    
        if let Some(last_submitted) = last_submission_time {
            if now.signed_duration_since(last_submitted) < required_delay {
                debug!(
                    "Delay not met for chain {}. Last submission: {:?}, Current time: {:?}, Required delay: {}s",
                    target_dst_chain_id, last_submitted, now, delay_seconds
                );
                return Ok(Vec::new()); // Delay not met
            }
        }
        // If timestamp is NULL (first time), proceed.
    
        // 3. Find the oldest journal among ProofReceived events for the TARGET chain
        let oldest_journal_data: Option<(Vec<u8>,)> = query_as(r#"
                SELECT journal 
                FROM events 
                WHERE status = 'ProofReceived'::event_status 
                  AND dst_chain_id = $1 -- Filter by target chain
                  AND journal IS NOT NULL 
                ORDER BY proof_received_at ASC 
                LIMIT 1
            "#)
            .bind(target_dst_chain_id as i32)
            .fetch_optional(&self.pool)
            .await?;
    
        let oldest_journal = match oldest_journal_data {
            Some((journal,)) => Bytes::from(journal),
            None => {
                debug!("No ProofReceived events found with a journal for chain {}.", target_dst_chain_id);
                return Ok(Vec::new()); // No events to process for this chain
            }
        };
    
        // 4. Start Transaction
        let mut tx = self.pool.begin().await?;
    
        // 5. Update Timestamp in chain_batch_sync for the target chain
        let now_utc = Utc::now();
        query(r#"
            INSERT INTO chain_batch_sync (dst_chain_id, last_batch_submitted_at) 
            VALUES ($1, $2) 
            ON CONFLICT (dst_chain_id) DO UPDATE 
            SET last_batch_submitted_at = EXCLUDED.last_batch_submitted_at
        "#)
        .bind(target_dst_chain_id as i32)
        .bind(now_utc)
        .execute(tx.as_mut())
        .await?;
    
        // 6. Claim Events for the specific journal AND target chain, applying LIMIT
        let records = query(r#"
            WITH events_to_claim AS (
                -- Select tx_hashes from the oldest journal for the target chain, ordered by index, limited
                SELECT tx_hash
                FROM events e
                WHERE e.journal = $2 -- oldest_journal
                  AND e.dst_chain_id = $3 -- target_dst_chain_id
                  AND e.status = 'ProofReceived'::event_status
                ORDER BY e.journal_index ASC
                LIMIT $4 -- max_tx
            )
            UPDATE events
            SET status = 'BatchSubmitted'::event_status,
                batch_submitted_at = $1 -- now_utc
            WHERE tx_hash IN (SELECT tx_hash FROM events_to_claim) -- Update only the limited set
            RETURNING -- Only return necessary fields for the updated set
                tx_hash, journal_index, dst_chain_id, journal, seal,
                msg_sender, market, amount, target_function
            -- ORDER BY journal_index ASC -- Ordering in UPDATE RETURNING is removed for reliability, sort in Rust
            "#)
            .bind(now_utc) // $1
            .bind(oldest_journal.to_vec()) // $2
            .bind(target_dst_chain_id as i32) // $3
            .bind(max_tx) // $4: Bind the max_tx limit
            .fetch_all(tx.as_mut())
            .await?;
    
        // 7. Commit Transaction
        tx.commit().await?;
    
        // 8. Process and Return Claimed Events (Sort here in Rust)
        let mut events = Vec::new();
        if records.is_empty() {
             warn!("Passed delay checks and updated timestamps, but claimed 0 events for chain {} / journal 0x{}. This might indicate a race condition or inconsistent state.", target_dst_chain_id, hex::encode(&oldest_journal));
            return Ok(events);
        }
    
        info!("Claimed {} (max {}) events for batch submission on chain {} (Journal: 0x{}...).", records.len(), max_tx, target_dst_chain_id, hex::encode(&oldest_journal[..std::cmp::min(8, oldest_journal.len())]));
    
        for row in records {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;
    
            // Safely parse potentially NULL fields
            let msg_sender = row.try_get::<Option<String>, _>("msg_sender")?.and_then(|s| s.parse::<Address>().ok());
            let market = row.try_get::<Option<String>, _>("market")?.and_then(|s| s.parse::<Address>().ok());
            let amount = row.try_get::<Option<String>, _>("amount")?.and_then(|s| s.parse::<U256>().ok());

            events.push(EventUpdate {
                tx_hash,
                status: EventStatus::BatchSubmitted, // Status is known
                journal_index: row.try_get("journal_index")?, // Should exist
                dst_chain_id: Some(target_dst_chain_id), // Known input
                journal: row.try_get::<Option<Vec<u8>>, _>("journal")?.map(Bytes::from),
                seal: row.try_get::<Option<Vec<u8>>, _>("seal")?.map(Bytes::from),
                msg_sender,
                market,
                amount,
                target_function: row.try_get("target_function")?,
                batch_submitted_at: Some(now_utc), // Set from variable
                ..Default::default()
            });
        }
    
        // Sort the results in Rust code after fetching
        events.sort_by_key(|e| e.journal_index.unwrap_or(0));

        Ok(events)
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

    pub async fn update_batch_status(
        &self,
        tx_hashes: &Vec<TxHash>,
        batch_tx_hash: Option<String>,
        batch_included_at: Option<DateTime<Utc>>,
        error: Option<String>,
        status: EventStatus,
    ) -> Result<()> {
        if tx_hashes.is_empty() {
            return Ok(());
        }

        let tx_hashes_str: Vec<String> = tx_hashes.iter().map(|h| h.to_string()).collect();
        let status_str = status.to_db_string();
        let count = tx_hashes.len();

        // Start a transaction
        let mut tx = self.pool.begin().await?;

        // Bulk update the events
        let rows_affected = query(
            r#"
            UPDATE events
            SET 
                status = $1::event_status,
                batch_tx_hash = COALESCE($2, batch_tx_hash),
                batch_included_at = COALESCE($3, batch_included_at),
                error = COALESCE($4, error)
            WHERE tx_hash = ANY($5::text[])
              AND status = 'BatchSubmitted'::event_status
            "#
        )
        .bind(&status_str)  // Use reference here
        .bind(batch_tx_hash)
        .bind(batch_included_at)
        .bind(error)
        .bind(&tx_hashes_str)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        // Commit the transaction
        tx.commit().await?;

        if rows_affected == count as u64 {
            info!("Successfully updated batch status for {} events to {}", count, status_str);
        } else {
            warn!(
                "Attempted to update {} events, but {} rows were affected. Some events might not exist or not be in BatchSubmitted status.",
                count,
                rows_affected
            );
        }

        Ok(())
    }

    pub async fn update_finished_events(
        &self,
        tx_hashes: &Vec<TxHash>,
        status: EventStatus,
    ) -> Result<()> {
        if tx_hashes.is_empty() {
            return Ok(());
        }

        let tx_hashes_str: Vec<String> = tx_hashes.iter().map(|h| h.to_string()).collect();
        let status_str = status.to_db_string();
        let count = tx_hashes.len();
        let now = Utc::now();

        // Start a transaction
        let mut tx = self.pool.begin().await?;

        // 1. Insert into finished_events and delete from events in a single transaction
        let insert_result = query(
            r#"
            WITH moved_events AS (
                DELETE FROM events 
                WHERE tx_hash = ANY($1::text[])
                RETURNING *
            )
            INSERT INTO finished_events (
                tx_hash, status, event_type, src_chain_id, dst_chain_id,
                msg_sender, amount, target_function, market, received_at_block, should_request_proof_at_block,
                journal_index, batch_tx_hash, received_at, processed_at, proof_requested_at, proof_received_at,
                batch_submitted_at, batch_included_at, tx_finished_at,
                resubmitted, error
            )
            SELECT 
                tx_hash, 
                $2::event_status,  -- New status
                event_type, 
                src_chain_id, 
                dst_chain_id,
                msg_sender, 
                amount, 
                target_function, 
                market, 
                received_at_block, 
                should_request_proof_at_block,
                journal_index, 
                batch_tx_hash, 
                received_at, 
                processed_at, 
                proof_requested_at, 
                proof_received_at,
                batch_submitted_at, 
                batch_included_at, 
                $3,  -- tx_finished_at
                resubmitted, 
                error
            FROM moved_events
            ON CONFLICT (tx_hash) DO NOTHING
            "#
        )
        .bind(&tx_hashes_str)
        .bind(&status_str)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        // Commit the transaction
        tx.commit().await?;

        let rows_affected = insert_result.rows_affected();
        if rows_affected == count as u64 {
            info!("Successfully migrated {} events to finished_events with status {}", count, status_str);
        } else {
            warn!(
                "Attempted to migrate {} events, but {} rows were affected. Some events might not exist.",
                count,
                rows_affected
            );
        }

        Ok(())
    }
}


