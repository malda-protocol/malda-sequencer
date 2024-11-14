//! Constants module containing RPC URLs, contract addresses, and other network-specific constants.
//!
//! This module provides centralized access to various network-specific constants, including:
//! - RPC endpoint URLs for different blockchain networks
//! - Sequencer request URLs for L2 networks
//! - WETH contract addresses across supported chains
//! - Constants used throughout the project for chain IDs, addresses, and cryptographic values.
//!
//! This module contains a comprehensive set of constant definitions that are used across different chains
//! and components of the Malda Protocol.

use alloy_primitives::{address, Address};
pub use guest_utils::constants::*;

/// RPC endpoint URLs for supported networks
pub const RPC_URL_LINEA: &str =
    "https://linea-mainnet.g.alchemy.com/v2/fSI-SMz_VGgi1ZwahhztYMCV51uTaN9e";
pub const RPC_URL_SCROLL: &str =
    "https://scroll-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";
pub const RPC_URL_ETHEREUM: &str =
    "https://eth-mainnet.g.alchemy.com/v2/scFv-881VOeTp7qHT88HEZ_EmsJqrGQ0";
pub const RPC_URL_BASE: &str = "https://mainnet.base.org";
pub const RPC_URL_OPTIMISM: &str =
    "https://opt-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";
pub const RPC_URL_ARBITRUM: &str =
    "https://arb-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";

/// Sequencer request URLs for Layer 2 networks
pub const SEQUENCER_REQUEST_OPTIMISM: &str = "https://optimism.operationsolarstorm.org/latest";
pub const SEQUENCER_REQUEST_BASE: &str = "https://base.operationsolarstorm.org/latest";

/// Wrapped Ether (WETH) contract addresses for each supported network
pub const WETH_ETHEREUM: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
pub const WETH_LINEA: Address = address!("e5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f");
pub const WETH_ARBITRUM: Address = address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1");
pub const WETH_OPTIMISM: Address = address!("4200000000000000000000000000000000000006");
pub const WETH_BASE: Address = address!("4200000000000000000000000000000000000006");
pub const WETH_SCROLL: Address = address!("5300000000000000000000000000000000000004");
