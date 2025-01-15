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

#[path = "../../methods/guest/guest_utils/src/constants.rs"]
mod constants;

use alloy_primitives::{address, Address};

pub use constants::*;

/// RPC endpoint URLs for supported networks
pub const RPC_URL_LINEA: &str =
    "https://linea-mainnet.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_SCROLL: &str =
    "https://scroll-mainnet.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_ETHEREUM: &str =
    "https://eth-mainnet.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_BASE: &str = "https://base.gateway.tenderly.co/29P4JAEzmz8Jkghs5Vdp72";
pub const RPC_URL_OPTIMISM: &str = "https://optimism.gateway.tenderly.co/1rDcbzMPbj4dIOGbO8uXKL";
pub const RPC_URL_ARBITRUM: &str =
    "https://arb-mainnet.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";

pub const RPC_URL_LINEA_SEPOLIA: &str =
    "https://linea-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_SCROLL_SEPOLIA: &str =
    "https://scroll-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_ETHEREUM_SEPOLIA: &str =
    "https://eth-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_BASE_SEPOLIA: &str =
    "https://base-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_OPTIMISM_SEPOLIA: &str =
    "https://opt-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_ARBITRUM_SEPOLIA: &str =
    "https://arb-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";

/// Sequencer request URLs for Layer 2 networks
pub const SEQUENCER_REQUEST_OPTIMISM: &str = "https://optimism.operationsolarstorm.org/latest";
pub const SEQUENCER_REQUEST_BASE: &str = "https://base.operationsolarstorm.org/latest";

/// have to run helios p2p server to get these
pub const SEQUENCER_REQUEST_OPTIMISM_SEPOLIA: &str =
    "http://127.0.0.1:9547/gossip_getSequencerCommitment";
pub const SEQUENCER_REQUEST_BASE_SEPOLIA: &str =
    "http://127.0.0.1:9545/gossip_getSequencerCommitment";

/// Wrapped Ether (WETH) contract addresses for each supported network
pub const WETH_ETHEREUM: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
pub const WETH_LINEA: Address = address!("e5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f");
pub const WETH_ARBITRUM: Address = address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1");
pub const WETH_OPTIMISM: Address = address!("4200000000000000000000000000000000000006");
pub const WETH_BASE: Address = address!("4200000000000000000000000000000000000006");
pub const WETH_SCROLL: Address = address!("5300000000000000000000000000000000000004");

pub const WETH_ETHEREUM_SEPOLIA: Address = address!("7b79995e5f793A07Bc00c21412e50Ecae098E7f9");
pub const WETH_LINEA_SEPOLIA: Address = address!("f7F895d17DBA17A872041bebbb648F95adE2aEF9");
pub const WETH_ARBITRUM_SEPOLIA: Address = address!("802CC0F559eBc79DA798bf3F3baB44141a1a06Ed");
pub const WETH_OPTIMISM_SEPOLIA: Address = address!("4200000000000000000000000000000000000006");
pub const WETH_BASE_SEPOLIA: Address = address!("4200000000000000000000000000000000000006");
pub const WETH_SCROLL_SEPOLIA: Address = address!("5300000000000000000000000000000000000004");
