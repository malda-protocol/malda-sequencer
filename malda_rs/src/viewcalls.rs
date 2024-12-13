//! Ethereum view call utilities for cross-chain view call proof.
//!
//! This module provides functionality to:
//! - Fetch user token balances across different EVM chains
//! - Handle sequencer commitments for L2 chains
//! - Manage L1 block verification
//! - Process linking blocks for reorg protection

use alloy_primitives::Address;
use anyhow::Error;

use crate::types::{ExecutionPayload, IL1Block, SequencerCommitment, IERC20};
use alloy_consensus::Header;
use risc0_steel::{
    ethereum::EthEvmEnv, host::BlockNumberOrTag, serde::RlpHeader, Contract, EvmInput,
};
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, ProveInfo, SessionInfo};
use tokio;
use url::Url;

use crate::constants::*;
use methods::BALANCE_OF_ELF;

/// Proves a user's token balance on a specified chain using the RISC Zero prover.
///
/// # Arguments
///
/// * `user` - The user's address to query
/// * `asset` - The token contract address
/// * `chain_id` - The chain identifier
///
/// # Returns
///
/// Returns a `Result` containing the `ProveInfo` or an error
pub async fn get_user_balance_prove(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> Result<ProveInfo, Error> {
    // Move all the work including env creation into the blocking task
    let prove_info = tokio::task::spawn_blocking(move || {
        // Create a new runtime for async operations within the blocking task
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Execute the async env creation in the new runtime
        let env = rt.block_on(get_user_balance_zkvm_env(user, asset, chain_id));

        // Perform the proving
        default_prover().prove(env, BALANCE_OF_ELF)
    })
    .await?;

    prove_info
}

/// Executes a user's token balance query on a specified chain using the RISC Zero executor.
///
/// # Arguments
///
/// * `user` - The user's address to query
/// * `asset` - The token contract address
/// * `chain_id` - The chain identifier
///
/// # Returns
///
/// Returns a `Result` containing the `SessionInfo` or an error
pub async fn get_user_balance_exec(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> Result<SessionInfo, Error> {
    let env = get_user_balance_zkvm_env(user, asset, chain_id).await;
    default_executor().execute(env, BALANCE_OF_ELF)
}

/// Creates a RISC Zero executor environment for token balance queries.
///
/// This function sets up the necessary environment for querying token balances,
/// handling different chain-specific requirements including sequencer commitments,
/// L1 block verification, and linking blocks for reorg protection.
///
/// # Arguments
///
/// * `user` - The user's address to query
/// * `asset` - The token contract address
/// * `chain_id` - The chain identifier
///
/// # Returns
///
/// Returns an `ExecutorEnv` configured for the balance query
///
/// # Panics
///
/// Panics if an invalid chain ID is provided
pub async fn get_user_balance_zkvm_env(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> ExecutorEnv<'static> {
    let rpc_url = match chain_id {
        BASE_CHAIN_ID => RPC_URL_BASE,
        OPTIMISM_CHAIN_ID => RPC_URL_OPTIMISM,
        LINEA_CHAIN_ID => RPC_URL_LINEA,
        ETHEREUM_CHAIN_ID => RPC_URL_ETHEREUM,
        OPTIMISM_SEPOLIA_CHAIN_ID => RPC_URL_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => RPC_URL_BASE_SEPOLIA,
        LINEA_SEPOLIA_CHAIN_ID => RPC_URL_LINEA_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => RPC_URL_ETHEREUM_SEPOLIA,
        _ => panic!("Invalid chain ID"),
    };

    let (block, commitment) = if chain_id == OPTIMISM_CHAIN_ID
        || chain_id == BASE_CHAIN_ID
        || chain_id == ETHEREUM_CHAIN_ID
        || chain_id == OPTIMISM_SEPOLIA_CHAIN_ID
        || chain_id == BASE_SEPOLIA_CHAIN_ID
        || chain_id == ETHEREUM_SEPOLIA_CHAIN_ID
    {
        let (commitment, block) = get_current_sequencer_commitment(chain_id).await;
        (Some(block), Some(commitment))
    } else {
        let block = EthEvmEnv::builder()
            .rpc(Url::parse(rpc_url).unwrap())
            .block_number_or_tag(BlockNumberOrTag::Latest)
            .build()
            .await
            .unwrap()
            .header()
            .inner()
            .inner()
            .number;
        (Some(block), None)
    };

    let (l1_block_call_input, ethereum_block) =
        if chain_id == ETHEREUM_CHAIN_ID || chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
            let chain_id = if chain_id == ETHEREUM_CHAIN_ID {
                OPTIMISM_CHAIN_ID
            } else {
                OPTIMISM_SEPOLIA_CHAIN_ID
            };
            let (l1_block_call_input, ethereum_block) =
                get_l1block_call_input(BlockNumberOrTag::Number(block.unwrap()), chain_id).await;

            (Some(l1_block_call_input), Some(ethereum_block))
        } else {
            (None, None)
        };

    let block = match chain_id {
        BASE_CHAIN_ID => block.unwrap(),
        OPTIMISM_CHAIN_ID => block.unwrap(),
        LINEA_CHAIN_ID => block.unwrap(),
        ETHEREUM_CHAIN_ID => ethereum_block.unwrap(),
        ETHEREUM_SEPOLIA_CHAIN_ID => ethereum_block.unwrap(),
        BASE_SEPOLIA_CHAIN_ID => block.unwrap(),
        OPTIMISM_SEPOLIA_CHAIN_ID => block.unwrap(),
        LINEA_SEPOLIA_CHAIN_ID => block.unwrap(),
        _ => panic!("Invalid chain ID"),
    };

    let linking_blocks = get_linking_blocks(chain_id, rpc_url, block).await;
    let balance_call_input = get_balance_call_input(chain_id, rpc_url, block, user, asset).await;

    ExecutorEnv::builder()
        .write(&balance_call_input)
        .unwrap()
        .write(&chain_id)
        .unwrap()
        .write(&user)
        .unwrap()
        .write(&asset)
        .unwrap()
        .write(&commitment)
        .unwrap()
        .write(&l1_block_call_input)
        .unwrap()
        .write(&linking_blocks)
        .unwrap()
        .build()
        .unwrap()
}

/// Constructs an EVM input for a balance query.
///
/// # Arguments
///
/// * `chain_url` - RPC endpoint URL for the target chain
/// * `block` - Block number or tag (latest) to query
/// * `user` - Address of the user
/// * `asset` - Token contract address
///
/// # Returns
///
/// Returns an `EvmInput` containing the encoded balance call
pub async fn get_balance_call_input(
    chain_id: u64,
    chain_url: &str,
    block: u64,
    user: Address,
    asset: Address,
) -> EvmInput<RlpHeader<Header>> {
    let reorg_protection_depth = match chain_id {
        OPTIMISM_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM,
        BASE_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE,
        LINEA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA,
        ETHEREUM_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM,
        SCROLL_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL,
        OPTIMISM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE_SEPOLIA,
        LINEA_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM_SEPOLIA,
        SCROLL_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL_SEPOLIA,
        _ => panic!("invalid chain id"),
    };

    let block_reorg_protected = block - reorg_protection_depth;

    let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(chain_url).unwrap())
        .block_number_or_tag(BlockNumberOrTag::Number(block_reorg_protected))
        .build()
        .await
        .unwrap();

    let call = IERC20::balanceOfCall { account: user };

    let mut contract = Contract::preflight(asset, &mut env);
    let _returns = contract.call_builder(&call).call().await.unwrap();

    env.into_input().await.unwrap()
}

