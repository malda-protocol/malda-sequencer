// Copyright 2023 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use alloy_primitives::{address, Address, U256, Signature, B256, keccak256};
use alloy_sol_types::{sol, SolValue};
use risc0_steel::{
    ethereum::{ETH_MAINNET_CHAIN_SPEC, EthEvmInput},
    Contract, SolCommitment,
};
use risc0_zkvm::guest::env;

use k256::ecdsa::{VerifyingKey, Error, RecoveryId};

use std::collections::HashMap;


sol! {
    /// ERC-20 balance function signature.
    interface ICompound {
        function accountLiquidityOf(address account) external view returns (uint256, uint256, uint256);
    }
}

sol! {
    struct Journal {
        SolCommitment commitment;
        uint256 liquidity;
        address user;
        uint256 chain_id;
        address comptroller;
    }
}

const SECP256K1N_HALF: U256 = U256::from_be_bytes([
    0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
    0x5D, 0x57, 0x6E, 0x73, 0x57, 0xA4, 0x50, 0x1D, 0xDF, 0xE9, 0x2F, 0x46, 0x68, 0x1B, 0x20, 0xA0,
]);


// this currently only works for Linea, other chains will panic on signature extraction from extra_data. Scroll will panic on call proof 
// due to different hash structure
fn main() {

    // Read the input data for this application.
    let input: EthEvmInput = env::read();
    let account: Address = env::read();
    let comptroller_address = env::read();

    let env = input.into_env().with_chain_spec(&ETH_MAINNET_CHAIN_SPEC);

    let comptroller = Contract::new(comptroller_address, &env);

    let call = ICompound::accountLiquidityOfCall { account };
    let returns = comptroller.call_builder(&call).call();
    let chain_id = check_block_validity_and_get_chain_id(env.header().clone());

    // Commit the journal that will be received by the application contract.
    // Journal is encoded using Solidity ABI for easy decoding in the app contract.
    let journal = Journal {
        commitment: env.commitment().clone(),
        liquidity: returns._1,
        user: account,
        chain_id: chain_id,
        comptroller: comptroller_address
    };
    env::commit_slice(&journal.abi_encode());
}


fn check_block_validity_and_get_chain_id(header: risc0_steel::ethereum::EthBlockHeader) -> U256 {
    
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

    let mut sequencer_to_chain_id = HashMap::<Address, u32>::new();
    sequencer_to_chain_id.insert(address!("b4b473b9de9fd8916d6de76b23ebe8895f8e5c80"), 534352); // Scroll
    sequencer_to_chain_id.insert(address!("8f81e2e3f8b46467523463835f965ffe476e1c9e"), 59144); // Linea

    U256::from(sequencer_to_chain_id.get(&sequencer).cloned().expect("Sequencer not found in chain id map - chain not supported"))
    

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

    /// Converts a public key into an ethereum address by hashing the encoded public key with
    /// keccak256.
    pub fn public_key_to_address(public: VerifyingKey) -> Address {
        let hash = keccak256(&public.to_encoded_point(/* compress = */ false).as_bytes()[1..]);
        Address::from_slice(&hash[12..])
    }