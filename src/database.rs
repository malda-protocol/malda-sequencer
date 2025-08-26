//! # Database Module
//!
//! This module provides the core database functionality for the sequencer system,
//! managing event lifecycle, proof generation, batch processing, and transaction
//! status tracking across multiple chains.
//!
//! ## Key Features:
//! - **Event Lifecycle Management**: Tracks events from reception to completion
//! - **Proof Generation Coordination**: Manages proof request timing and journal indexing
//! - **Batch Processing**: Handles batch submission and inclusion tracking
//! - **Cross-Chain Support**: Manages events across multiple destination chains
//! - **Volume Flow Control**: Implements lane-based volume management
//! - **Stuck Event Recovery**: Automatically resets stuck events with retry logic
//! - **Transaction Status Tracking**: Monitors transaction success/failure states
//!
//! ## Architecture:
//! ```
//! Database Operations
//! ├── Event Management: Add, update, and track event status
//! ├── Proof Coordination: Request timing and journal management
//! ├── Batch Processing: Submission and inclusion tracking
//! ├── Volume Control: Lane-based flow management
//! ├── Recovery Systems: Stuck event detection and reset
//! └── Transaction Tracking: Success/failure monitoring
//! ```
//!
//! ## Event Lifecycle:
//! 1. **Received**: Event initially received from blockchain
//! 2. **Processed**: Event parsed and validated
//! 3. **ReorgSecurityDelay**: Waiting for reorg protection
//! 4. **ReadyToRequestProof**: Ready for proof generation
//! 5. **ProofRequested**: Proof generation initiated
//! 6. **ProofReceived**: Proof generated and stored
//! 7. **BatchSubmitted**: Included in batch submission
//! 8. **BatchIncluded**: Batch confirmed on blockchain
//! 9. **TxProcessSuccess/Fail**: Final transaction status
//!
//! ## Database Schema:
//! - **events**: Active events being processed
//! - **finished_events**: Completed events with final status
//! - **volume_flow**: Chain-specific volume tracking
//! - **chain_batch_sync**: Chain-specific batch timing
//! - **sync_timestamps**: Global synchronization timestamps
//! - **node_status**: Provider node health tracking

use alloy::primitives::{Address, Bytes, TxHash, U256};
use chrono::{DateTime, Utc};
use eyre::Result;
use hex; // Ensure hex is imported if used in logs
use serde::{Deserialize, Serialize};
use sqlx::{query, query_as, query_scalar, Pool, Postgres, Row};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use tracing::info;
use tracing::{debug, warn};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::timeout;
use std::sync::atomic::{AtomicBool, Ordering};

/// Chain-specific parameters for volume flow management
///
/// This struct defines the configuration parameters for managing
/// volume flow on a per-chain basis, including maximum volume
/// thresholds, time intervals, and protection mechanisms.
#[derive(Clone)]
pub struct ChainParams {
    /// Maximum volume allowed for the chain in USD
    pub max_volume: i32,
    /// Time interval for volume reset in seconds
    pub time_interval: Duration,
    /// Block delay for volume-based processing
    pub block_delay: u64,
    /// Reorg protection delay in blocks
    pub reorg_protection: u64,
}

/// Database configuration for primary and fallback connections
///
/// This struct defines the configuration parameters for database
/// connections, including primary and fallback URLs, health monitoring
/// intervals, and failure thresholds.
#[derive(Clone)]
pub struct DatabaseConfig {
    /// Primary database connection URL
    pub primary_url: String,
    /// Fallback database connection URL (optional)
    pub fallback_url: Option<String>,
    /// Health check interval in seconds
    pub health_check_interval: Duration,
    /// Number of consecutive failures before switching to fallback
    pub failure_threshold: u32,
    /// Number of consecutive successes before switching back to primary
    pub recovery_threshold: u32,
    /// Timeout for health check queries in seconds
    pub health_check_timeout: Duration,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            primary_url: String::new(),
            fallback_url: None,
            health_check_interval: Duration::from_secs(5),
            failure_threshold: 3,
            recovery_threshold: 2,
            health_check_timeout: Duration::from_secs(2),
        }
    }
}

/// Event status enumeration for tracking event lifecycle
///
/// This enum represents all possible states an event can be in
/// during its processing lifecycle, from initial reception to
/// final completion or failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EventStatus {
    /// Event has been received from the blockchain
    Received,
    /// Event has been processed and validated
    Processed,
    /// Event is in reorg security delay period
    ReorgSecurityDelay,
    /// Event has been included in a batch
    IncludedInBatch,
    /// Event is ready to request proof generation
    ReadyToRequestProof,
    /// Proof generation has been requested
    ProofRequested,
    /// Proof has been received and stored
    ProofReceived,
    /// Batch containing the event has been submitted
    BatchSubmitted,
    /// Batch containing the event has been included on blockchain
    BatchIncluded,
    /// Batch submission failed with error details
    BatchFailed { error: String },
    /// Transaction processing completed successfully
    TxProcessSuccess,
    /// Transaction processing failed
    TxProcessFail,
    /// Event processing failed with error details
    Failed { error: String },
}

