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

use alloy_primitives::{address, Address, U256};
use alloy_sol_types::{sol, SolValue};
use risc0_steel::{
    config::ETH_MAINNET_CHAIN_SPEC, ethereum::EthEvmInput,
    Contract, SolCommitment,
};
use risc0_zkvm::guest::env;

risc0_zkvm::guest::entry!(main);

sol! {
    /// ERC-20 balance function signature.
    interface ICompound {
        function getAccountLiquidity(address account) external view returns (uint256, uint256, uint256);
    }
}

sol! {
    struct Journal {
        SolCommitment commitment;
        uint256 liquidity;
        address user;
    }
}

fn main() {
    // Read the input data for this application.
    let input: EthEvmInput = env::read();
    let account: Address = env::read();

    println!("Account: {}", account);
    let comptroller_address = address!("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd3B");

    let env = input.into_env().with_chain_spec(&ETH_MAINNET_CHAIN_SPEC);


    let comptroller = Contract::new(comptroller_address, &env);

    let call = ICompound::getAccountLiquidityCall { account };
    let returns = comptroller.call_builder(&call).call();

    // Run the computation.
    // In this case, asserting liquidity is not zero
    assert!(returns._1 > U256::from(0), "liquidity is 0");

    // Commit the journal that will be received by the application contract.
    // Journal is encoded using Solidity ABI for easy decoding in the app contract.
    let journal = Journal {
        commitment: env.block_commitment(),
        liquidity: returns._1,
        user: account
    };
    env::commit_slice(&journal.abi_encode());
}
