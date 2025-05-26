use alloy::primitives::{address, Address};
use std::time::Duration;


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

pub const mUSDC_market: Address = address!("269C36A173D881720544Fb303E681370158FF1FD");
pub const mWETH_market: Address = address!("C7Bc6bD45Eb84D594f51cED3c5497E6812C7732f");
pub const mUSDT_market: Address = address!("DF0635c1eCfdF08146150691a97e2Ff6a8Aa1a90");
pub const mWBTC_market: Address = address!("cb4d153604a6F21Ff7625e5044E89C3b903599Bc");
pub const mwstETH_market: Address = address!("1D8e8cEFEb085f3211Ab6a443Ad9051b54D1cd1a");
pub const mezETH_market: Address = address!("0B3c6645F4F2442AD4bbee2e2273A250461cA6f8");
pub const mweETH_market: Address = address!("8BaD0c523516262a439197736fFf982F5E0987cC");
pub const mwrsETH_market: Address = address!("4DF3DD62DB219C47F6a7CB1bE02C511AFceAdf5E");
