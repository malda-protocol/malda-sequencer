use std::time::Duration;
use alloy::primitives::{Address, address};

// Channel capacities
pub const EVENT_CHANNEL_CAPACITY: usize = 1000;
pub const PROCESSED_CHANNEL_CAPACITY: usize = 1000;
pub const PROOF_CHANNEL_CAPACITY: usize = 1000;

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
    // Chain IDs
    LINEA_SEPOLIA_CHAIN_ID,
    OPTIMISM_SEPOLIA_CHAIN_ID,
    ETHEREUM_SEPOLIA_CHAIN_ID,
    
    // Markets (non-chain specific)
    USDC_MARKET_SEPOLIA,
    WETH_MARKET_SEPOLIA,
};

// WebSocket URLs
pub const WS_URL_ETH_SEPOLIA: &str = "wss://eth-sepolia.g.alchemy.com/v2/uGenJq8d9bfW9gXcaUZln_ZBDhS61oJY";
pub const WS_URL_OPT_SEPOLIA: &str = "wss://opt-sepolia.g.alchemy.com/v2/uGenJq8d9bfW9gXcaUZln_ZBDhS61oJY";
pub const WS_URL_LINEA_SEPOLIA: &str = "wss://linea-sepolia.g.alchemy.com/v2/uGenJq8d9bfW9gXcaUZln_ZBDhS61oJY";

// Sequencer configuration
pub const SEQUENCER_ADDRESS: Address = address!("2693946791da99dA78Ac441abA6D5Ce2Bccd96D3");
pub const SEQUENCER_PRIVATE_KEY: &str = "0xbc4e6261e470a5f67ec85062c0901cb87a1c9286d1f37712ca1d16a56a81a1bf";

// Timing configurations
pub const LISTENER_SPAWN_DELAY: Duration = Duration::from_millis(100);

#[cfg(test)]
pub mod test {
    use super::*;
    
    pub const TEST_WS_URL: &str = "wss://example.com";
    pub const TEST_CHAIN_ID: u64 = 1;
    pub const TEST_EVENT_SIGNATURE: &str = "Event()";
} 