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

use guest_utils::*;
use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use risc0_steel::{ethereum::EthEvmInput, Contract, serde::RlpHeader};
use risc0_zkvm::guest::env;
use alloy_consensus::Header;

fn main() {
    // Read the input data for this application.
    let input: EthEvmInput = env::read();
    let chain_id: u64 = env::read();
    let account: Address = env::read();
    let asset_address = env::read();

    let env = input.into_env();

    let erc20_contract = Contract::new(asset_address, &env);

    let call = IERC20::balanceOfCall { account };
    let returns = erc20_contract.call_builder(&call).call();

    if chain_id == LINEA_CHAIN_ID {
        validate_linea_env(env.header().inner().clone());
    } else if chain_id == OPTIMISM_CHAIN_ID {
        let commitment: SequencerCommitment = env::read();
        validate_opstack_env(chain_id, &commitment, env.commitment().digest);
    } else if chain_id == ETHEREUM_CHAIN_ID {
        let commitment: SequencerCommitment = env::read();
        let input_op: EthEvmInput = env::read();
        let linking_blocks: Vec<RlpHeader<Header>> = env::read();
        validate_ethereum_env_via_opstack(commitment, env.header().seal(), input_op, linking_blocks);
    }
    

    let journal = Journal {
        balance: returns._0,
        user: account,
        asset: asset_address,
    };
    env::commit_slice(&journal.abi_encode());
}



