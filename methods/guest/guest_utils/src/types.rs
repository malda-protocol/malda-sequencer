use alloy_sol_types::sol;

use eyre::Result;
use serde::{Deserialize, Serialize};

use alloy_rlp::RlpEncodable;
use ssz_derive::{Decode, Encode};
use ssz_types::{FixedVector, VariableList, typenum};
use ssz::Decode;

use alloy_primitives::{Bytes, Address, Signature, B256, U256};
use crate::cryptography::signature_msg;

sol! {
    /// ERC-20 balance function signature.
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }

    interface IL1Block {
        function hash() external view returns (bytes32);
        function number() external view returns (uint64);
    }

    struct Journal {
        uint256 balance;
        address user;
        address asset;
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencerCommitment {
    data: Bytes,
    pub signature: Signature,
}

impl SequencerCommitment {
    pub fn new(data: &[u8]) -> Result<Self> {
        let mut decoder = snap::raw::Decoder::new();
        let decompressed = decoder.decompress_vec(&data)?;

        let signature = Signature::try_from(&decompressed[..65])?;
        let data = Bytes::from(decompressed[65..].to_vec());

        Ok(SequencerCommitment { data, signature })
    }

    pub fn verify(&self, signer: Address, chain_id: u64) -> Result<()> {
        let msg = signature_msg(&self.data, chain_id);
        let pk = self.signature.recover_from_prehash(&msg)?;
        let recovered_signer = Address::from_public_key(&pk);

        if signer != recovered_signer {
            eyre::bail!("invalid signer");
        }

        Ok(())
    }
}

impl TryFrom<&SequencerCommitment> for ExecutionPayload {
    type Error = eyre::Report;

    fn try_from(value: &SequencerCommitment) -> Result<Self> {
        let payload_bytes = &value.data[32..];
        ExecutionPayload::from_ssz_bytes(payload_bytes).map_err(|_| eyre::eyre!("decode failed"))
    }
}

#[derive(Debug, Clone, Encode, Decode)]
pub struct ExecutionPayload {
    pub parent_hash: B256,
    pub fee_recipient: Address,
    pub state_root: B256,
    pub receipts_root: B256,
    pub logs_bloom: LogsBloom,
    pub prev_randao: B256,
    pub block_number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub extra_data: ExtraData,
    pub base_fee_per_gas: U256,
    pub block_hash: B256,
    pub transactions: VariableList<Transaction, typenum::U1048576>,
    pub withdrawals: VariableList<Withdrawal, typenum::U16>,
    pub blob_gas_used: u64,
    pub excess_blob_gas: u64,
}

pub type Transaction = VariableList<u8, typenum::U1073741824>;
pub type LogsBloom = FixedVector<u8, typenum::U256>;
pub type ExtraData = VariableList<u8, typenum::U32>;

#[derive(Clone, Debug, Encode, Decode, RlpEncodable)]
pub struct Withdrawal {
    index: u64,
    validator_index: u64,
    address: Address,
    amount: u64,
}