use alloy_primitives::{address, keccak256, Address, Signature, B256, U256, Bytes};
use alloy_sol_types::sol;
use k256::ecdsa::{Error, RecoveryId, VerifyingKey};


use eyre::Result;
use serde::{Deserialize, Serialize};

use alloy_rlp::RlpEncodable;
use ssz_derive::{Decode, Encode};
use ssz_types::{FixedVector, VariableList, typenum};
use ssz::Decode;



pub const SECP256K1N_HALF: U256 = U256::from_be_bytes([
    0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x5D, 0x57, 0x6E, 0x73, 0x57, 0xA4, 0x50, 0x1D, 0xDF, 0xE9, 0x2F, 0x46, 0x68, 0x1B, 0x20, 0xA0,
]);

pub const OPTIMISM_CHAIN_ID: u64 = 10;
pub const LINEA_CHAIN_ID: u64 = 59144;
pub const OPTIMISM_SEQUENCER: Address = address!("AAAA45d9549EDA09E70937013520214382Ffc4A2");
pub const LINEA_SEQUENCER: Address = address!("8f81e2e3f8b46467523463835f965ffe476e1c9e");

sol! {
    /// ERC-20 balance function signature.
    interface IERC20 {
        function balanceOf(address account) external view returns (uint256);
    }
}

sol! {
    struct Journal {
        uint256 balance;
        address user;
        address asset;
    }
}

pub fn check_block_validity_linea(header: risc0_steel::ethereum::EthBlockHeader) {
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

fn recover_signer(signature: Signature, sighash: B256) -> Option<Address> {
    if signature.s() > SECP256K1N_HALF {
        return None;
    }

    let mut sig: [u8; 65] = [0; 65];

    sig[0..32].copy_from_slice(&signature.r().to_be_bytes::<32>());
    sig[32..64].copy_from_slice(&signature.s().to_be_bytes::<32>());
    sig[64] = signature.v().y_parity_byte();

    // NOTE: we are removing error from underlying crypto library as it will restrain primitive
    // errors and we care only if recovery is passing or not.
    recover_signer_unchecked(&sig, &sighash.0).ok()
}

pub fn recover_signer_unchecked(sig: &[u8; 65], msg: &[u8; 32]) -> Result<Address, Error> {
    let mut signature = k256::ecdsa::Signature::from_slice(&sig[0..64])?;
    let mut recid = sig[64];

    // normalize signature and flip recovery id if needed.
    if let Some(sig_normalized) = signature.normalize_s() {
        signature = sig_normalized;
        recid ^= 1;
    }
    let recid = RecoveryId::from_byte(recid).expect("recovery ID is valid");

    // recover key
    let recovered_key = VerifyingKey::recover_from_prehash(&msg[..], &signature, recid)?;
    Ok(public_key_to_address(recovered_key))
}

pub fn public_key_to_address(public: VerifyingKey) -> Address {
    let hash = keccak256(&public.to_encoded_point(/* compress = */ false).as_bytes()[1..]);
    Address::from_slice(&hash[12..])
}






pub fn validate_commitment(commitment: &SequencerCommitment, env_block_hash: B256) {

    commitment.verify(OPTIMISM_SEQUENCER, OPTIMISM_CHAIN_ID).unwrap();
    let payload = ExecutionPayload::try_from(commitment).unwrap();
    assert_eq!(payload.block_hash, env_block_hash, "block hash mismatch");
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

fn signature_msg(data: &[u8], chain_id: u64) -> B256 {
    let domain = B256::ZERO;
    let chain_id = B256::left_padding_from(&chain_id.to_be_bytes());
    let payload_hash = keccak256(data);

    let signing_data = [
        domain.as_slice(),
        chain_id.as_slice(),
        payload_hash.as_slice(),
    ];

    keccak256(signing_data.concat()).into()
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