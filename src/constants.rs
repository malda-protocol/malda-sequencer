use alloy::primitives::{address, Address};
use std::time::Duration;

// Channel capacities
pub const EVENT_CHANNEL_CAPACITY: usize = 1000;
pub const PROCESSED_CHANNEL_CAPACITY: usize = 1000;
pub const PROOF_CHANNEL_CAPACITY: usize = 1000;

// Batch configurations
pub const BATCH_SIZE: usize = 50; // Number of events to process in each batch

// Retry configurations
pub const MAX_PROOF_RETRIES: u32 = 6;
pub const PROOF_RETRY_DELAY: Duration = Duration::from_secs(1);

pub const MAX_TX_RETRIES: u32 = 3;
pub const TX_RETRY_DELAY: Duration = Duration::from_secs(1);

// Gas configurations
pub const GAS_MULTIPLIER: f64 = 1.1;
pub const PRIORITY_FEE_MULTIPLIER: f64 = 1.2;

// Import necessary constants from malda_rs
pub use malda_rs::constants::{
    ETHEREUM_CHAIN_ID,

    // Chain IDs
    LINEA_CHAIN_ID,
    OPTIMISM_CHAIN_ID,
    BASE_CHAIN_ID,
    // Markets (non-chain specific)

};

// WebSocket URLs
// pub const WS_URL_ETH_SEPOLIA: &str =
//     "ws://localhost:8551";
// pub const WS_URL_OPT_SEPOLIA: &str =
//     "ws://localhost:8552";
// pub const WS_URL_BASE_SEPOLIA: &str =
//     "ws://localhost:8553";
// pub const WS_URL_LINEA_SEPOLIA: &str =
//     "ws://localhost:8554";

pub const WS_URL_LINEA: &str =
    "ws://localhost:8554";
pub const WS_URL_OPT: &str =
    "ws://localhost:8556";
pub const WS_URL_ETH: &str =
    "ws://localhost:8555";
// pub const WS_URL_BASE: &str =
//     "ws://localhost:8557";
pub const WS_URL_BASE: &str =
    "wss://base-mainnet.g.alchemy.com/v2/O-kuLtbrAk5Y-a8tmDmqjj4LIHFXz5xi";

pub const WS_URL_LINEA_BACKUP: &str =
    "wss://linea-mainnet.g.alchemy.com/v2/O-kuLtbrAk5Y-a8tmDmqjj4LIHFXz5xi";
pub const WS_URL_OPT_BACKUP: &str =
    "wss://opt-mainnet.g.alchemy.com/v2/O-kuLtbrAk5Y-a8tmDmqjj4LIHFXz5xi";
pub const WS_URL_ETH_BACKUP: &str =
    "wss://eth-mainnet.g.alchemy.com/v2/O-kuLtbrAk5Y-a8tmDmqjj4LIHFXz5xi";
pub const WS_URL_BASE_BACKUP: &str =
    "wss://base-mainnet.g.alchemy.com/v2/O-kuLtbrAk5Y-a8tmDmqjj4LIHFXz5xi";

// pub const RPC_URL_LINEA_BACKUP: &str =
//     "https://linea-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi";
// pub const RPC_URL_OPT_BACKUP: &str =
//     "https://opt-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi";
// pub const RPC_URL_ETH_BACKUP: &str =
//     "https://eth-mainnet.g.alchemy.com/v2/xaLwh_pW3EIPXyTzwLQ-hIRiwtQO-egi";

// Sequencer configuration
pub const SEQUENCER_ADDRESS: Address = address!("5641B4889177419E8f79de939967E9277C127cDe");
pub const SEQUENCER_PRIVATE_KEY: &str =
    "0x3efed4b9817c166fc2108d429d3e317d2c6f530e9327bd8f0b66e8171f115275";

// Timing configurations
pub const LISTENER_SPAWN_DELAY: Duration = Duration::from_millis(100);
pub const ETHEREUM_BLOCK_DELAY: u64 = 12;

// Add this with other constants
pub const PROOF_REQUEST_DELAY: u64 = 15;

// pub const BATCH_SUBMITTER_OLD: Address = address!("1b2ac3856D953110DB451Db177e5D4EEcdBA6BA8");
pub const BATCH_SUBMITTER: Address = address!("04f0cDc5a215dEdf6A1Ed5444E07367e20768041");

/// The time window to wait for additional events to batch together (in seconds)
pub const BATCH_WINDOW: u64 = 2;

#[cfg(test)]
pub mod test {
    pub const TEST_WS_URL: &str = "wss://example.com";
    pub const TEST_CHAIN_ID: u64 = 1;
    pub const TEST_EVENT_SIGNATURE: &str = "Event()";
}
