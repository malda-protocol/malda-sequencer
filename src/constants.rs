use alloy::primitives::{address, Address};
use std::time::Duration;

// Channel capacities
pub const EVENT_CHANNEL_CAPACITY: usize = 1000;
pub const PROCESSED_CHANNEL_CAPACITY: usize = 1000;
pub const PROOF_CHANNEL_CAPACITY: usize = 1000;

// Batch configurations
pub const BATCH_SIZE: usize = 10; // Number of events to process in each batch

// Retry configurations
pub const MAX_PROOF_RETRIES: u32 = 3;
pub const PROOF_RETRY_DELAY: Duration = Duration::from_secs(1);

pub const MAX_TX_RETRIES: u32 = 3;
pub const TX_RETRY_DELAY: Duration = Duration::from_secs(1);
pub const TX_TIMEOUT: Duration = Duration::from_secs(30);

// Gas configurations
pub const GAS_MULTIPLIER: f64 = 1.1;
pub const PRIORITY_FEE_MULTIPLIER: f64 = 1.2;

// Import necessary constants from malda_rs
pub use malda_rs::constants::{
    ETHEREUM_SEPOLIA_CHAIN_ID,

    // Chain IDs
    LINEA_SEPOLIA_CHAIN_ID,
    OPTIMISM_SEPOLIA_CHAIN_ID,
    // Markets (non-chain specific)
    USDC_MOCK_MARKET_SEPOLIA,
    WSTETH_MOCK_MARKET_SEPOLIA,
};

// WebSocket URLs
pub const WS_URL_ETH_SEPOLIA: &str =
    "ws://localhost:8546";
pub const WS_URL_OPT_SEPOLIA: &str =
    "ws://localhost:8548";
pub const WS_URL_LINEA_SEPOLIA: &str =
    "ws://localhost:8556";

// Sequencer configuration
pub const SEQUENCER_ADDRESS: Address = address!("2693946791da99dA78Ac441abA6D5Ce2Bccd96D3");
pub const SEQUENCER_PRIVATE_KEY: &str =
    "0xbc4e6261e470a5f67ec85062c0901cb87a1c9286d1f37712ca1d16a56a81a1bf";

// Timing configurations
pub const LISTENER_SPAWN_DELAY: Duration = Duration::from_millis(100);
pub const ETHEREUM_BLOCK_DELAY: u64 = 12;

// Add this with other constants
pub const PROOF_REQUEST_DELAY: u64 = 15;

pub const BATCH_SUBMITTER: Address = address!("1b2ac3856D953110DB451Db177e5D4EEcdBA6BA8");

/// The time window to wait for additional events to batch together (in seconds)
pub const BATCH_WINDOW: u64 = 2;

#[cfg(test)]
pub mod test {
    pub const TEST_WS_URL: &str = "wss://example.com";
    pub const TEST_CHAIN_ID: u64 = 1;
    pub const TEST_EVENT_SIGNATURE: &str = "Event()";
}
