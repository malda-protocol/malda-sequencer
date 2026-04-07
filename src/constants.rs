//! # Constants Module
//!
//! This module defines global constants used throughout the sequencer system.
//! It includes market contract addresses for both mainnet and testnet environments.
//! These constants are used for contract interaction and market identification
//! across all components.
//!
//! ## Key Features:
//! - **Mainnet Market Addresses**: Production market contract addresses
//! - **Testnet Market Addresses**: Mock market addresses for testing
//! - **Market Identification**: Standardized addresses for different asset types
//! - **Environment Support**: Separate constants for mainnet and testnet
//!
//! ## Architecture:
//! ```
//! Constants
//! ├── Mainnet Markets: M_USDC_MARKET, M_WETH_MARKET, M_USDT_MARKET, ...
//! ├── Testnet Markets: M_USDC_MOCK_MARKET, MWST_ETH_MOCK_MARKET
//! └── Market Types: USDC, WETH, USDT, WBTC, wstETH, ezETH, weETH, wrsETH
//! ```
//!
//! ## Market Addresses:
//!
//! ### Mainnet Markets:
//! - **USDC**: `0x1eEa258B505cd6381171c1075EC6934F8D0Faf3b`
//! - **WETH**: `0x6AECeD8e67964Eb6d0Ae7B159D27eF07F6c11b99`
//! - **USDT**: `0x66DfCBf23319D68bdF0cB57797Fcc0A64d2265f8`
//! - **WBTC**: `0x0E5ad58f827f53C9F92c71319b77772F2a1FBdb2`
//! - **wstETH**: `0xe79a5f1E2E5619dF1cbb089Db3B11ff9E4dA5aff`
//! - **ezETH**: `0x867B44af79da71684508c25a1323db3cce5bC23D`

//! - **weETH**: `0x8BaD0c523516262a439197736fFf982F5E0987cC`
//! - **wrsETH**: `0x4DF3DD62DB219C47F6a7CB1bE02C511AFceAdf5E`
//!
//! ### Testnet Markets:
//! - **Mock USDC**: `0xc6e1FB449b08B26B2063c289DF9BBcb79B91c99`
//! - **Mock wstETH**: `0x0d7Ee0ee6E449e38269F2E089262b40cA4096594`

use alloy::primitives::{address, Address};

// Mainnet market contract addresses
/// USDC market contract address
pub const M_USDC_MARKET: Address = address!("1eEa258B505cd6381171c1075EC6934F8D0Faf3b");
/// WETH market contract address
pub const M_WETH_MARKET: Address = address!("6AECeD8e67964Eb6d0Ae7B159D27eF07F6c11b99");
/// USDT market contract address
pub const M_USDT_MARKET: Address = address!("66DfCBf23319D68bdF0cB57797Fcc0A64d2265f8");
/// WBTC market contract address
pub const M_WBTC_MARKET: Address = address!("0E5ad58f827f53C9F92c71319b77772F2a1FBdb2");
/// wstETH market contract address
pub const MWST_ETH_MARKET: Address = address!("e79a5f1E2E5619dF1cbb089Db3B11ff9E4dA5aff");
/// ezETH market contract address
pub const MEZ_ETH_MARKET: Address = address!("867B44af79da71684508c25a1323db3cce5bC23D");
/// weETH market contract address
pub const MWE_ETH_MARKET: Address = address!("8BaD0c523516262a439197736fFf982F5E0987cC");
/// wrsETH market contract address
pub const MWRS_ETH_MARKET: Address = address!("4DF3DD62DB219C47F6a7CB1bE02C511AFceAdf5E");

// TESTNET MOCK MARKETS
/// Mock USDC market contract address (testnet)
pub const M_USDC_MOCK_MARKET: Address = address!("0x76daf584Cbf152c85EB2c7Fe7a3d50DaF3f5B6e6");
/// Mock wstETH market contract address (testnet)
pub const MWST_ETH_MOCK_MARKET: Address = address!("0xD4286cc562b906589f8232335413f79d9aD42f7E");
