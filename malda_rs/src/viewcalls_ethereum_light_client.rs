//! Ethereum view call utilities for cross-chain view call proof.
//!
//! This module provides functionality to:
//! - Generate zero-knowledge proofs for token balance queries across EVM chains
//! - Execute and verify token balance queries using RISC Zero
//! - Handle Ethereum consensus layer (beacon chain) data verification
//! - Process block headers for reorg protection
//! - Build execution environments for zero-knowledge proofs

use alloy_primitives::{Address, B256};
use alloy_primitives_old::B256 as OldB256;
use alloy_consensus::Header;

use consensus_core::{calc_sync_period, types::{Bootstrap, OptimisticUpdate, Update}};
use consensus::rpc::{ConsensusRpc, nimbus_rpc::NimbusRpc};

use risc0_steel::{serde::RlpHeader, ethereum::{EthEvmInput, EthEvmEnv}, host::BlockNumberOrTag, Contract, EvmInput};
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, ProveInfo, SessionInfo};

use tokio;
use url::Url;
use anyhow::Error;

use crate::types::{SequencerCommitment, IERC20};
use crate::constants::*;
use methods::BALANCE_OF_ETHEREUM_LIGHT_CLIENT_ELF;




pub const RPC_URL_BEACON: &str = "https://www.lightclientdata.org";

/// Generates a zero-knowledge proof for a user's token balance query.
///
/// # Arguments
///
/// * `user` - The user's Ethereum address
/// * `asset` - The token contract address to query
/// * `chain_id` - The target chain identifier
/// * `trusted_hash` - The trusted beacon chain block hash to anchor verification from
///
/// # Returns
///
/// Returns a `Result` containing the zero-knowledge `ProveInfo` or an error
pub async fn get_user_balance_prove(
    user: Address,
    asset: Address,
    chain_id: u64,
    trusted_hash: B256,
) -> Result<ProveInfo, Error> {
    // Move all the work including env creation into the blocking task
    let prove_info = tokio::task::spawn_blocking(move || {
        // Create a new runtime for async operations within the blocking task
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Execute the async env creation in the new runtime
        let env = rt.block_on(get_user_balance_zkvm_env(
            user,
            asset,
            chain_id,
            trusted_hash,
        ));

        // Perform the proving
        default_prover().prove(env, BALANCE_OF_ETHEREUM_LIGHT_CLIENT_ELF)
    })
    .await?;

    prove_info
}

/// Executes a token balance query without generating a proof.
///
/// Useful for testing and debugging balance queries before generating proofs.
///
/// # Arguments
///
/// * `user` - The user's Ethereum address
/// * `asset` - The token contract address to query
/// * `chain_id` - The target chain identifier
/// * `trusted_hash` - The trusted beacon chain block hash to anchor verification from
///
/// # Returns
///
/// Returns a `Result` containing the execution `SessionInfo` or an error
pub async fn get_user_balance_exec(
    user: Address,
    asset: Address,
    chain_id: u64,
    trusted_hash: B256,
) -> Result<SessionInfo, Error> {
    let env = get_user_balance_zkvm_env(user, asset, chain_id, trusted_hash).await;
    default_executor().execute(env, BALANCE_OF_ETHEREUM_LIGHT_CLIENT_ELF)
}

/// Creates a RISC Zero executor environment for token balance queries.
///
/// This function:
/// 1. Fetches and validates beacon chain consensus data
/// 2. Retrieves necessary block headers for reorg protection
/// 3. Prepares the balance query call data
/// 4. Builds a complete environment for zero-knowledge proof generation
///
/// # Arguments
///
/// * `user` - The user's Ethereum address
/// * `asset` - The token contract address to query
/// * `chain_id` - The target chain identifier
/// * `trusted_hash` - The trusted beacon chain block hash to anchor verification from
///
/// # Returns
///
/// Returns an `ExecutorEnv` configured for generating balance query proofs
///
/// # Panics
///
/// Panics if an unsupported chain ID is provided
pub async fn get_user_balance_zkvm_env(
    user: Address,
    asset: Address,
    chain_id: u64,
    trusted_hash: B256,
) -> ExecutorEnv<'static> {
    let (rpc_url, rpc_url_beacon) = match chain_id {
        ETHEREUM_CHAIN_ID => (RPC_URL_ETHEREUM, RPC_URL_BEACON),
        _ => panic!("Invalid chain ID"),
    };

    let beacon_rpc = NimbusRpc::new(rpc_url_beacon);
    let beacon_root = OldB256::from(trusted_hash.0);
    let bootstrap: Bootstrap = beacon_rpc.get_bootstrap(beacon_root).await.unwrap();
    let current_period = calc_sync_period(bootstrap.header.slot);

    let updates: Vec<Update> = beacon_rpc.get_updates(current_period, 10).await.unwrap();
    let finality_update = beacon_rpc.get_optimistic_update().await.unwrap();

    // let current_beacon_root = finality_update.attested_header.tree_root_hash();
    let beacon_block_slot = finality_update.attested_header.slot;
    let beacon_block = beacon_rpc.get_block(beacon_block_slot).await.unwrap();
    let block = beacon_block.body.execution_payload().block_number().clone();

    let linking_blocks = get_linking_blocks(chain_id, rpc_url, block).await;
    let balance_call_input = get_balance_call_input(chain_id, rpc_url, block, user, asset).await;

    let beacon_input = get_balance_call_input(chain_id, rpc_url, block + REORG_PROTECTION_DEPTH_ETHEREUM, user, asset).await;

    build_l1_chain_builder_environment(
        balance_call_input,
        chain_id,
        user,
        asset,
        None,
        None,
        linking_blocks,
        bootstrap,
        beacon_root,
        updates,
        finality_update,
        beacon_input,
    )
}

