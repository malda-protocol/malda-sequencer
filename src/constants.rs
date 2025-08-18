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
//! - **USDC**: `0x269C36A173D881720544Fb303E681370158FF1FD`
//! - **WETH**: `0xC7Bc6bD45Eb84D594f51cED3c5497E6812C7732f`
//! - **USDT**: `0xDF0635c1eCfdF08146150691a97e2Ff6a8Aa1a90`
//! - **WBTC**: `0xcb4d153604a6F21Ff7625e5044E89C3b903599Bc`
//! - **wstETH**: `0x1D8e8cEFEb085f3211Ab6a443Ad9051b54D1cd1a`
//! - **ezETH**: `0x0B3c6645F4F2442AD4bbee2e2273A250461cA6f8`
//! - **weETH**: `0x8BaD0c523516262a439197736fFf982F5E0987cC`
//! - **wrsETH**: `0x4DF3DD62DB219C47F6a7CB1bE02C511AFceAdf5E`
//!
//! ### Testnet Markets:
//! - **Mock USDC**: `0x76daf584Cbf152c85EB2c7Fe7a3d50DaF3f5B6e6`
//! - **Mock wstETH**: `0xD4286cc562b906589f8232335413f79d9aD42f7E`

use alloy::primitives::{address, Address};

// Mainnet market contract addresses
/// USDC market contract address
pub const M_USDC_MARKET: Address = address!("269C36A173D881720544Fb303E681370158FF1FD");
/// WETH market contract address
pub const M_WETH_MARKET: Address = address!("C7Bc6bD45Eb84D594f51cED3c5497E6812C7732f");
/// USDT market contract address
pub const M_USDT_MARKET: Address = address!("DF0635c1eCfdF08146150691a97e2Ff6a8Aa1a90");
/// WBTC market contract address
pub const M_WBTC_MARKET: Address = address!("cb4d153604a6F21Ff7625e5044E89C3b903599Bc");
/// wstETH market contract address
pub const MWST_ETH_MARKET: Address = address!("1D8e8cEFEb085f3211Ab6a443Ad9051b54D1cd1a");
/// ezETH market contract address
pub const MEZ_ETH_MARKET: Address = address!("0B3c6645F4F2442AD4bbee2e2273A250461cA6f8");
/// weETH market contract address
pub const MWE_ETH_MARKET: Address = address!("8BaD0c523516262a439197736fFf982F5E0987cC");
/// wrsETH market contract address
pub const MWRS_ETH_MARKET: Address = address!("4DF3DD62DB219C47F6a7CB1bE02C511AFceAdf5E");

// TESTNET MOCK MARKETS
/// Mock USDC market contract address (testnet)
pub const M_USDC_MOCK_MARKET: Address = address!("0x76daf584Cbf152c85EB2c7Fe7a3d50DaF3f5B6e6");
/// Mock wstETH market contract address (testnet)
pub const MWST_ETH_MOCK_MARKET: Address = address!("0xD4286cc562b906589f8232335413f79d9aD42f7E");
