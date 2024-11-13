//! Ethereum view call utilities for cross-chain balance verification.
//! 
//! This module provides functionality to:
//! - Fetch user token balances across different EVM chains
//! - Handle sequencer commitments for L2 chains
//! - Manage L1 block verification
//! - Process linking blocks for reorg protection

use alloy_primitives::Address;
use anyhow::Error;

use risc0_steel::{ethereum::EthEvmEnv, host::BlockNumberOrTag, serde::RlpHeader, Contract, EvmInput};
use risc0_zkvm::{default_executor, ExecutorEnv, SessionInfo};
use url::Url;
use guest_utils::*;
use alloy_consensus::Header;

use crate::constants::*;
use methods::BALANCE_OF_ELF;

/// Retrieves and verifies a user's token balance on specified EVM chain.
/// 
/// # Arguments
/// 
/// * `user` - The address of the user whose balance is being queried
/// * `asset` - The token contract address
/// * `chain_id` - The chain identifier (Ethereum, Optimism, Base, or Linea)
/// 
/// # Returns
/// 
/// Returns a `SessionInfo` containing the verified balance information
/// 
/// # Errors
/// 
/// Returns an `Error` if:
/// - RPC connection fails
/// - Contract calls fail
/// - Invalid chain ID is provided
pub async fn get_user_balance(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> Result<SessionInfo, Error> {

    let rpc_url = match chain_id {
        BASE_CHAIN_ID => RPC_URL_BASE,
        OPTIMISM_CHAIN_ID => RPC_URL_OPTIMISM,
        LINEA_CHAIN_ID => RPC_URL_LINEA,
        ETHEREUM_CHAIN_ID => RPC_URL_ETHEREUM,
        _ => panic!("Invalid chain ID"),
    };

    let (block, commitment) = if chain_id == OPTIMISM_CHAIN_ID || chain_id == BASE_CHAIN_ID || chain_id == ETHEREUM_CHAIN_ID {
        let (commitment, block) = get_current_sequencer_commitment(chain_id).await;
        (Some(block), Some(commitment))
    } else {
        (None, None)
    };

    

    let (l1_block_call_input, linking_blocks, ethereum_block) = if chain_id == ETHEREUM_CHAIN_ID {
        let (l1_block_call_input, l1_block) = get_l1block_call_input(block.unwrap(), OPTIMISM_CHAIN_ID).await;
        let (linking_blocks, ethereum_block) = get_linking_blocks_ethereum(l1_block).await;
        (
            Some(l1_block_call_input),
            Some(linking_blocks),
            Some(ethereum_block)
        )
    } else {
        (None, None, None)
    };

    let block = match chain_id {
        BASE_CHAIN_ID => block.unwrap(),
        OPTIMISM_CHAIN_ID => block.unwrap(),
        LINEA_CHAIN_ID => BlockNumberOrTag::Latest,
        ETHEREUM_CHAIN_ID => BlockNumberOrTag::Number(ethereum_block.unwrap()),
        _ => panic!("Invalid chain ID"),
    };

    let balance_call_input = get_balance_call_input(rpc_url, block, user, asset).await;


    let mut env_builder = ExecutorEnv::builder();
        env_builder
        .write(&balance_call_input)
        .unwrap()
        .write(&chain_id)
        .unwrap()
        .write(&user)
        .unwrap()
        .write(&asset)
        .unwrap();

    if let Some(commitment) = commitment {
        env_builder.write(&commitment)
        .unwrap();
        };

    if let Some(l1_block_input) = l1_block_call_input {
        env_builder.write(&l1_block_input)
        .unwrap();
        };

    if let Some(linking_blocks) = linking_blocks {
        env_builder.write(&linking_blocks)
        .unwrap();
        };

    
    let env = env_builder.build().unwrap();

    // NOTE: Use the executor to run tests without proving.
    default_executor().execute(env, BALANCE_OF_ELF)
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
async fn get_balance_call_input(chain_url: &str, block: BlockNumberOrTag, user: Address, asset: Address) -> EvmInput<RlpHeader<Header>> {
    let mut env = EthEvmEnv::builder()
    .rpc(Url::parse(chain_url).unwrap())
    .block_number_or_tag(block)
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
async fn get_current_sequencer_commitment(chain_id: u64) -> (SequencerCommitment, BlockNumberOrTag) {
    let req = match chain_id {
        BASE_CHAIN_ID => {
            SEQUENCER_REQUEST_BASE
        }
        OPTIMISM_CHAIN_ID => {
            SEQUENCER_REQUEST_OPTIMISM
        }
        ETHEREUM_CHAIN_ID => {
            SEQUENCER_REQUEST_OPTIMISM
        }
        _ => {
            panic!("Invalid chain ID");
        }
    };
    let commitment = reqwest::get(req)
    .await.unwrap()
    .json::<SequencerCommitment>()
    .await.unwrap();

    let block = ExecutionPayload::try_from(&commitment).unwrap().block_number;

    (commitment, BlockNumberOrTag::Number(block))
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
async fn get_l1block_call_input(block: BlockNumberOrTag, chain_id: u64) -> (EvmInput<RlpHeader<Header>>, u64) {
    let rpc_url = match chain_id {
        BASE_CHAIN_ID => {
            RPC_URL_BASE
        }
        OPTIMISM_CHAIN_ID => {
            RPC_URL_OPTIMISM
        }
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

    let call = IL1Block::hashCall { };
    let mut contract = Contract::preflight(L1_BLOCK_ADDRESS_OPTIMISM, &mut env);
    let _l1_block_hash = contract.call_builder(&call).call().await.unwrap()._0;
    let view_call_input_l1_block = env.into_input().await.unwrap();

    let mut env = EthEvmEnv::builder()
    .rpc(Url::parse(rpc_url).unwrap())
    .block_number_or_tag(block)
    .build()
    .await
    .unwrap();

    let call = IL1Block::numberCall { };
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
async fn get_linking_blocks_ethereum(current_block: u64) -> (Vec<RlpHeader<Header>>, u64) {

    let mut linking_blocks = vec![];

    let start_block = current_block - REORG_PROTECTION_DEPTH + 1;

    for block_nr in (start_block)..=(current_block) {
        let env = EthEvmEnv::builder()
        .rpc(Url::parse(RPC_URL_ETHEREUM).unwrap())
        .block_number_or_tag(BlockNumberOrTag::Number(block_nr))
        .build()
        .await
        .unwrap();
        let header = env.header().inner().clone();
        linking_blocks.push(header);
    }
    (linking_blocks, start_block - 1)
}

#[cfg(test)]
mod tests {
    // use super::*;

    #[test]
    fn it_works() {

    }
}
