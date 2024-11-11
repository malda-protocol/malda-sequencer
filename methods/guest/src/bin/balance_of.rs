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

mod guest_utils;

use alloy_primitives::Address;
use alloy_sol_types::SolValue;
use risc0_steel::{ethereum::EthEvmInput, Contract};
use risc0_zkvm::guest::env;

use guest_utils::*;

fn main() {
    // Read the input data for this application.
    let input: EthEvmInput = env::read();
    let account: Address = env::read();
    let asset_address = env::read();

    let env = input.into_env();

    let comptroller = Contract::new(asset_address, &env);

    let call = IERC20::balanceOfCall { account };
    let returns = comptroller.call_builder(&call).call();

    check_block_validity_linea(env.header().inner().clone());

    let journal = Journal {
        balance: returns._0,
        user: account,
        asset: asset_address,
    };
    env::commit_slice(&journal.abi_encode());
}