/// Fetches the current sequencer commitment for L2 chains.
///
/// # Arguments
///
/// * `chain_id` - The chain identifier (Base or Optimism)
///
/// # Returns
///
/// Returns a tuple of (SequencerCommitment, BlockNumberOrTag)
///
/// # Panics
///
/// Panics if an invalid chain ID is provided
pub async fn get_current_sequencer_commitment(chain_id: u64) -> (SequencerCommitment, u64) {
    let req = match chain_id {
        BASE_CHAIN_ID => SEQUENCER_REQUEST_BASE,
        OPTIMISM_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM,
        ETHEREUM_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM,
        OPTIMISM_SEPOLIA_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => SEQUENCER_REQUEST_BASE_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM_SEPOLIA,
        _ => {
            panic!("Invalid chain ID");
        }
    };
    let commitment = reqwest::get(req)
        .await
        .unwrap()
        .json::<SequencerCommitment>()
        .await
        .unwrap();

    let block = ExecutionPayload::try_from(&commitment)
        .unwrap()
        .block_number;

    (commitment, block)
}

/// Retrieves L1 block information for L2 chains.
///
/// # Arguments
///
/// * `block` - Block number or tag to query
/// * `chain_id` - The chain identifier
///
/// # Returns
///
/// Returns a tuple containing the L1 block call input and block number
///
/// # Panics
///
/// Panics if an invalid chain ID is provided
pub async fn get_l1block_call_input(
    block: BlockNumberOrTag,
    chain_id: u64,
) -> (EvmInput<RlpHeader<Header>>, u64) {
    let rpc_url = match chain_id {
        BASE_CHAIN_ID => RPC_URL_BASE,
        OPTIMISM_CHAIN_ID => RPC_URL_OPTIMISM,
        BASE_SEPOLIA_CHAIN_ID => RPC_URL_BASE_SEPOLIA,
        OPTIMISM_SEPOLIA_CHAIN_ID => RPC_URL_OPTIMISM_SEPOLIA,

        _ => {
            panic!("Invalid chain ID");
        }
    };
    let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(rpc_url).unwrap())
        .block_number_or_tag(block)
        .build()
        .await
        .unwrap();

    let call = IL1Block::hashCall {};
    let mut contract = Contract::preflight(L1_BLOCK_ADDRESS_OPTIMISM, &mut env);
    let _l1_block_hash = contract.call_builder(&call).call().await.unwrap()._0;
    let view_call_input_l1_block = env.into_input().await.unwrap();

    let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(rpc_url).unwrap())
        .block_number_or_tag(block)
        .build()
        .await
        .unwrap();

    let call = IL1Block::numberCall {};
    let mut contract = Contract::preflight(L1_BLOCK_ADDRESS_OPTIMISM, &mut env);
    let l1_block = contract.call_builder(&call).call().await.unwrap()._0;

    (view_call_input_l1_block, l1_block)
}

