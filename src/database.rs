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
use std::time::Duration;

#[derive(Clone)]
pub struct ChainParams {
    pub max_volume: i32,
    pub time_interval: Duration,
    pub block_delay: u64,
    pub reorg_protection: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventStatus {
    Received,
    Processed,
    ReorgSecurityDelay,
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
            EventStatus::ReorgSecurityDelay => "ReorgSecurityDelay",
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
    pub received_block_timestamp: Option<i64>,
    pub received_at_block: Option<i32>,
    pub should_request_proof_at_block: Option<i32>,
    pub journal_index: Option<i32>,
    pub journal: Option<Bytes>,
    pub seal: Option<Bytes>,
    pub batch_tx_hash: Option<String>,
    pub received_at: Option<DateTime<Utc>>,
    pub proof_requested_at: Option<DateTime<Utc>>,
    pub proof_received_at: Option<DateTime<Utc>>,
    pub batch_submitted_at: Option<DateTime<Utc>>,
    pub batch_included_at: Option<DateTime<Utc>>,
    pub tx_finished_at: Option<DateTime<Utc>>,
    pub finished_block_timestamp: Option<i64>,
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

    pub async fn add_new_events(&self, events: &Vec<EventUpdate>, is_retarded: bool) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let mut tx = self.pool.begin().await?;

        let mut filtered_events = events.clone();
        let mut _discarded = 0;
        if is_retarded {
            let tx_hashes: Vec<String> = events.iter().map(|e| e.tx_hash.to_string()).collect();
            let rows = sqlx::query(
                r#"
                SELECT tx_hash
                FROM unnest($1::text[]) AS tx_hash
                WHERE tx_hash NOT IN (SELECT tx_hash FROM finished_events)
                  AND tx_hash NOT IN (SELECT tx_hash FROM events)
                "#
            )
            .bind(&tx_hashes)
            .fetch_all(&self.pool)
            .await?;
            let valid_hashes: std::collections::HashSet<String> = rows.iter()
                .filter_map(|row| row.try_get::<String, _>("tx_hash").ok())
                .collect();
            let orig_len = filtered_events.len();
            filtered_events.retain(|e| valid_hashes.contains(&e.tx_hash.to_string()));
            _discarded = orig_len - filtered_events.len();
        }

        for event in &filtered_events {
            // Only insert fields relevant to a newly processed event
            query(
                r#"
                INSERT INTO events (
                    tx_hash, status, event_type, src_chain_id, dst_chain_id, 
                    msg_sender, amount, target_function, market,
                    received_at_block, received_block_timestamp, should_request_proof_at_block,
                    received_at
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
            .bind(event.received_block_timestamp.map(|ts| ts as i32)) // Convert i64 to i32 for database
            .bind(event.should_request_proof_at_block)
            .bind(event.received_at) // Set in event_listener
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;

        if is_retarded && !filtered_events.is_empty() {
            info!("{} retarded events added to events table", filtered_events.len());
        }

        info!("Attempted to add batch of {} new events to database", events.len());

        Ok(())
    }

    pub async fn get_ready_to_request_proof_events(
        &self, 
        delay_seconds: i64, 
        batch_limit: i64
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
            chain_counts AS (
                SELECT 
                    dst_chain_id,
                    COUNT(*) as chain_total,
                    COUNT(*) * 100.0 / SUM(COUNT(*)) OVER () as chain_percentage
                FROM ready_events
                GROUP BY dst_chain_id
            ),
            ranked_events AS (
                SELECT 
                    tx_hash,
                    dst_chain_id,
                    ROW_NUMBER() OVER (
                        PARTITION BY dst_chain_id 
                        ORDER BY received_at ASC, tx_hash ASC
                    ) as row_num
                FROM ready_events
            ),
            claim_targets AS (
                SELECT tx_hash
                FROM ranked_events r
                JOIN chain_counts c ON r.dst_chain_id = c.dst_chain_id
                WHERE 
                    -- If chain has more than its fair share, limit to fair share
                    CASE 
                        WHEN c.chain_total > ($1 * c.chain_percentage / 100) THEN
                            r.row_num <= ($1 * c.chain_percentage / 100)
                        -- Otherwise take all available events from this chain
                        ELSE TRUE
                    END
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
        .bind(batch_limit) // Bind the batch limit parameter
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
                ORDER BY received_at ASC 
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
            let amount: Option<U256> = row.try_get("amount").ok().and_then(|s| U256::from_str(s).ok());

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
            WHERE e.status = 'ReorgSecurityDelay'::event_status                -- Only consider 'Processed' events
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
        uuid: &str,
        stark_time: i32,
        snark_time: i32,
        total_cycles: i64,
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
                journal_index = id.journal_idx,
                bonsai_uuid = $5,
                stark_time = $6,
                snark_time = $7,
                total_cycles = $8
            FROM index_data id
            WHERE e.tx_hash = id.tx_hash_text
              AND e.status = 'ProofRequested'::event_status; -- Ensure we only update events we claimed
            "#
        )
        .bind(&tx_hashes)
        .bind(&journal_indices)
        .bind(journal.to_vec()) // Bind journal bytes
        .bind(seal.to_vec())    // Bind seal bytes
        .bind(uuid)            // Bind uuid
        .bind(stark_time)      // Bind stark_time
        .bind(snark_time)      // Bind snark_time
        .bind(total_cycles)    // Bind total_cycles
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
        finished_block_timestamps: &Vec<i64>,
        is_retarded: bool,
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
            ),
            timestamp_data (tx_hash_text, finished_timestamp) AS (
                SELECT * FROM UNNEST($1::text[], $4::bigint[])
            )
            INSERT INTO finished_events (
                tx_hash, status, event_type, src_chain_id, dst_chain_id,
                msg_sender, amount, target_function, market, received_at_block, received_block_timestamp, should_request_proof_at_block,
                journal_index, bonsai_uuid, stark_time, snark_time, total_cycles, batch_tx_hash, received_at, 
                proof_requested_at, proof_received_at, batch_submitted_at, batch_included_at, tx_finished_at,
                finished_block_timestamp, resubmitted, error
            )
            SELECT 
                me.tx_hash, 
                $2::event_status,  -- New status
                me.event_type, 
                me.src_chain_id, 
                me.dst_chain_id,
                me.msg_sender, 
                me.amount, 
                me.target_function, 
                me.market, 
                me.received_at_block,
                me.received_block_timestamp,
                me.should_request_proof_at_block,
                me.journal_index,
                me.bonsai_uuid,
                me.stark_time,
                me.snark_time,
                me.total_cycles,
                me.batch_tx_hash, 
                me.received_at, 
                me.proof_requested_at, 
                me.proof_received_at,
                me.batch_submitted_at, 
                me.batch_included_at, 
                $3,  -- tx_finished_at
                td.finished_timestamp,  -- finished_block_timestamp from input
                me.resubmitted, 
                me.error
            FROM moved_events me
            JOIN timestamp_data td ON me.tx_hash = td.tx_hash_text
            ON CONFLICT (tx_hash) DO NOTHING
            "#
        )
        .bind(&tx_hashes_str)
        .bind(&status_str)
        .bind(now)
        .bind(finished_block_timestamps)
        .execute(&mut *tx)
        .await?;

        // Commit the transaction
        tx.commit().await?;

        let rows_affected = insert_result.rows_affected();
        if rows_affected == count as u64 {
            if is_retarded {
                info!("Successfully migrated {} retarded events to finished_events with status {}", count, status_str);
            } else {
                info!("Successfully migrated {} events to finished_events with status {}", count, status_str);
            }
        } else {
            if is_retarded {
                warn!(
                    "Attempted to migrate {} retarded events, but {} rows were affected. Some events might not exist.",
                    count,
                    rows_affected
                );
            } else {
                warn!(
                    "Attempted to migrate {} events, but {} rows were affected. Some events might not exist.",
                    count,
                    rows_affected
                );
            }
        }

        Ok(())
    }

    pub async fn update_lane_status(
        &self,
        chain_params: &HashMap<u32, ChainParams>,
        market_prices: &HashMap<Address, f64>,
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Convert market prices to a format usable in SQL
        let market_price_pairs: Vec<(String, f64)> = market_prices
            .iter()
            .map(|(addr, price)| (addr.to_string(), *price))
            .collect();

        // Convert chain params to a format usable in SQL
        let chain_param_pairs: Vec<(i32, i32, i64, i32, i32)> = chain_params
            .iter()
            .map(|(chain_id, params)| (
                *chain_id as i32,
                params.max_volume,
                params.time_interval.as_secs() as i64,
                params.block_delay as i32,
                params.reorg_protection as i32
            ))
            .collect();

        // Single atomic query that:
        // 1. Resets volumes if time interval exceeded
        // 2. Calculates dollar amounts using market prices
        // 3. Updates events and volumes based on thresholds
        query(
            r#"
            WITH RECURSIVE
            -- Convert market prices to a table
            market_prices(market, price) AS (
                SELECT * FROM UNNEST($1::text[], $2::numeric(36, 27)[])
            ),
            -- Convert chain params to a table
            chain_params(chain_id, max_volume, time_interval, block_delay, reorg_protection) AS (
                SELECT * FROM UNNEST($3::int[], $4::int[], $5::int8[], $6::int[], $7::int[])
            ),
            -- Reset volumes if time interval exceeded
            reset_volumes AS (
                UPDATE volume_flow vf
                SET 
                    dollar_value = 0,
                    last_reset = NOW()
                FROM chain_params cp
                WHERE vf.chain_id = cp.chain_id
                  AND EXTRACT(EPOCH FROM (NOW() - vf.last_reset)) > cp.time_interval
            ),
            -- Get events with their calculated dollar amounts
            events_with_dollars AS (
                SELECT 
                    e.tx_hash,
                    e.src_chain_id,
                    e.received_at_block,
                    COALESCE(
                        CASE 
                            WHEN e.amount IS NOT NULL AND mp.price IS NOT NULL 
                            THEN (e.amount::numeric(78,0) * mp.price::numeric(36,27))::numeric(78,0)::bigint
                            ELSE 0
                        END,
                        0
                    ) as dollar_amount,
                    vf.dollar_value as current_volume,
                    cp.max_volume,
                    cp.block_delay,
                    cp.reorg_protection
                FROM events e
                LEFT JOIN market_prices mp ON e.market = mp.market
                LEFT JOIN volume_flow vf ON e.src_chain_id = vf.chain_id
                LEFT JOIN chain_params cp ON e.src_chain_id = cp.chain_id
                WHERE e.status = 'Received'::event_status
                ORDER BY e.src_chain_id, e.received_at ASC
            ),
            -- Calculate cumulative volume and determine block delays
            events_with_volume AS (
                SELECT 
                    tx_hash,
                    src_chain_id,
                    received_at_block,
                    dollar_amount,
                    current_volume,
                    max_volume,
                    block_delay,
                    reorg_protection,
                    SUM(dollar_amount) OVER (
                        PARTITION BY src_chain_id 
                        ORDER BY received_at_block ASC
                    ) + current_volume as running_volume
                FROM events_with_dollars
            ),
            -- Update events and volumes
            update_events AS (
                UPDATE events e
                SET 
                    status = 'ReorgSecurityDelay'::event_status,
                    should_request_proof_at_block = CASE 
                        WHEN ewv.running_volume > ewv.max_volume 
                        THEN ewv.received_at_block + ewv.block_delay
                        ELSE ewv.received_at_block + ewv.reorg_protection
                    END
                FROM events_with_volume ewv
                WHERE e.tx_hash = ewv.tx_hash
            ),
            -- Update volumes for events that didn't exceed max_volume
            update_volumes AS (
                UPDATE volume_flow vf
                SET dollar_value = ewv.running_volume
                FROM events_with_volume ewv
                WHERE vf.chain_id = ewv.src_chain_id
                  AND ewv.running_volume <= ewv.max_volume
            )
            SELECT 1
            "#
        )
        .bind(market_price_pairs.iter().map(|(addr, _)| addr.as_str()).collect::<Vec<_>>())
        .bind(market_price_pairs.iter().map(|(_, price)| *price).collect::<Vec<_>>())
        .bind(chain_param_pairs.iter().map(|(id, _, _, _, _)| *id).collect::<Vec<_>>())
        .bind(chain_param_pairs.iter().map(|(_, max, _, _, _)| *max).collect::<Vec<_>>())
        .bind(chain_param_pairs.iter().map(|(_, _, interval, _, _)| *interval).collect::<Vec<_>>())
        .bind(chain_param_pairs.iter().map(|(_, _, _, delay, _)| *delay).collect::<Vec<_>>())
        .bind(chain_param_pairs.iter().map(|(_, _, _, _, protection)| *protection).collect::<Vec<_>>())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn reset_stuck_events(&self, finished_sample_size: i64, multiplier: f64, batch_limit: i64, max_retries: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;

        // Check if we have enough historical data first
        let has_sufficient_data = query_scalar::<_, bool>(
            r#"
            SELECT COUNT(*) >= $1
            FROM finished_events
            WHERE status = 'TxProcessSuccess'::event_status
            "#
        )
        .bind(finished_sample_size)
        .fetch_one(&mut *tx)
        .await?;

        if !has_sufficient_data {
            info!("Insufficient historical data (less than {} successful events) to calculate timeouts. No events will be reset this cycle.", finished_sample_size);
            return Ok(());
        }

        // Calculate timeouts only if we have enough data
        let timeouts = query(
            r#"
            WITH recent_successes AS (
                SELECT 
                    GREATEST(AVG(EXTRACT(EPOCH FROM ( proof_received_at - proof_requested_at))), 20) as avg_proof_request_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (batch_submitted_at - proof_received_at))), 10) as avg_proof_generation_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (batch_included_at - batch_submitted_at))), 10) as avg_batch_submission_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (tx_finished_at - batch_included_at))), 20) as avg_batch_inclusion_time
                FROM finished_events
                WHERE status = 'TxProcessSuccess'::event_status
                GROUP BY tx_hash
                ORDER BY received_at DESC
                LIMIT $1
            )
            SELECT 
                CEIL(avg_proof_request_time * $2)::INT8 as proof_request_timeout,
                CEIL(avg_proof_generation_time * $2)::INT8 as proof_generation_timeout,
                CEIL(avg_batch_submission_time * $2)::INT8 as batch_submission_timeout,
                CEIL(avg_batch_inclusion_time * $2)::INT8 as batch_inclusion_timeout
            FROM recent_successes
            "#
        )
        .bind(finished_sample_size)
        .bind(multiplier)
        .fetch_one(&mut *tx)
        .await?;

        // Extract timeout values with proper type conversion
        let proof_request_timeout: i64 = timeouts.try_get("proof_request_timeout")?;
        let proof_generation_timeout: i64 = timeouts.try_get("proof_generation_timeout")?;
        let batch_submission_timeout: i64 = timeouts.try_get("batch_submission_timeout")?;
        let batch_inclusion_timeout: i64 = timeouts.try_get("batch_inclusion_timeout")?;

        // Combined stuck events query with batch limit
        let rows_affected = query(
            r#"
            WITH stuck_events AS (
                SELECT 
                    tx_hash,
                    status,
                    resubmitted,
                    CASE 
                        WHEN status = 'ProofRequested'::event_status AND proof_requested_at < NOW() - ($1::bigint || ' seconds')::interval THEN 'proof_requested'
                        WHEN status = 'ProofReceived'::event_status AND proof_received_at < NOW() - ($2::bigint || ' seconds')::interval THEN 'proof_received'
                        WHEN status = 'BatchSubmitted'::event_status AND batch_submitted_at < NOW() - ($3::bigint || ' seconds')::interval THEN 'batch_submitted'
                        WHEN status = 'BatchFailed'::event_status AND batch_submitted_at < NOW() - ($4::bigint || ' seconds')::interval THEN 'batch_failed'
                        ELSE NULL
                    END as stuck_type
                FROM events
                WHERE status IN ('ProofRequested', 'ProofReceived', 'BatchSubmitted', 'BatchFailed')
                  AND (resubmitted IS NULL OR resubmitted < $6)
                LIMIT $5
            )
            UPDATE events e
            SET 
                status = CASE 
                    WHEN se.resubmitted IS NULL OR se.resubmitted < $6 THEN 'Received'::event_status
                    ELSE 'Failed'::event_status
                END,
                proof_requested_at = CASE 
                    WHEN se.stuck_type = 'proof_requested' AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE proof_requested_at 
                END,
                proof_received_at = CASE 
                    WHEN se.stuck_type = 'proof_received' AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE proof_received_at 
                END,
                batch_submitted_at = CASE 
                    WHEN se.stuck_type IN ('batch_submitted', 'batch_failed') AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE batch_submitted_at 
                END,
                batch_included_at = CASE 
                    WHEN se.stuck_type IN ('batch_submitted', 'batch_failed') AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE batch_included_at 
                END,
                tx_finished_at = CASE 
                    WHEN se.stuck_type IN ('batch_submitted', 'batch_failed') AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE tx_finished_at 
                END,
                journal = CASE 
                    WHEN se.stuck_type = 'proof_received' AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE journal 
                END,
                seal = CASE 
                    WHEN se.stuck_type = 'proof_received' AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE seal 
                END,
                journal_index = CASE 
                    WHEN se.stuck_type = 'proof_received' AND (se.resubmitted IS NULL OR se.resubmitted < $6) THEN NULL 
                    ELSE journal_index 
                END,
                error = CASE 
                    WHEN se.resubmitted IS NOT NULL AND se.resubmitted + 1 >= $6 THEN 
                        CASE se.stuck_type
                            WHEN 'proof_requested' THEN 'Proof request timeout after retry'
                            WHEN 'proof_received' THEN 'Proof generation timeout after retry'
                            WHEN 'batch_submitted' THEN 'Batch submission timeout after retry'
                            WHEN 'batch_failed' THEN 'Batch failed timeout after retry'
                        END
                    ELSE NULL
                END,
                resubmitted = COALESCE(se.resubmitted, 0) + 1
            FROM stuck_events se
            WHERE e.tx_hash = se.tx_hash
              AND se.stuck_type IS NOT NULL
            "#
        )
        .bind(proof_request_timeout)
        .bind(proof_generation_timeout)
        .bind(batch_submission_timeout)
        .bind(batch_inclusion_timeout)
        .bind(batch_limit)
        .bind(max_retries)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        // Delete BatchFailed events that are in finished_events
        let deleted_batch_failed_rows = query(
            r#"
            DELETE FROM events e
            USING finished_events fe
            WHERE e.tx_hash = fe.tx_hash
              AND e.status = 'BatchFailed'::event_status
              AND e.batch_submitted_at < NOW() - ($1::bigint || ' seconds')::interval
            "#
        )
        .bind(batch_inclusion_timeout)
        .execute(&mut *tx)
        .await?
        .rows_affected();

        tx.commit().await?;

        if rows_affected > 0 || deleted_batch_failed_rows > 0 {
            info!(
                "Reset {} stuck events and deleted {} batch failed events from finished_events",
                rows_affected, deleted_batch_failed_rows
            );
        }

        Ok(())
    }

    pub async fn sequencer_start_events_reset(&self) -> Result<()> {
        let rows_affected = query(
            r#"
            UPDATE events
            SET 
                status = 'ReadyToRequestProof'::event_status,
                proof_requested_at = NULL
            WHERE status = 'ProofRequested'::event_status
            "#
        )
        .execute(&self.pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!("Reset {} events from ProofRequested back to ReadyToRequestProof", rows_affected);
        } else {
            debug!("No events were reset from ProofRequested status");
        }

        Ok(())
    }

    // Add a new record to the node_status table
    pub async fn add_to_node_status(
        &self,
        chain_id: u64,
        primary_url: &str,
        fallback_url: &str,
        reason: &str
    ) -> Result<()> {
        query(
            r#"
            INSERT INTO node_status (chain_id, primary_url, fallback_url, reason)
            VALUES ($1, $2, $3, $4)
            "#
        )
        .bind(chain_id as i32)
        .bind(primary_url)
        .bind(fallback_url)
        .bind(reason)
        .execute(&self.pool)
        .await?;

        debug!("Recorded node failure for chain {}: {}", chain_id, reason);
        Ok(())
    }

    pub async fn get_failed_tx(&self, max_retries: i64) -> Result<Vec<(u32, Address, U256, Vec<TxHash>)>> {
        use sqlx::Row;
        use std::str::FromStr;
        use alloy::primitives::{Address, TxHash, U256};

        // Query: group by dst_chain_id and market, sum amounts, collect tx_hashes
        let rows = sqlx::query(
            r#"
            SELECT 
                dst_chain_id, 
                market, 
                SUM(amount::numeric)::text as total_amount, 
                array_agg(tx_hash) as tx_hashes
            FROM finished_events
            WHERE status = 'TxProcessFail'::event_status
              AND event_type IN ('HostWithdraw', 'HostBorrow', 'ExtensionSupply')
              AND (resubmitted IS NULL OR resubmitted < $1)
            GROUP BY dst_chain_id, market
            "#
        )
        .bind(max_retries)
        .fetch_all(&self.pool)
        .await?;

        let mut result = Vec::new();
        for row in rows {
            // Parse dst_chain_id
            let dst_chain_id: Option<i32> = row.try_get("dst_chain_id").ok();
            let dst_chain_id = match dst_chain_id {
                Some(id) => id as u32,
                None => continue, // skip if missing
            };
            // Parse market
            let market: Option<String> = row.try_get("market").ok();
            let market = match market {
                Some(ref s) => match Address::from_str(s) {
                    Ok(addr) => addr,
                    Err(_) => continue, // skip if invalid
                },
                None => continue, // skip if missing
            };
            // Parse total_amount
            let total_amount: Option<&str> = row.try_get("total_amount").ok();
            let total_amount = match total_amount {
                Some(s) => U256::from_str(s).unwrap_or(U256::from(0)),
                None => U256::from(0),
            };
            // Parse tx_hashes
            let tx_hashes: Option<Vec<String>> = row.try_get("tx_hashes").ok();
            let tx_hashes = match tx_hashes {
                Some(vec) => vec.into_iter().filter_map(|h| TxHash::from_str(&h).ok()).collect(),
                None => Vec::new(),
            };
            result.push((dst_chain_id, market, total_amount, tx_hashes));
        }
        Ok(result)
    }

    pub async fn reset_failed_transactions(&self, tx_hashes: &Vec<TxHash>) -> Result<()> {
        if tx_hashes.is_empty() {
            return Ok(());
        }
        let tx_hashes_str: Vec<String> = tx_hashes.iter().map(|h| h.to_string()).collect();
        let mut tx = self.pool.begin().await?;
        // Move the specified fields from finished_events to events, set status to ProofReceived
        sqlx::query(
            r#"
            WITH moved AS (
                DELETE FROM finished_events
                WHERE tx_hash = ANY($1::text[])
                RETURNING 
                    tx_hash, event_type, src_chain_id, dst_chain_id, msg_sender, amount, target_function, market, received_block_timestamp, received_at_block, received_at, resubmitted
            )
            INSERT INTO events (
                tx_hash, status, event_type, src_chain_id, dst_chain_id, msg_sender, amount, target_function, market, received_block_timestamp, received_at_block, received_at, resubmitted
            )
            SELECT 
                tx_hash, 'Received'::event_status, event_type, src_chain_id, dst_chain_id, msg_sender, amount, target_function, market, received_block_timestamp, received_at_block, received_at, COALESCE(resubmitted, 0) + 1
            FROM moved
            ON CONFLICT (tx_hash) DO NOTHING
            "#
        )
        .bind(&tx_hashes_str)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }
}


