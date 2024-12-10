//! Validator functions for verifying blockchain environments and commitments.
//!
//! This module provides validation utilities for different blockchain environments including:
//! - Linea environment validation
//! - OpStack (Optimism/Base) environment validation
//! - Ethereum environment validation via OpStack
//! - Chain length and hash linking validation
use crate::constants::*;
use crate::cryptography::{recover_signer, signature_from_bytes};
use crate::types::*;
use alloy_consensus::Header;
use alloy_primitives::Address;
use alloy_primitives::B256;
use alloy_sol_types::SolValue;
use risc0_steel::{ethereum::EthEvmInput, serde::RlpHeader, Contract};
use risc0_zkvm::guest::env;

/// Validates the balance of a given account for a specific asset across different blockchain environments.
///
/// This function is the main logic executed in the risc0 guest program. It validates the balance of a given account
/// for a specific asset across different blockchain environments, including Linea, OpStack (Optimism/Base), and Ethereum.
///
/// # Arguments
/// * `chain_id` - The ID of the blockchain network to validate against
/// * `account` - The address of the account to query the balance for
/// * `asset` - The address of the asset to query the balance for
/// * `env_input` - The Ethereum EVM input for the environment
/// * `sequencer_commitment` - The sequencer commitment for OpStack and Ethereum environments
/// * `op_env_input` - The Ethereum EVM input for the Optimism environment (used for Ethereum validation)
/// * `linking_blocks` - The linking blocks for Ethereum environment validation
///
/// # Details
///
/// This function first constructs an ERC-20 contract instance for the given asset and queries the balance of the given account.
/// Then, it validates the environment based on the `chain_id`:
/// - For Linea, it validates the block header by verifying the sequencer signature.
/// - For OpStack (Optimism/Base), it verifies the sequencer commitment.
/// - For Ethereum, it validates the environment via OpStack by verifying the sequencer commitment, block seal, and linking blocks.
///
/// Finally, it constructs a `Journal` entry with the balance, account, and asset information and commits it to the environment.
pub fn validate_balance_of_call(
    chain_id: u64,
    account: Address,
    asset: Address,
    env_input: EthEvmInput,
    sequencer_commitment: Option<SequencerCommitment>,
    op_env_input: Option<EthEvmInput>,
    linking_blocks: Vec<RlpHeader<Header>>,
) {
    let env = env_input.into_env();

    let erc20_contract = Contract::new(asset, &env);

    let call = IERC20::balanceOfCall { account: account };
    let balance = erc20_contract.call_builder(&call).call()._0;

    let last_block = linking_blocks[linking_blocks.len() - 1].clone();

    let validated_block_hash = if chain_id == LINEA_CHAIN_ID {
        validate_linea_env(last_block.clone());
        last_block.hash_slow()
    } else if chain_id == OPTIMISM_CHAIN_ID || chain_id == BASE_CHAIN_ID {
        let last_block_hash = last_block.hash_slow();
        validate_opstack_env(chain_id, &sequencer_commitment.unwrap(), last_block_hash);
        last_block_hash
    } else if chain_id == ETHEREUM_CHAIN_ID {
        let ethereum_hash = get_ethereum_block_hash_via_opstack(
            sequencer_commitment.unwrap(),
            op_env_input.unwrap(),
        );
        ethereum_hash
    } else {
        panic!("invalid chain id");
    };

    validate_chain_length(
        chain_id,
        env.header().seal(),
        linking_blocks,
        validated_block_hash,
    );

    let journal = Journal {
        balance,
        account,
        asset,
    };
    env::commit_slice(&journal.abi_encode());
}

/// Validates a Linea block header by verifying the sequencer signature.
///
/// # Arguments
/// * `header` - The Linea block header to validate
///
/// # Panics
/// * If the block is not signed by the official Linea sequencer
pub fn validate_linea_env(header: risc0_steel::ethereum::EthBlockHeader) {
    // extract sequencer signature from extra data
    let extra_data = header.inner().extra_data.clone();

    let length = extra_data.len();
    let prefix = extra_data.slice(0..length - 65);
    let signature_bytes = extra_data.slice(length - 65..length);

    let sig = signature_from_bytes(&signature_bytes.try_into().unwrap());

    // hash block without signature
    let mut header = header.inner().clone();
    header.extra_data = prefix;

    let sighash: [u8; 32] = header.hash_slow().to_vec().try_into().unwrap();
    let sighash = B256::new(sighash);

    let sequencer = recover_signer(sig, sighash).unwrap();

    if sequencer != LINEA_SEQUENCER {
        panic!("Block not signed by linea sequencer");
    }
}