impl EventStatus {
    /// Converts the event status to its database string representation
    ///
    /// This method maps each enum variant to its corresponding
    /// database string value for storage and retrieval.
    ///
    /// # Returns
    /// * `String` - Database string representation of the status
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

/// Event update structure for database operations
///
/// This struct contains all the fields that can be updated for an event,
/// including transaction details, status information, proof data, and
/// timing information for various processing stages.
#[derive(Debug, Clone, Default)]
pub struct EventUpdate {
    /// Transaction hash of the event
    pub tx_hash: TxHash,
    /// Current status of the event
    pub status: EventStatus,
    /// Type of event (e.g., "HostWithdraw", "HostBorrow")
    pub event_type: Option<String>,
    /// Source chain ID
    pub src_chain_id: Option<u32>,
    /// Destination chain ID
    pub dst_chain_id: Option<u32>,
    /// Message sender address
    pub msg_sender: Option<Address>,
    /// Transaction amount
    pub amount: Option<U256>,
    /// Target function name
    pub target_function: Option<String>,
    /// Market address
    pub market: Option<Address>,
    /// Block timestamp when event was received
    pub received_block_timestamp: Option<i64>,
    /// Block number when event was received
    pub received_at_block: Option<i32>,
    /// Block number when proof should be requested
    pub should_request_proof_at_block: Option<i32>,
    /// Journal index for proof ordering
    pub journal_index: Option<i32>,
    /// Journal data for proof verification
    pub journal: Option<Bytes>,
    /// Seal data for proof verification
    pub seal: Option<Bytes>,
    /// Batch transaction hash
    pub batch_tx_hash: Option<String>,
    /// Timestamp when event was received
    pub received_at: Option<DateTime<Utc>>,
    /// Timestamp when proof was requested
    pub proof_requested_at: Option<DateTime<Utc>>,
    /// Timestamp when proof was received
    pub proof_received_at: Option<DateTime<Utc>>,
    /// Timestamp when batch was submitted
    pub batch_submitted_at: Option<DateTime<Utc>>,
    /// Timestamp when batch was included
    pub batch_included_at: Option<DateTime<Utc>>,
    /// Timestamp when transaction was finished
    pub tx_finished_at: Option<DateTime<Utc>>,
    /// Block timestamp when transaction finished
    pub finished_block_timestamp: Option<i64>,
    /// Number of times event has been resubmitted
    pub resubmitted: Option<i32>,
    /// Error message if event failed
    pub error: Option<String>,
}

/// Database connection and operations manager
///
/// This struct provides the main interface for all database operations,
/// including event management, proof coordination, batch processing,
/// and transaction status tracking.
///
/// ## Key Responsibilities:
///
/// - **Event Lifecycle Management**: Track events through all processing stages
/// - **Proof Generation Coordination**: Manage proof request timing and journal indexing
/// - **Batch Processing**: Handle batch submission and inclusion tracking
/// - **Volume Flow Control**: Implement lane-based volume management
/// - **Stuck Event Recovery**: Automatically detect and reset stuck events
/// - **Transaction Status Tracking**: Monitor transaction success/failure states
/// - **Fallback Database Support**: Automatic failover to backup database
/// - **Health Monitoring**: Continuous monitoring of database connectivity
#[derive(Clone)]
pub struct Database {
    /// Primary PostgreSQL connection pool (None if failed to initialize)
    pub pool: Option<Pool<Postgres>>,
    /// Fallback PostgreSQL connection pool (optional)
    fallback_pool: Option<Pool<Postgres>>,
    /// Configuration for database connections and health monitoring
    config: DatabaseConfig,
    /// Current active pool (primary or fallback)
    active_pool: Arc<RwLock<Pool<Postgres>>>,
    /// Health monitoring state
    health_state: Arc<HealthState>,
}

/// Internal health monitoring state
struct HealthState {
    /// Whether currently using fallback database
    using_fallback: AtomicBool,
    /// Number of consecutive failures for active database
    active_failures: Arc<RwLock<u32>>,
    /// Number of consecutive successes for secondary database
    secondary_successes: Arc<RwLock<u32>>,
    /// Whether health monitoring is running
    monitoring_active: AtomicBool,
}

impl Database {
    /// Creates a new database connection with automatic migration support
    ///
    /// This method initializes a new database connection with the provided
    /// connection string, sets up SSL connections, and runs any pending
    /// database migrations automatically.
    ///
    /// # Arguments
    /// * `database_url` - PostgreSQL connection string
    ///
    /// # Returns
    /// * `Result<Self>` - Database instance or error
    ///
    /// # Example
    /// ```rust
    /// let db = Database::new("postgresql://user:pass@localhost/db").await?;
    /// ```
    pub async fn new(database_url: &str) -> Result<Self> {
        let pool = sqlx::PgPool::connect_with(
            sqlx::postgres::PgConnectOptions::from_str(database_url)?
                .ssl_mode(sqlx::postgres::PgSslMode::Require)
        )
        .await?;

        // Run migrations
        sqlx::migrate!("./migrations").run(&pool).await?;

        info!("Database initialized successfully");

        // Create a simple config for the basic constructor
        let config = DatabaseConfig {
            primary_url: database_url.to_string(),
            fallback_url: None,
            health_check_interval: Duration::from_secs(5),
            failure_threshold: 3,
            recovery_threshold: 2,
            health_check_timeout: Duration::from_secs(2),
        };

        // Initialize health state
        let health_state = Arc::new(HealthState {
            using_fallback: AtomicBool::new(false),
            active_failures: Arc::new(RwLock::new(0)),
            secondary_successes: Arc::new(RwLock::new(0)),
            monitoring_active: AtomicBool::new(false),
        });

        let active_pool = Arc::new(RwLock::new(pool.clone()));

        Ok(Self { 
            pool: Some(pool),
            fallback_pool: None,
            config,
            active_pool,
            health_state,
        })
    }

