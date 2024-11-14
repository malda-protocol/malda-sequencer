//! Types module containing core data structures and implementations for blockchain payload processing.
//! 
//! This module provides essential types and structures for handling blockchain execution payloads,
//! sequencer commitments, and related blockchain data structures.

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
    /// Interface for querying ERC-20 token balances.
    interface IERC20 {
        /// Returns the token balance of a given account.
        /// 
        /// # Arguments
        /// * `account` - The address to query the balance for
        function balanceOf(address account) external view returns (uint256);
    }

    /// Interface for accessing L1 block information.
    interface IL1Block {
        /// Returns the hash of the current L1 block.
        function hash() external view returns (bytes32);
        /// Returns the number of the current L1 block.
        function number() external view returns (uint64);
    }

    /// Represents a journal entry for tracking asset balances.
    /// 
    /// Contains information about user balances and associated assets.
    struct Journal {
        /// The balance amount
        uint256 balance;
        /// The user's address
        address account;
        /// The asset's contract address
        address asset;
    }
}

/// Represents a commitment made by a sequencer, containing signed payload data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequencerCommitment {
    /// The compressed payload data
    data: Bytes,
    /// The cryptographic signature of the commitment
    pub signature: Signature,
}

impl SequencerCommitment {
    /// Creates a new SequencerCommitment from compressed data.
    /// 
    /// # Arguments
    /// * `data` - The compressed data bytes
    /// 
    /// # Returns
    /// * `Result<Self>` - The created commitment or an error
    pub fn new(data: &[u8]) -> Result<Self> {
        let mut decoder = snap::raw::Decoder::new();
        let decompressed = decoder.decompress_vec(&data)?;

        let signature = Signature::try_from(&decompressed[..65])?;
        let data = Bytes::from(decompressed[65..].to_vec());

        Ok(SequencerCommitment { data, signature })
    }

    /// Verifies the commitment signature against a given signer and chain ID.
    /// 
    /// # Arguments
    /// * `signer` - The expected signer's address
    /// * `chain_id` - The blockchain network ID
    /// 
    /// # Returns
    /// * `Result<()>` - Ok if verification succeeds, Error otherwise
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

/// Conversion implementation from SequencerCommitment to ExecutionPayload.
impl TryFrom<&SequencerCommitment> for ExecutionPayload {
    type Error = eyre::Report;

    /// Attempts to convert a SequencerCommitment into an ExecutionPayload.
    /// 
    /// # Arguments
    /// * `value` - The SequencerCommitment to convert
    /// 
    /// # Returns
    /// * `Result<Self>` - The converted payload or an error
    fn try_from(value: &SequencerCommitment) -> Result<Self> {
        let payload_bytes = &value.data[32..];
        ExecutionPayload::from_ssz_bytes(payload_bytes).map_err(|_| eyre::eyre!("decode failed"))
    }
}

/// Represents a complete blockchain execution payload.
#[derive(Debug, Clone, Encode, Decode)]
pub struct ExecutionPayload {
    /// Hash of the parent block
    pub parent_hash: B256,
    /// Address of the fee recipient
    pub fee_recipient: Address,
    /// Root hash of the state trie
    pub state_root: B256,
    /// Root hash of the receipt trie
    pub receipts_root: B256,
    /// Bloom filter for the logs
    pub logs_bloom: LogsBloom,
    /// Previous random value used in block production
    pub prev_randao: B256,
    /// Block number
    pub block_number: u64,
    /// Maximum gas allowed in the block
    pub gas_limit: u64,
    /// Total gas used in the block
    pub gas_used: u64,
    /// Block timestamp
    pub timestamp: u64,
    /// Additional data included in the block
    pub extra_data: ExtraData,
    /// Base fee per gas unit
    pub base_fee_per_gas: U256,
    /// Hash of the current block
    pub block_hash: B256,
    /// List of transactions included in the block
    pub transactions: VariableList<Transaction, typenum::U1048576>,
    /// List of withdrawals processed in the block
    pub withdrawals: VariableList<Withdrawal, typenum::U16>,
    /// Amount of blob gas used in the block
    pub blob_gas_used: u64,
    /// Excess blob gas in the block
    pub excess_blob_gas: u64,
}

/// Type alias for a transaction, represented as a variable-length byte list
pub type Transaction = VariableList<u8, typenum::U1073741824>;
/// Type alias for a logs bloom filter, represented as a fixed-length byte vector
pub type LogsBloom = FixedVector<u8, typenum::U256>;
/// Type alias for extra data, represented as a variable-length byte list
pub type ExtraData = VariableList<u8, typenum::U32>;

/// Represents a withdrawal operation in the blockchain.
#[derive(Clone, Debug, Encode, Decode, RlpEncodable)]
pub struct Withdrawal {
    /// Sequential index of the withdrawal
    index: u64,
    /// Index of the validator processing the withdrawal
    validator_index: u64,
    /// Recipient address of the withdrawal
    address: Address,
    /// Amount being withdrawn
    amount: u64,
}