/// Validates an OpStack (Optimism/Base) environment by verifying sequencer commitments.
///
/// # Arguments
/// * `chain_id` - The chain ID to validate against (Optimism or Base)
/// * `commitment` - The sequencer commitment to verify
/// * `env_block_hash` - The expected block hash to validate against
///
/// # Panics
/// * If the chain ID is invalid
/// * If the commitment verification fails
/// * If the block hash doesn't match the expected hash
pub fn validate_opstack_env(chain_id: u64, commitment: &SequencerCommitment, env_block_hash: B256) {
    match chain_id {
        OPTIMISM_CHAIN_ID => commitment
            .verify(OPTIMISM_SEQUENCER, OPTIMISM_CHAIN_ID)
            .unwrap(),
        BASE_CHAIN_ID => commitment.verify(BASE_SEQUENCER, BASE_CHAIN_ID).unwrap(),
        OPTIMISM_SEPOLIA_CHAIN_ID => commitment
            .verify(OPTIMISM_SEPOLIA_SEQUENCER, OPTIMISM_SEPOLIA_CHAIN_ID)
            .unwrap(),
        BASE_SEPOLIA_CHAIN_ID => commitment
            .verify(BASE_SEPOLIA_SEQUENCER, BASE_SEPOLIA_CHAIN_ID)
            .unwrap(),
        _ => panic!("invalid chain id"),
    }
    let payload = ExecutionPayload::try_from(commitment).unwrap();
    assert_eq!(payload.block_hash, env_block_hash, "block hash mismatch");
}

/// Validates an Ethereum environment through OpStack by verifying block hashes and chain linking.
///
/// # Arguments
/// * `commitment` - The sequencer commitment for validation
/// * `ethereum_hash` - The Ethereum block hash to verify
/// * `input_op` - The Ethereum EVM input containing environment data
/// * `linking_blocks` - Vector of block headers linking the historical block to current block
///
/// # Panics
/// * If any validation step fails
pub fn get_ethereum_block_hash_via_opstack(
    commitment: SequencerCommitment,
    input_op: EthEvmInput,
) -> B256 {
    let env_op = input_op.into_env();
    validate_opstack_env(OPTIMISM_CHAIN_ID, &commitment, env_op.commitment().digest);
    let l1_block = Contract::new(L1_BLOCK_ADDRESS_OPTIMISM, &env_op);
    let call = IL1Block::hashCall {};
    l1_block.call_builder(&call).call()._0
}

/// Validates the length and integrity of a chain of blocks.
///
/// Ensures that:
/// 1. The chain length meets minimum reorg protection requirements
/// 2. All blocks are properly hash-linked
/// 3. The final hash matches the expected current hash
///
/// # Arguments
/// * `historical_hash` - The hash of the historical block to start validation from
/// * `linking_blocks` - Vector of block headers forming the chain
/// * `current_hash` - The expected hash of the current block
///
/// # Panics
/// * If the chain length is less than the reorg protection depth
/// * If blocks are not properly hash-linked
/// * If the final hash doesn't match the expected current hash
pub fn validate_chain_length(
    chain_id: u64,
    historical_hash: B256,
    linking_blocks: Vec<RlpHeader<Header>>,
    current_hash: B256,
) {
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
    let chain_length = linking_blocks.len() as u64;
    assert!(
        chain_length >= reorg_protection_depth,
        "chain length is less than reorg protection"
    );
    let mut previous_hash = historical_hash;
    for header in linking_blocks {
        let parent_hash = header.parent_hash;
        assert_eq!(parent_hash, previous_hash, "blocks not hashlinked");
        previous_hash = header.hash_slow();
    }
    assert_eq!(
        previous_hash, current_hash,
        "last hash doesnt correspond to current l1 hash"
    );
}