/// Fetches a sequence of Ethereum blocks for reorg protection.
///
/// # Arguments
///
/// * `current_block` - The latest block number to start from
///
/// # Returns
///
/// Returns a tuple containing:
/// - Vector of block headers for the reorg protection window
/// - The block number before the start of the window
pub async fn get_linking_blocks(
    chain_id: u64,
    rpc_url: &str,
    current_block: u64,
) -> Vec<RlpHeader<Header>> {
    let reorg_protection_depth = match chain_id {
        OPTIMISM_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM,
        BASE_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE,
        LINEA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA,
        ETHEREUM_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM,
        SCROLL_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL,
        OPTIMISM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE_SEPOLIA,
        LINEA_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM_SEPOLIA,
        SCROLL_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL_SEPOLIA,
        _ => panic!("invalid chain id"),
    };

    let mut linking_blocks = vec![];

    let start_block = current_block - reorg_protection_depth + 1;

    for block_nr in (start_block)..=(current_block) {
        let env = EthEvmEnv::builder()
            .rpc(Url::parse(rpc_url).unwrap())
            .block_number_or_tag(BlockNumberOrTag::Number(block_nr))
            .build()
            .await
            .unwrap();
        let header = env.header().inner().clone();
        linking_blocks.push(header);
    }
    linking_blocks
}