    /// Creates a new database connection with fallback support and health monitoring
    ///
    /// This method initializes database connections with primary and optional fallback
    /// URLs, sets up SSL connections, runs migrations, and starts health monitoring.
    ///
    /// # Arguments
    /// * `config` - Database configuration including primary and fallback URLs
    ///
    /// # Returns
    /// * `Result<Self>` - Database instance or error
    ///
    /// # Example
    /// ```rust
    /// let config = DatabaseConfig {
    ///     primary_url: "postgresql://user:pass@primary/db".to_string(),
    ///     fallback_url: Some("postgresql://user:pass@fallback/db".to_string()),
    ///     health_check_interval: Duration::from_secs(5),
    ///     failure_threshold: 3,
    ///     recovery_threshold: 2,
    ///     health_check_timeout: Duration::from_secs(2),
    /// };
    /// let db = Database::new_with_fallback(config).await?;
    /// ```
    pub async fn new_with_fallback(config: DatabaseConfig) -> Result<Self> {
        // Try to create primary pool
        let primary_pool = match sqlx::postgres::PgConnectOptions::from_str(&config.primary_url) {
            Ok(connect_options) => {
                match sqlx::PgPool::connect_with(connect_options.ssl_mode(sqlx::postgres::PgSslMode::Require)).await {
                    Ok(pool) => {
                        info!("Primary database pool created successfully");
                        Some(pool)
                    }
                    Err(e) => {
                        debug!("Failed to create primary database pool: {:?}", e);
                        None
                    }
                }
            }
            Err(e) => {
                debug!("Failed to parse primary database URL during health check: {:?}", e);
                None
            }
        };

        // Create fallback pool if provided
        let fallback_pool = if let Some(ref fallback_url) = config.fallback_url {
            match sqlx::postgres::PgConnectOptions::from_str(fallback_url) {
                Ok(connect_options) => {
                    match sqlx::PgPool::connect_with(connect_options.ssl_mode(sqlx::postgres::PgSslMode::Require)).await {
                        Ok(pool) => {
                            info!("Fallback database pool created successfully");
                            Some(pool)
                        }
                        Err(e) => {
                            warn!("Failed to create fallback database pool: {:?}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to parse fallback database URL: {:?}", e);
                    None
                }
            }
        } else {
            None
        };

        // Run migrations on primary if it's working
        if let Some(ref primary_pool) = primary_pool {
            if let Err(e) = sqlx::migrate!("./migrations").run(primary_pool).await {
                warn!("Failed to run migrations on primary database: {:?}", e);
            }
        }

        // Run migrations on fallback if available
        if let Some(ref fallback_pool) = fallback_pool {
            if let Err(e) = sqlx::migrate!("./migrations").run(fallback_pool).await {
                warn!("Failed to run migrations on fallback database: {:?}", e);
            }
        }

        // Initialize health state
        let health_state = Arc::new(HealthState {
            using_fallback: AtomicBool::new(false),
            active_failures: Arc::new(RwLock::new(0)),
            secondary_successes: Arc::new(RwLock::new(0)),
            monitoring_active: AtomicBool::new(false),
        });

        // Determine initial active pool
        let initial_pool = if let Some(ref fallback_pool) = fallback_pool {
            if primary_pool.is_none() {
                info!("Primary database unavailable, starting with fallback database");
                health_state.using_fallback.store(true, Ordering::Relaxed);
                fallback_pool.clone()
            } else {
                primary_pool.as_ref().unwrap().clone()
            }
        } else if let Some(ref primary_pool) = primary_pool {
            primary_pool.clone()
        } else {
            return Err(eyre::eyre!("No database pools available"));
        };

        let active_pool = Arc::new(RwLock::new(initial_pool));

        let db = Self {
            pool: primary_pool,
            fallback_pool,
            config,
            active_pool,
            health_state,
        };

        // Start health monitoring
        db.start_health_monitoring().await;

        info!("Database with fallback support initialized successfully");

        Ok(db)
    }

    /// Creates a new database connection with simple fallback support
    ///
    /// This is a convenience method that creates a DatabaseConfig with
    /// default health monitoring settings and calls new_with_fallback.
    ///
    /// # Arguments
    /// * `primary_url` - Primary database connection string
    /// * `fallback_url` - Optional fallback database connection string
    ///
    /// # Returns
    /// * `Result<Self>` - Database instance or error
    ///
    /// # Example
    /// ```rust
    /// let db = Database::new_with_simple_fallback(
    ///     "postgresql://user:pass@primary/db",
    ///     Some("postgresql://user:pass@fallback/db".to_string())
    /// ).await?;
    /// ```
    pub async fn new_with_simple_fallback(
        primary_url: &str,
        fallback_url: Option<String>,
    ) -> Result<Self> {
        let config = DatabaseConfig {
            primary_url: primary_url.to_string(),
            fallback_url,
            ..Default::default()
        };

        Self::new_with_fallback(config).await
    }



    /// Starts the background health monitoring task
    ///
    /// This method spawns a background task that continuously monitors
    /// the health of both primary and fallback databases and automatically
    /// switches between them based on availability.
    async fn start_health_monitoring(&self) {
        if self.health_state.monitoring_active.load(Ordering::Relaxed) {
            return; // Already monitoring
        }

        self.health_state.monitoring_active.store(true, Ordering::Relaxed);

        let health_state = self.health_state.clone();
        let config = self.config.clone();
        let active_pool = self.active_pool.clone();
        let primary_pool = self.pool.clone();
        let fallback_pool = self.fallback_pool.clone();

        // Clone config for logging before moving into async block
        let config_for_logging = config.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.health_check_interval);
            let mut status_interval = tokio::time::interval(Duration::from_secs(60)); // Status log every minute
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if !health_state.monitoring_active.load(Ordering::Relaxed) {
                            break;
                        }

                        let using_fallback = health_state.using_fallback.load(Ordering::Relaxed);
                        
                        if using_fallback {
                            // Currently using fallback, check both databases
                            Self::check_fallback_health(
                                &health_state,
                                &config,
                                &active_pool,
                                &primary_pool,
                                &fallback_pool,
                            ).await;
                        } else {
                            // Currently using primary, check primary health
                            Self::check_primary_health(
                                &health_state,
                                &config,
                                &active_pool,
                                &primary_pool,
                                &fallback_pool,
                            ).await;
                        }
                    }
                    _ = status_interval.tick() => {
                        if !health_state.monitoring_active.load(Ordering::Relaxed) {
                            break;
                        }

                        // Log status every minute
                        let using_fallback = health_state.using_fallback.load(Ordering::Relaxed);
                        let secondary_successes = *health_state.secondary_successes.read().await;

                        info!("Database Status: Active={}, Secondary(successes={})", 
                              if using_fallback { "Fallback" } else { "Primary" },
                              secondary_successes);
                    }
                }
            }
        });

        info!("Health monitoring started with interval: {:?}", config_for_logging.health_check_interval);
    }

    /// Checks primary database health and switches to fallback if needed
    async fn check_primary_health(
        health_state: &Arc<HealthState>,
        config: &DatabaseConfig,
        active_pool: &Arc<RwLock<Pool<Postgres>>>,
        primary_pool: &Option<Pool<Postgres>>,
        fallback_pool: &Option<Pool<Postgres>>,
    ) {
        // Try to initialize primary pool if it's None
        let primary_pool = if primary_pool.is_none() {
            match sqlx::postgres::PgConnectOptions::from_str(&config.primary_url) {
                Ok(connect_options) => {
                    match sqlx::PgPool::connect_with(connect_options.ssl_mode(sqlx::postgres::PgSslMode::Require)).await {
                        Ok(pool) => {
                            debug!("Primary database pool initialized successfully during health check");
                            Some(pool)
                        }
                        Err(e) => {
                            debug!("Failed to initialize primary database pool during health check: {:?}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to parse primary database URL during health check: {:?}", e);
                    None
                }
            }
        } else {
            primary_pool.clone()
        };

        // Check primary health if pool exists
        if let Some(ref primary_pool) = primary_pool {
            match timeout(config.health_check_timeout, Self::ping_database(primary_pool)).await {
                Ok(Ok(_)) => {
                    // Primary is healthy, reset failures
                    let mut active_failures = health_state.active_failures.write().await;
                    *active_failures = 0;
                    
                    debug!("Primary database is healthy");
                }
                Ok(Err(e)) => {
                    // Primary is unhealthy
                    let mut active_failures = health_state.active_failures.write().await;
                    *active_failures += 1;
                    
                    debug!("Primary database health check failed (failures: {}): {:?}", *active_failures, e);
                    
                    // Check if we should switch to fallback
                    if *active_failures >= config.failure_threshold {
                        if let Some(fallback_pool) = fallback_pool {
                            // Check if fallback is healthy before switching
                            match timeout(config.health_check_timeout, Self::ping_database(fallback_pool)).await {
                                Ok(Ok(_)) => {
                                    // Fallback is healthy, switch to it
                                    health_state.using_fallback.store(true, Ordering::Relaxed);
                                    *active_pool.write().await = fallback_pool.clone();
                                    
                                    // Reset counters when switching
                                    let mut secondary_successes = health_state.secondary_successes.write().await;
                                    *secondary_successes = 0;
                                    *active_failures = 0;
                                    
                                    info!("Switched to fallback database after {} consecutive primary failures", *active_failures);
                                }
                                Ok(Err(e)) => {
                                    warn!("Both primary and fallback databases are unhealthy. Primary failures: {}, Fallback error: {:?}", *active_failures, e);
                                }
                                Err(_) => {
                                    warn!("Both primary and fallback databases are unhealthy. Primary failures: {}, Fallback timeout", *active_failures);
                                }
                            }
                        } else {
                            warn!("Primary database unhealthy (failures: {}) but no fallback available", *active_failures);
                        }
                    }
                }
                Err(_) => {
                    // Primary timeout
                    let mut active_failures = health_state.active_failures.write().await;
                    *active_failures += 1;
                    
                    debug!("Primary database health check timed out (failures: {})", *active_failures);
                    
                    // Check if we should switch to fallback
                    if *active_failures >= config.failure_threshold {
                        if let Some(fallback_pool) = fallback_pool {
                            // Check if fallback is healthy before switching
                            match timeout(config.health_check_timeout, Self::ping_database(fallback_pool)).await {
                                Ok(Ok(_)) => {
                                    // Fallback is healthy, switch to it
                                    health_state.using_fallback.store(true, Ordering::Relaxed);
                                    *active_pool.write().await = fallback_pool.clone();
                                    
                                    // Reset counters when switching
                                    let mut secondary_successes = health_state.secondary_successes.write().await;
                                    *secondary_successes = 0;
                                    *active_failures = 0;
                                    
                                    info!("Switched to fallback database after {} consecutive primary failures (timeout)", *active_failures);
                                }
                                Ok(Err(e)) => {
                                    warn!("Both primary and fallback databases are unhealthy. Primary failures: {}, Fallback error: {:?}", *active_failures, e);
                                }
                                Err(_) => {
                                    warn!("Both primary and fallback databases are unhealthy. Primary failures: {}, Fallback timeout", *active_failures);
                                }
                            }
                        } else {
                            warn!("Primary database unhealthy (failures: {}) but no fallback available", *active_failures);
                        }
                    }
                }
            }
        } else {
            // Primary pool is None, increment failures
            let mut active_failures = health_state.active_failures.write().await;
            *active_failures += 1;
            debug!("Primary database pool is None (failures: {})", *active_failures);
        }
    }

    /// Checks fallback database health and switches back to primary if possible
    async fn check_fallback_health(
        health_state: &Arc<HealthState>,
        config: &DatabaseConfig,
        active_pool: &Arc<RwLock<Pool<Postgres>>>,
        primary_pool: &Option<Pool<Postgres>>,
        fallback_pool: &Option<Pool<Postgres>>,
    ) {
        // Try to initialize primary pool if it's None
        let primary_pool = if primary_pool.is_none() {
            match sqlx::postgres::PgConnectOptions::from_str(&config.primary_url) {
                Ok(connect_options) => {
                    match sqlx::PgPool::connect_with(connect_options.ssl_mode(sqlx::postgres::PgSslMode::Require)).await {
                        Ok(pool) => {
                            debug!("Primary database pool initialized successfully during health check");
                            Some(pool)
                        }
                        Err(e) => {
                            debug!("Failed to initialize primary database pool during health check: {:?}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    debug!("Failed to parse primary database URL during health check: {:?}", e);
                    None
                }
            }
        } else {
            primary_pool.clone()
        };

        // Check primary health first - if it's healthy, switch back immediately
        if let Some(ref primary_pool) = primary_pool {
            match timeout(config.health_check_timeout, Self::ping_database(primary_pool)).await {
                Ok(Ok(_)) => {
                    // Primary is healthy, switch back to it immediately
                    health_state.using_fallback.store(false, Ordering::Relaxed);
                    *active_pool.write().await = primary_pool.clone();
                    
                    // Reset counters when switching
                    let mut active_failures = health_state.active_failures.write().await;
                    *active_failures = 0;
                    let mut secondary_successes = health_state.secondary_successes.write().await;
                    *secondary_successes = 0;
                    
                    info!("Switched back to primary database (primary is healthy)");
                }
                Ok(Err(e)) => {
                    // Primary is still unhealthy
                    debug!("Primary database still unhealthy: {:?}", e);
                }
                Err(_) => {
                    // Primary timeout
                    debug!("Primary database still unhealthy (timeout)");
                }
            }
        } else {
            // Primary pool is None
            debug!("Primary database pool is None");
        }

        // Check fallback health (only track successes for secondary)
        if let Some(fallback_pool) = fallback_pool {
            match timeout(config.health_check_timeout, Self::ping_database(fallback_pool)).await {
                Ok(Ok(_)) => {
                    // Fallback is healthy, increment successes
                    let mut secondary_successes = health_state.secondary_successes.write().await;
                    *secondary_successes += 1;
                    
                    debug!("Fallback database health check passed (successes: {})", *secondary_successes);
                }
                Ok(Err(e)) => {
                    // Fallback is unhealthy, reset successes
                    let mut secondary_successes = health_state.secondary_successes.write().await;
                    *secondary_successes = 0;
                    
                    warn!("Fallback database health check failed: {:?}", e);
                }
                Err(_) => {
                    // Fallback timeout, reset successes
                    let mut secondary_successes = health_state.secondary_successes.write().await;
                    *secondary_successes = 0;
                    
                    warn!("Fallback database health check timed out");
                }
            }
        }
    }

    /// Performs a simple ping query to check database health
    async fn ping_database(pool: &Pool<Postgres>) -> Result<()> {
        query("SELECT 1").execute(pool).await?;
        Ok(())
    }

    /// Stops the health monitoring task
    pub fn stop_health_monitoring(&self) {
        self.health_state.monitoring_active.store(false, Ordering::Relaxed);
        info!("Health monitoring stopped");
    }

    /// Returns whether currently using fallback database
    pub fn is_using_fallback(&self) -> bool {
        self.health_state.using_fallback.load(Ordering::Relaxed)
    }

    /// Returns the current active pool for database operations
    async fn get_active_pool(&self) -> Pool<Postgres> {
        self.active_pool.read().await.clone()
    }

    /// Adds new events to the database with retarded event filtering
    ///
    /// This method adds a batch of new events to the database, with special
    /// handling for retarded events that filters out events already in the
    /// finished_events table or active events table.
    ///
    /// ## Retarded Event Handling:
    ///
    /// For retarded events, the method performs additional filtering to ensure
    /// only truly new events are added, preventing duplicate processing of
    /// events that may have been processed in previous runs.
    ///
    /// # Arguments
    /// * `events` - Vector of events to add to the database
    /// * `is_retarded` - Whether these are retarded events requiring special filtering
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let events = vec![event1, event2, event3];
    /// db.add_new_events(&events, false).await?;
    /// ```
    pub async fn add_new_events(&self, events: &Vec<EventUpdate>, is_retarded: bool) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let active_pool = self.get_active_pool().await;
        let mut tx = active_pool.begin().await?;

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
                "#,
            )
            .bind(&tx_hashes)
            .fetch_all(&active_pool)
            .await?;
            let valid_hashes: std::collections::HashSet<String> = rows
                .iter()
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
                "#,
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
            info!(
                "{} retarded events added to events table",
                filtered_events.len()
            );
        }

        info!(
            "Attempted to add batch of {} new events to database",
            events.len()
        );

        Ok(())
    }

    /// Retrieves events ready for proof generation with rate limiting
    ///
    /// This method implements a sophisticated proof request system that:
    /// 1. Checks rate limiting using global timestamps
    /// 2. Ensures fair distribution across chains
    /// 3. Claims events atomically to prevent race conditions
    /// 4. Applies batch limits with chain-specific quotas
    ///
    /// ## Rate Limiting:
    ///
    /// The method uses a global timestamp table to ensure proof requests
    /// are not made too frequently, preventing overwhelming the proof
    /// generation system.
    ///
    /// ## Fair Distribution:
    ///
    /// Events are distributed fairly across chains based on their
    /// percentage of total ready events, ensuring no single chain
    /// dominates the proof generation queue.
    ///
    /// # Arguments
    /// * `delay_seconds` - Minimum delay between proof requests in seconds
    /// * `batch_limit` - Maximum number of events to claim in this batch
    ///
    /// # Returns
    /// * `Result<Vec<EventUpdate>>` - Vector of claimed events ready for proof generation
    ///
    /// # Example
    /// ```rust
    /// let events = db.get_ready_to_request_proof_events(60, 10).await?;
    /// ```
    pub async fn get_ready_to_request_proof_events(
        &self,
        delay_seconds: i64,
        batch_limit: i64,
    ) -> Result<(Vec<EventUpdate>, Vec<EventUpdate>)> {
        let active_pool = self.get_active_pool().await;
        
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
            "#,
        )
        .bind(delay_seconds)
        .fetch_one(&active_pool) // Use fetch_one, assuming the row always exists
        .await?
        .try_get::<bool, _>("should_proceed")?;

        if !should_proceed {
            debug!(
                "Proof request delay ({}s) not met, skipping claim.",
                delay_seconds
            );
            return Ok((Vec::new(), Vec::new())); // Return empty if delay not met
        }

        // Use a transaction to ensure atomicity
        let mut tx = active_pool.begin().await?;

        // Re-added UPDATE sync_timestamps set last_proof_requested_at = NOW()
        // This acts as a lock for this cycle
        query(
            r#"
            UPDATE sync_timestamps
            SET last_proof_requested_at = NOW()
            WHERE id = 1
            "#,
        )
        .execute(tx.as_mut())
        .await?;

        // Claim events and set status/timestamp atomically, returning only needed fields
        let records = query(
            r#"
            WITH ready_events AS (
                SELECT tx_hash, dst_chain_id, received_at, msg_sender
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
                    msg_sender,
                    ROW_NUMBER() OVER (
                        PARTITION BY dst_chain_id 
                        ORDER BY received_at ASC, tx_hash ASC
                    ) as row_num
                FROM ready_events
            ),
            claim_targets AS (
                SELECT tx_hash, msg_sender
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
            "#,
        )
        .bind(batch_limit) // Bind the batch limit parameter
        .fetch_all(tx.as_mut())
        .await?;

        let number_events_claimed = records.len();

        // Commit the transaction
        tx.commit().await?;

        let mut boundless_events = Vec::new();
        let mut bonsai_events = Vec::new();
        
        if records.is_empty() {
            return Ok((bonsai_events, boundless_events));
        }



        // Get all msg_senders from the claimed events to check against boundless_users
        let msg_senders: Vec<String> = records
            .iter()
            .filter_map(|row| row.try_get::<Option<String>, _>("msg_sender").ok().flatten())
            .collect();

        // Check which msg_senders are in boundless_users table
        let boundless_users = if !msg_senders.is_empty() {
            let boundless_query = query(
                r#"
                SELECT user_address 
                FROM boundless_users 
                WHERE user_address = ANY($1::text[])
                "#,
            )
            .bind(&msg_senders)
            .fetch_all(&active_pool)
            .await?;

            boundless_query
                .iter()
                .filter_map(|row| row.try_get::<String, _>("user_address").ok())
                .collect::<std::collections::HashSet<String>>()
        } else {
            std::collections::HashSet::new()
        };

        for row in records {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;

            // We know the status is ProofRequested because we just set it.
            // Only populate fields returned by the query.
            let event = EventUpdate {
                tx_hash,
                status: EventStatus::ProofRequested, // Set status explicitly
                src_chain_id: row
                    .try_get::<Option<i32>, _>("src_chain_id")?
                    .map(|id| id as u32),
                dst_chain_id: row
                    .try_get::<Option<i32>, _>("dst_chain_id")?
                    .map(|id| id as u32),
                msg_sender: row
                    .try_get::<Option<String>, _>("msg_sender")?
                    .map(|addr| addr.parse().unwrap()),
                market: row
                    .try_get::<Option<String>, _>("market")?
                    .map(|addr| addr.parse().unwrap()),
                // Other fields are Default::default()
                ..Default::default()
            };

            // Check if the msg_sender is in boundless_users
            let is_boundless = event.msg_sender
                .as_ref()
                .map(|addr| boundless_users.contains(&addr.to_string()))
                .unwrap_or(false);

            if is_boundless {
                boundless_events.push(event);
            } else {
                bonsai_events.push(event);
            }
        }

        info!("Claimed {} events for proof generation. {} boundless, {} bonsai", number_events_claimed, boundless_events.len(), bonsai_events.len());

        Ok((bonsai_events, boundless_events))
    }

    /// Retrieves proven events for batch submission on a specific destination chain
    ///
    /// This method implements chain-specific batch processing that:
    /// 1. Checks delay requirements for the target chain
    /// 2. Finds the oldest journal for the target chain
    /// 3. Claims events atomically with journal ordering
    /// 4. Applies transaction limits per batch
    ///
    /// ## Chain-Specific Processing:
    ///
    /// Each destination chain has its own batch timing and limits,
    /// ensuring that batches are submitted at appropriate intervals
    /// and don't exceed chain-specific transaction limits.
    ///
    /// ## Journal Ordering:
    ///
    /// Events are processed in journal order to maintain the correct
    /// sequence for proof verification and batch inclusion.
    ///
    /// # Arguments
    /// * `delay_seconds` - Minimum delay between batch submissions for this chain
    /// * `target_dst_chain_id` - Destination chain ID to process events for
    /// * `max_tx` - Maximum number of transactions per batch
    ///
    /// # Returns
    /// * `Result<Vec<EventUpdate>>` - Vector of events ready for batch submission
    ///
    /// # Example
    /// ```rust
    /// let events = db.get_proven_events_for_chain(120, 59144, 50).await?;
    /// ```
    pub async fn get_proven_events_for_chain(
        &self,
        delay_seconds: i64,
        target_dst_chain_id: u32,
        max_tx: i64, // Added max_tx limit parameter
    ) -> Result<Vec<EventUpdate>> {
        let active_pool = self.get_active_pool().await;
        
        // 1. & 2. Get last submission time for the target chain and check delay
        let last_submission_time: Option<DateTime<Utc>> = query_scalar(
            "SELECT last_batch_submitted_at FROM chain_batch_sync WHERE dst_chain_id = $1",
        )
        .bind(target_dst_chain_id as i32)
        .fetch_optional(&active_pool)
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
        let oldest_journal_data: Option<(Vec<u8>,)> = query_as(
            r#"
                SELECT journal 
                FROM events 
                WHERE status = 'ProofReceived'::event_status 
                  AND dst_chain_id = $1 -- Filter by target chain
                  AND journal IS NOT NULL 
                ORDER BY received_at ASC 
                LIMIT 1
            "#,
        )
        .bind(target_dst_chain_id as i32)
        .fetch_optional(&active_pool)
        .await?;

        let oldest_journal = match oldest_journal_data {
            Some((journal,)) => Bytes::from(journal),
            None => {
                debug!(
                    "No ProofReceived events found with a journal for chain {}.",
                    target_dst_chain_id
                );
                return Ok(Vec::new()); // No events to process for this chain
            }
        };

        // 4. Start Transaction
        let mut tx = active_pool.begin().await?;

        // 5. Update Timestamp in chain_batch_sync for the target chain
        let now_utc = Utc::now();
        query(
            r#"
            INSERT INTO chain_batch_sync (dst_chain_id, last_batch_submitted_at) 
            VALUES ($1, $2) 
            ON CONFLICT (dst_chain_id) DO UPDATE 
            SET last_batch_submitted_at = EXCLUDED.last_batch_submitted_at
        "#,
        )
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

        info!(
            "Claimed {} (max {}) events for batch submission on chain {} (Journal: 0x{}...).",
            records.len(),
            max_tx,
            target_dst_chain_id,
            hex::encode(&oldest_journal[..std::cmp::min(8, oldest_journal.len())])
        );

        for row in records {
            let tx_hash_str: String = row.try_get("tx_hash")?;
            let tx_hash = TxHash::from_str(&tx_hash_str)?;

            // Safely parse potentially NULL fields
            let msg_sender = row
                .try_get::<Option<String>, _>("msg_sender")?
                .and_then(|s| s.parse::<Address>().ok());
            let market = row
                .try_get::<Option<String>, _>("market")?
                .and_then(|s| s.parse::<Address>().ok());
            let amount: Option<U256> = row
                .try_get("amount")
                .ok()
                .and_then(|s| U256::from_str(s).ok());

            events.push(EventUpdate {
                tx_hash,
                status: EventStatus::BatchSubmitted, // Status is known
                journal_index: row.try_get("journal_index")?, // Should exist
                dst_chain_id: Some(target_dst_chain_id), // Known input
                journal: row
                    .try_get::<Option<Vec<u8>>, _>("journal")?
                    .map(Bytes::from),
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

    /// Updates events to ready status based on current block numbers
    ///
    /// This method updates events from `ReorgSecurityDelay` status to
    /// `ReadyToRequestProof` status when the current block number meets
    /// the required proof request block threshold.
    ///
    /// ## Block-Based Updates:
    ///
    /// The method processes events chain by chain, updating only events
    /// where the current block number has reached or exceeded the
    /// `should_request_proof_at_block` threshold.
    ///
    /// # Arguments
    /// * `current_block_map` - HashMap of chain ID to current block number
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let block_map = HashMap::from([(59144, 12345), (59140, 67890)]);
    /// db.update_events_to_ready_status(&block_map).await?;
    /// ```
    pub async fn update_events_to_ready_status(
        &self,
        current_block_map: &HashMap<u64, i32>,
    ) -> Result<()> {
        if current_block_map.is_empty() {
            info!("No current block numbers provided, skipping update.");
            return Ok(());
        }

        // Convert HashMap to vectors for binding
        let chain_ids: Vec<i64> = current_block_map.keys().map(|&k| k as i64).collect();
        let block_numbers: Vec<i32> = current_block_map.values().cloned().collect();

        let active_pool = self.get_active_pool().await;
        
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
        .execute(&active_pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!(
                "Updated {} events to ReadyToRequestProof status based on current block numbers.",
                rows_affected
            );
        } else {
            debug!("No events updated to ReadyToRequestProof status in this cycle.");
        }

        Ok(())
    }

    /// Updates events with proof data and journal indices
    ///
    /// This method updates events that have been claimed for proof generation
    /// with the received proof data, including journal, seal, and timing
    /// information. It also assigns journal indices for proper ordering.
    ///
    /// ## Proof Data Management:
    ///
    /// The method updates multiple events with the same proof data (journal
    /// and seal) while assigning unique journal indices to maintain proper
    /// ordering for batch processing.
    ///
    /// ## Performance Tracking:
    ///
    /// The method records timing information (stark_time, snark_time) and
    /// cycle counts for performance monitoring and optimization.
    ///
    /// # Arguments
    /// * `updates` - Vector of (tx_hash, journal_index) pairs
    /// * `journal` - Journal data for proof verification
    /// * `seal` - Seal data for proof verification
    /// * `uuid` - Unique identifier for the proof generation run
    /// * `stark_time` - Stark proof generation time in seconds
    /// * `snark_time` - Snark proof generation time in seconds
    /// * `total_cycles` - Total cycles used for proof generation
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let updates = vec![(tx_hash1, 0), (tx_hash2, 1)];
    /// db.set_events_proof_received_with_index(
    ///     &updates, &journal, &seal, "uuid-123", 10, 5, 1000
    /// ).await?;
    /// ```
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

        let active_pool = self.get_active_pool().await;
        
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
        .execute(&active_pool)
        .await?
        .rows_affected();

        if rows_affected == count as u64 {
            info!(
                "Successfully updated {} events with proof data and journal indices.",
                count
            );
        } else {
            warn!("Attempted to update {} events with proof data, but {} rows were affected. Some events might not have been in 'ProofRequested' state.", count, rows_affected);
        }

        Ok(())
    }

    /// Updates batch status for a group of events
    ///
    /// This method updates the batch status for multiple events that were
    /// submitted as part of the same batch transaction. It handles both
    /// successful batch inclusion and failed batch submissions.
    ///
    /// ## Batch Status Management:
    ///
    /// The method updates events atomically within a transaction to ensure
    /// consistency, setting the appropriate status and optional batch
    /// transaction hash and inclusion timestamp.
    ///
    /// ## Error Handling:
    ///
    /// For failed batches, the method can set an error message to provide
    /// context about why the batch failed.
    ///
    /// # Arguments
    /// * `tx_hashes` - Vector of transaction hashes to update
    /// * `batch_tx_hash` - Optional batch transaction hash
    /// * `batch_included_at` - Optional timestamp when batch was included
    /// * `error` - Optional error message for failed batches
    /// * `status` - New status for the events (BatchIncluded, BatchFailed, etc.)
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// let tx_hashes = vec![tx_hash1, tx_hash2];
    /// db.update_batch_status(
    ///     &tx_hashes, Some("0x123..."), Some(now), None, EventStatus::BatchIncluded
    /// ).await?;
    /// ```
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

        let active_pool = self.get_active_pool().await;
        
        // Start a transaction
        let mut tx = active_pool.begin().await?;

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
            "#,
        )
        .bind(&status_str) // Use reference here
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
            info!(
                "Successfully updated batch status for {} events to {}",
                count, status_str
            );
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

        let active_pool = self.get_active_pool().await;
        
        // Start a transaction
        let mut tx = active_pool.begin().await?;

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
                info!(
                    "Successfully migrated {} retarded events to finished_events with status {}",
                    count, status_str
                );
            } else {
                info!(
                    "Successfully migrated {} events to finished_events with status {}",
                    count, status_str
                );
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
        let active_pool = self.get_active_pool().await;
        let mut tx = active_pool.begin().await?;

        // Convert market prices to a format usable in SQL
        let market_price_pairs: Vec<(String, f64)> = market_prices
            .iter()
            .map(|(addr, price)| (addr.to_string(), *price))
            .collect();

        // Convert chain params to a format usable in SQL
        let chain_param_pairs: Vec<(i32, i32, i64, i32, i32)> = chain_params
            .iter()
            .map(|(chain_id, params)| {
                (
                    *chain_id as i32,
                    params.max_volume,
                    params.time_interval.as_secs() as i64,
                    params.block_delay as i32,
                    params.reorg_protection as i32,
                )
            })
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

    pub async fn reset_stuck_events(
        &self,
        finished_sample_size: i64,
        multiplier: f64,
        batch_limit: i64,
        max_retries: i64,
    ) -> Result<()> {
        let active_pool = self.get_active_pool().await;
        let mut tx = active_pool.begin().await?;

        // Check if we have enough historical data first
        let has_sufficient_data = query_scalar::<_, bool>(
            r#"
            SELECT COUNT(*) >= $1
            FROM finished_events
            WHERE status = 'TxProcessSuccess'::event_status
            "#,
        )
        .bind(finished_sample_size)
        .fetch_one(&mut *tx)
        .await?;

        if !has_sufficient_data {
            info!("Insufficient historical data (less than {} successful events) to calculate timeouts. No events will be reset this cycle.", finished_sample_size);
            return Ok(());
        }

        // Calculate separate timeouts for boundless and bonsai proofs
        let timeouts = query(
            r#"
            WITH boundless_successes AS (
                SELECT 
                    GREATEST(AVG(EXTRACT(EPOCH FROM (proof_received_at - proof_requested_at))), 60) as avg_proof_request_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (batch_submitted_at - proof_received_at))), 10) as avg_proof_generation_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (batch_included_at - batch_submitted_at))), 10) as avg_batch_submission_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (tx_finished_at - batch_included_at))), 20) as avg_batch_inclusion_time
                FROM finished_events fe
                JOIN boundless_users bu ON fe.msg_sender = bu.user_address
                WHERE status = 'TxProcessSuccess'::event_status
                LIMIT $1
            ),
            bonsai_successes AS (
                SELECT 
                    GREATEST(AVG(EXTRACT(EPOCH FROM (proof_received_at - proof_requested_at))), 20) as avg_proof_request_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (batch_submitted_at - proof_received_at))), 10) as avg_proof_generation_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (batch_included_at - batch_submitted_at))), 10) as avg_batch_submission_time,
                    GREATEST(AVG(EXTRACT(EPOCH FROM (tx_finished_at - batch_included_at))), 20) as avg_batch_inclusion_time
                FROM finished_events fe
                LEFT JOIN boundless_users bu ON fe.msg_sender = bu.user_address
                WHERE status = 'TxProcessSuccess'::event_status
                  AND bu.user_address IS NULL
                LIMIT $1
            )
            SELECT 
                -- Use boundless timeouts (longer) as default, fallback to bonsai if no boundless data
                COALESCE(
                    (SELECT CEIL(avg_proof_request_time * $2)::INT8 FROM boundless_successes),
                    (SELECT CEIL(avg_proof_request_time * $2)::INT8 FROM bonsai_successes),
                    120  -- Default fallback: 2 minutes
                ) as boundless_proof_request_timeout,
                COALESCE(
                    (SELECT CEIL(avg_proof_request_time * $2)::INT8 FROM bonsai_successes),
                    (SELECT CEIL(avg_proof_request_time * $2)::INT8 FROM boundless_successes),
                    60   -- Default fallback: 1 minute
                ) as bonsai_proof_request_timeout,
                -- Use the same timeouts for other stages (they don't differ by proof method)
                COALESCE(
                    (SELECT CEIL(avg_proof_generation_time * $2)::INT8 FROM boundless_successes),
                    (SELECT CEIL(avg_proof_generation_time * $2)::INT8 FROM bonsai_successes),
                    20   -- Default fallback: 20 seconds
                ) as proof_generation_timeout,
                COALESCE(
                    (SELECT CEIL(avg_batch_submission_time * $2)::INT8 FROM boundless_successes),
                    (SELECT CEIL(avg_batch_submission_time * $2)::INT8 FROM bonsai_successes),
                    20   -- Default fallback: 20 seconds
                ) as batch_submission_timeout,
                COALESCE(
                    (SELECT CEIL(avg_batch_inclusion_time * $2)::INT8 FROM boundless_successes),
                    (SELECT CEIL(avg_batch_inclusion_time * $2)::INT8 FROM bonsai_successes),
                    40   -- Default fallback: 40 seconds
                ) as batch_inclusion_timeout
            "#
        )
        .bind(finished_sample_size)
        .bind(multiplier)
        .fetch_one(&mut *tx)
        .await?;

        // Extract timeout values with proper type conversion
        let boundless_proof_request_timeout: i64 = timeouts.try_get("boundless_proof_request_timeout")?;
        let bonsai_proof_request_timeout: i64 = timeouts.try_get("bonsai_proof_request_timeout")?;
        let proof_generation_timeout: i64 = timeouts.try_get("proof_generation_timeout")?;
        let batch_submission_timeout: i64 = timeouts.try_get("batch_submission_timeout")?;
        let batch_inclusion_timeout: i64 = timeouts.try_get("batch_inclusion_timeout")?;

        // Combined stuck events query with batch limit and proof method-specific timeouts
        let rows_affected = query(
            r#"
            WITH stuck_events AS (
                SELECT 
                    e.tx_hash,
                    e.status,
                    e.resubmitted,
                    CASE 
                        WHEN e.status = 'ProofRequested'::event_status AND 
                             e.proof_requested_at < NOW() - (
                                 CASE 
                                     WHEN bu.user_address IS NOT NULL THEN $1::bigint  -- boundless timeout
                                     ELSE $2::bigint  -- bonsai timeout
                                 END || ' seconds'
                             )::interval THEN 'proof_requested'
                        WHEN e.status = 'ProofReceived'::event_status AND e.proof_received_at < NOW() - ($3::bigint || ' seconds')::interval THEN 'proof_received'
                        WHEN e.status = 'BatchSubmitted'::event_status AND e.batch_submitted_at < NOW() - ($4::bigint || ' seconds')::interval THEN 'batch_submitted'
                        WHEN e.status = 'BatchFailed'::event_status AND e.batch_submitted_at < NOW() - ($5::bigint || ' seconds')::interval THEN 'batch_failed'
                        ELSE NULL
                    END as stuck_type
                FROM events e
                LEFT JOIN boundless_users bu ON e.msg_sender = bu.user_address
                WHERE e.status IN ('ProofRequested', 'ProofReceived', 'BatchSubmitted', 'BatchFailed')
                  AND (e.resubmitted IS NULL OR e.resubmitted < $7)
                LIMIT $6
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
        .bind(boundless_proof_request_timeout)  // $1 - boundless timeout
        .bind(bonsai_proof_request_timeout)     // $2 - bonsai timeout
        .bind(proof_generation_timeout)         // $3 - proof generation timeout
        .bind(batch_submission_timeout)         // $4 - batch submission timeout
        .bind(batch_inclusion_timeout)          // $5 - batch inclusion timeout
        .bind(batch_limit)                      // $6 - batch limit
        .bind(max_retries)                      // $7 - max retries
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
            "#,
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
        let active_pool = self.get_active_pool().await;
        
        let rows_affected = query(
            r#"
            UPDATE events
            SET 
                status = 'ReadyToRequestProof'::event_status,
                proof_requested_at = NULL
            WHERE status = 'ProofRequested'::event_status
            "#,
        )
        .execute(&active_pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!(
                "Reset {} events from ProofRequested back to ReadyToRequestProof",
                rows_affected
            );
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
        reason: &str,
    ) -> Result<()> {
        let active_pool = self.get_active_pool().await;
        
        query(
            r#"
            INSERT INTO node_status (chain_id, primary_url, fallback_url, reason)
            VALUES ($1, $2, $3, $4)
            "#,
        )
        .bind(chain_id as i32)
        .bind(primary_url)
        .bind(fallback_url)
        .bind(reason)
        .execute(&active_pool)
        .await?;

        debug!("Recorded node failure for chain {}: {}", chain_id, reason);
        Ok(())
    }

    pub async fn get_failed_tx(
        &self,
        max_retries: i64,
    ) -> Result<Vec<(u32, Address, U256, Vec<TxHash>)>> {
        use alloy::primitives::{Address, TxHash, U256};
        use sqlx::Row;
        use std::str::FromStr;

        let active_pool = self.get_active_pool().await;
        
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
            "#,
        )
        .bind(max_retries)
        .fetch_all(&active_pool)
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
                Some(vec) => vec
                    .into_iter()
                    .filter_map(|h| TxHash::from_str(&h).ok())
                    .collect(),
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
        let active_pool = self.get_active_pool().await;
        let mut tx = active_pool.begin().await?;
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

    /// Adds a boundless user to the database
    ///
    /// This method adds a new Ethereum address to the boundless_users table.
    /// If the address already exists, it will be ignored (ON CONFLICT DO NOTHING).
    ///
    /// ## Boundless Users:
    ///
    /// Boundless users are special addresses that receive different proof generation
    /// treatment compared to regular users. They may use different proof generation
    /// services or have different timeout configurations.
    ///
    /// # Arguments
    /// * `address` - Ethereum address to add (should be validated before calling)
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// db.add_boundless_user("0x0000000000000000000000000000000000000000").await?;
    /// ```
    pub async fn add_boundless_user(&self, address: &str) -> Result<()> {
        let active_pool = self.get_active_pool().await;
        
        let rows_affected = query(
            r#"
            INSERT INTO boundless_users (user_address) 
            VALUES ($1)
            ON CONFLICT (user_address) DO NOTHING
            "#,
        )
        .bind(address)
        .execute(&active_pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!("Added boundless user: {}", address);
        } else {
            debug!("Boundless user already exists: {}", address);
        }

        Ok(())
    }

    /// Removes a boundless user from the database
    ///
    /// This method removes an Ethereum address from the boundless_users table.
    /// If the address doesn't exist, no error is returned.
    ///
    /// ## Boundless Users:
    ///
    /// Boundless users are special addresses that receive different proof generation
    /// treatment compared to regular users. Removing them will cause them to be
    /// treated as regular users in future operations.
    ///
    /// # Arguments
    /// * `address` - Ethereum address to remove (should be validated before calling)
    ///
    /// # Returns
    /// * `Result<()>` - Success or error status
    ///
    /// # Example
    /// ```rust
    /// db.remove_boundless_user("0x0000000000000000000000000000000000000000").await?;
    /// ```
    pub async fn remove_boundless_user(&self, address: &str) -> Result<()> {
        let active_pool = self.get_active_pool().await;
        
        let rows_affected = query(
            r#"
            DELETE FROM boundless_users 
            WHERE user_address = $1
            "#,
        )
        .bind(address)
        .execute(&active_pool)
        .await?
        .rows_affected();

        if rows_affected > 0 {
            info!("Removed boundless user: {}", address);
        } else {
            debug!("Boundless user not found: {}", address);
        }

        Ok(())
    }


    /// Checks if a user is boundless
    ///
    /// This method checks if an Ethereum address exists in the boundless_users table.
    /// Returns true if the user is boundless, false otherwise.
    ///
    /// ## Boundless Users:
    ///
    /// Boundless users are special addresses that receive different proof generation
    /// treatment compared to regular users. This method provides a way to check if
    /// a specific address is configured as boundless.
    ///
    /// # Arguments
    /// * `address` - Ethereum address to check (should be validated before calling)
    ///
    /// # Returns
    /// * `Result<bool>` - True if user is boundless, false otherwise
    ///
    /// # Example
    /// ```rust
    /// let is_boundless = db.is_boundless_user("0x0000000000000000000000000000000000000000").await?;
    /// println!("User is boundless: {}", is_boundless);
    /// ```
    pub async fn is_boundless_user(&self, address: &str) -> Result<bool> {
        let active_pool = self.get_active_pool().await;
        
        let exists: Option<i32> = query_scalar(
            r#"
            SELECT 1 FROM boundless_users 
            WHERE user_address = $1
            "#,
        )
        .bind(address)
        .fetch_optional(&active_pool)
        .await?;

        Ok(exists.is_some())
    }

}
