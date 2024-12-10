//! Ethereum view call utilities for cross-chain view call proof.
//!
//! This module provides functionality to:
//! - Fetch user token balances across different EVM chains
//! - Handle sequencer commitments for L2 chains
//! - Manage L1 block verification
//! - Process linking blocks for reorg protection

use alloy_primitives::{Address, B256};
use anyhow::Error;

use consensus_core::calc_sync_period;
use consensus_core::types::{Bootstrap, FinalityUpdate, OptimisticUpdate, Update};
use risc0_steel::{ethereum::EthEvmEnv, host::BlockNumberOrTag, Commitment, Contract};
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, ProveInfo, SessionInfo};
use tokio;
use url::Url;

use consensus::rpc::nimbus_rpc::NimbusRpc;
use consensus::rpc::ConsensusRpc;

use alloy_primitives_old::B256 as OldB256;

use alloy::providers::ProviderBuilder;

use risc0_steel::serde::RlpHeader;
use risc0_steel::EvmInput;

use alloy_consensus::Header;

use risc0_zkvm;

use crate::types::IERC20;

use crate::constants::*;
use methods::BALANCE_OF_ETHEREUM_LIGHT_CLIENT_ELF;

pub const RPC_URL_BEACON: &str = "https://www.lightclientdata.org";

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
    trusted_hash: B256
) -> Result<ProveInfo, Error> {
    // Move all the work including env creation into the blocking task
    let prove_info = tokio::task::spawn_blocking(move || {
        // Create a new runtime for async operations within the blocking task
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Execute the async env creation in the new runtime
        let env = rt.block_on(get_user_balance_zkvm_env(user, asset, chain_id, trusted_hash));

        // Perform the proving
        default_prover().prove(env, BALANCE_OF_ETHEREUM_LIGHT_CLIENT_ELF)
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
    trusted_hash: B256
) -> Result<SessionInfo, Error> {
    let env = get_user_balance_zkvm_env(user, asset, chain_id, trusted_hash).await;
    default_executor().execute(env, BALANCE_OF_ETHEREUM_LIGHT_CLIENT_ELF)
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
    trusted_hash: B256
) -> ExecutorEnv<'static> {
    let (rpc_url, rpc_url_beacon) = match chain_id {
        ETHEREUM_CHAIN_ID => (RPC_URL_ETHEREUM, RPC_URL_BEACON),
        _ => panic!("Invalid chain ID"),
    };

    // let provider = ProviderBuilder::new()
    // .with_recommended_fillers()
    // .on_http(Url::parse(chain_url).unwrap());

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

    build_l1_chain_builder_environment(
        &balance_call_input,
        user,
        bootstrap,
        beacon_root,
        updates,
        finality_update,
    )
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

#[tokio::test]
async fn proves_check_liquidity_l1() {
    // this is the same as check_liquidity but an additional proof that L1 state is valid to be used on different chains
    let chain_url = RPC_URL_ETHEREUM;
    let user = Address::ZERO;
    let asset = WETH_ETHEREUM;
    let block = 20770922; // we fix this in case account removes liquidity

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .on_http(Url::parse(chain_url).unwrap());

    let beacon_url = "https://www.lightclientdata.org";
    let beacon_rpc = NimbusRpc::new(beacon_url);
    let bytes: [u8; 32] = [
        0xe5, 0x73, 0xae, 0x24, 0xd2, 0xb8, 0x28, 0xb0, 0x1c, 0x9f, 0xf9, 0x31, 0xf7, 0xb0, 0xe3,
        0xfe, 0xde, 0x9d, 0x03, 0xfd, 0xf5, 0x86, 0x6a, 0xfd, 0xc0, 0x52, 0x30, 0x29, 0x81, 0x98,
        0x24, 0x1b,
    ];
    let beacon_root = OldB256::from(bytes);
    let bootstrap: Bootstrap = beacon_rpc.get_bootstrap(beacon_root).await.unwrap();
    let current_period = calc_sync_period(bootstrap.header.slot);

    let updates: Vec<Update> = beacon_rpc.get_updates(current_period, 10).await.unwrap();
    let finality_update = beacon_rpc.get_optimistic_update().await.unwrap();

    // let current_beacon_root = finality_update.attested_header.tree_root_hash();
    let beacon_block_slot = finality_update.attested_header.slot;
    let beacon_block = beacon_rpc.get_block(beacon_block_slot).await.unwrap();
    let block = beacon_block.body.execution_payload().block_number().clone();

    let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(chain_url).unwrap())
        .block_number_or_tag(BlockNumberOrTag::Number(block))
        .beacon_api(Url::parse(beacon_url).unwrap())
        .build()
        .await
        .unwrap();

    let block_number = env.header().inner().number;
    println!("block_number: {}", block_number);

    let call = IERC20::balanceOfCall { account: user };

  

}

pub fn build_l1_chain_builder_environment(
    view_call_input: &EvmInput<RlpHeader<Header>>,
    user: Address,
    bootstrap: Bootstrap,
    checkpoint: OldB256,
    updates: Vec<Update>,
    finality_update: OptimisticUpdate,
) -> risc0_zkvm::ExecutorEnv {
    let mut env = risc0_zkvm::ExecutorEnv::builder();
    env.write(&view_call_input)
        .unwrap()
        .write(&user)
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
        // .write(&finality_update.finalized_header)
        // .unwrap()
        // .write(&finality_update.finality_branch)
        // .unwrap()
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

    env.build().unwrap()
}
