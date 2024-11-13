use alloy_consensus::Header;
use alloy_primitives::{B256, Signature, U256};
use risc0_steel::{ethereum::EthEvmInput, Contract, serde::RlpHeader};
use crate::types::*;
use crate::constants::{OPTIMISM_CHAIN_ID, OPTIMISM_SEQUENCER, L1_BLOCK_ADDRESS_OPTIMISM, LINEA_SEQUENCER, BASE_CHAIN_ID, BASE_SEQUENCER, REORG_PROTECTION_DEPTH};
use crate::cryptography::recover_signer;


pub fn validate_linea_env(header: risc0_steel::ethereum::EthBlockHeader) {
    // extract sequencer signature from extra data
    let extra_data = header.inner().extra_data.clone();

    let length = extra_data.len();
    let signature = extra_data.slice(length - 65..length);
    let prefix = extra_data.slice(0..length - 65);

    let r_array: [u8; 32] = signature.slice(0..32).to_vec().try_into().unwrap();
    let r = U256::from_be_bytes(r_array);

    let s_array: [u8; 32] = signature.slice(32..64).to_vec().try_into().unwrap();
    let s = U256::from_be_bytes(s_array);

    let v_array: [u8; 1] = signature.slice(64..65).to_vec().try_into().unwrap();
    let v = v_array[0] == 1;

    let sig = Signature::from_rs_and_parity(r, s, v).unwrap();

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

pub fn validate_opstack_env(chain_id: u64, commitment: &SequencerCommitment, env_block_hash: B256) {

    if chain_id == OPTIMISM_CHAIN_ID {
        commitment.verify(OPTIMISM_SEQUENCER, OPTIMISM_CHAIN_ID).unwrap();
    } else if chain_id == BASE_CHAIN_ID {
        commitment.verify(BASE_SEQUENCER, BASE_CHAIN_ID).unwrap();
    } else {
        panic!("invalid chain id");
    }
    let payload = ExecutionPayload::try_from(commitment).unwrap();
    assert_eq!(payload.block_hash, env_block_hash, "block hash mismatch");
}

pub fn validate_ethereum_env_via_opstack(commitment: SequencerCommitment, ethereum_hash: B256, input_op: EthEvmInput, linking_blocks: Vec<RlpHeader<Header>>) {

    let env_op = input_op.into_env();
    validate_opstack_env(OPTIMISM_CHAIN_ID, &commitment, env_op.commitment().digest);
    let l1_block = Contract::new(L1_BLOCK_ADDRESS_OPTIMISM, &env_op);
    let call = IL1Block::hashCall { };
    let l1_hash = l1_block.call_builder(&call).call()._0;

    validate_chain_length(ethereum_hash, linking_blocks, l1_hash);
}

pub fn validate_chain_length(historical_hash: B256, linking_blocks: Vec<RlpHeader<Header>>, current_hash: B256) {
    let chain_length = linking_blocks.len() as u64;
    assert!(chain_length >= REORG_PROTECTION_DEPTH, "chain length is less than reorg protection");
    let mut previous_hash = historical_hash;
    for header in linking_blocks {
        let parent_hash = header.parent_hash;
        assert_eq!(parent_hash, previous_hash, "blocks not hashlinked");
        previous_hash = header.hash_slow();
    }
    assert_eq!(previous_hash, current_hash, "last hash doesnt correspond to current l1 hash");
}