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
    "http://localhost:8547";
pub const RPC_URL_SCROLL_SEPOLIA: &str =
    "https://scroll-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_ETHEREUM_SEPOLIA: &str =
    "https://eth-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_BASE_SEPOLIA: &str =
    "https://base-sepolia.g.alchemy.com/v2/XJ0Ro-Iy8q_T-F4O9mUn_oRWY0x57sGK";
pub const RPC_URL_OPTIMISM_SEPOLIA: &str = "http://localhost:8545";
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