/// Constructs an EVM input for a balance query.
///
/// Prepares the encoded EVM call data for querying an ERC20 token's balanceOf function,
/// taking into account chain-specific reorg protection depths.
///
/// # Arguments
///
/// * `chain_id` - The target chain identifier
/// * `chain_url` - RPC endpoint URL for the target chain
/// * `block` - Block number to query the balance at
/// * `user` - Address of the user to query
/// * `asset` - Token contract address to query
///
/// # Returns
///
/// Returns an `EvmInput` containing the encoded balance call and block header data
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
        .beacon_api(Url::parse(RPC_URL_BEACON).unwrap())
        .build()
        .await
        .unwrap();

    let call = IERC20::balanceOfCall { account: user };

    let mut contract = Contract::preflight(asset, &mut env);
    let _returns = contract.call_builder(&call).call().await.unwrap();

    env.into_input().await.unwrap()
}

/// Fetches a sequence of Ethereum blocks for reorg protection.
///
/// Retrieves a continuous sequence of block headers starting from a given block,
/// going back by the chain-specific reorg protection depth. This ensures the
/// balance proof remains valid even if a chain reorganization occurs.
///
/// # Arguments
///
/// * `chain_id` - The target chain identifier
/// * `rpc_url` - RPC endpoint URL for the target chain
/// * `current_block` - The latest block number to start from
///
/// # Returns
///
/// Returns a vector of block headers covering the reorg protection window
///
/// # Panics
///
/// Panics if an unsupported chain ID is provided
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

/// Builds a complete RISC Zero environment for L1 chain verification.
///
/// Assembles all necessary components for verifying L1 data, including:
/// - View call inputs and chain identification
/// - User and asset addresses
/// - Sequencer commitments (for L2 chains)
/// - Block headers for reorg protection
/// - Beacon chain consensus data
/// - Additional verification data for the beacon chain
///
/// This environment enables zero-knowledge proofs that demonstrate valid
/// token balance queries while ensuring consensus-layer security.
pub fn build_l1_chain_builder_environment(
    view_call_input: EvmInput<RlpHeader<Header>>,
    chain_id: u64,
    user: Address,
    asset: Address,
    sequencer_commitment: Option<SequencerCommitment>,
    env_op_input: Option<EthEvmInput>,
    linking_blocks: Vec<RlpHeader<Header>>,
    bootstrap: Bootstrap,
    checkpoint: OldB256,
    updates: Vec<Update>,
    finality_update: OptimisticUpdate,
    beacon_input: EvmInput<RlpHeader<Header>>,
) -> risc0_zkvm::ExecutorEnv<'static> {
    let mut env = risc0_zkvm::ExecutorEnv::builder();
    env.write(&view_call_input)
        .unwrap()
        .write(&chain_id)
        .unwrap()
        .write(&user)
        .unwrap()
        .write(&asset)
        .unwrap()
        .write(&sequencer_commitment)
        .unwrap()
        .write(&env_op_input)
        .unwrap()
        .write(&linking_blocks)
        .unwrap()
        .write(&bootstrap.header)
        .unwrap()
        .write(&bootstrap.current_sync_committee)
        .unwrap()
        .write(&bootstrap.current_sync_committee_branch)
        .unwrap()
        .write(&checkpoint)
        .unwrap()
        .write(&finality_update.attested_header)
        .unwrap()
        .write(&finality_update.sync_aggregate)
        .unwrap()
        .write(&finality_update.signature_slot)
        .unwrap()
        .write(&updates.len())
        .unwrap();

    for update in updates {
        env.write(&update.attested_header).unwrap();
        env.write(&update.next_sync_committee).unwrap();
        env.write(&update.next_sync_committee_branch).unwrap();
        env.write(&update.finalized_header).unwrap();
        env.write(&update.finality_branch).unwrap();
        env.write(&update.sync_aggregate).unwrap();
        env.write(&update.signature_slot).unwrap();
    }

    env.write(&beacon_input).unwrap();

    env.build().unwrap()
}
