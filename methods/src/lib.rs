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

//! Generated crate containing the image ID and ELF binary of the build guest.
include!(concat!(env!("OUT_DIR"), "/methods.rs"));

#[cfg(test)]
mod tests {
    use std::any::Any;

    use alloy_primitives::{address, Address, U256};
    use alloy_sol_types::{sol, SolCall, SolValue};
    use risc0_steel::{
        config::{ETH_MAINNET_CHAIN_SPEC, ETH_SEPOLIA_CHAIN_SPEC},
        ethereum::EthEvmEnv,
        Contract, EvmBlockHeader,
    };
    use risc0_zkvm::{default_executor, ExecutorEnv};

    sol! {
        interface ICompound {
            function getAccountLiquidity(address user) external view returns (uint256, uint256, uint256);
        }

        interface IUserLiquidity {
            function set(address user, bytes calldata seal) external;
        }
    }

    #[test]
    fn proves_when_liquidity_is_non_zero() {
        let user = address!("d8da6bf26964af9d7eed9e03e53415d37aa96045");
        let comptroller = address!("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd3B");

        println!("User: {}", user);
        let mut env = EthEvmEnv::from_rpc(
            "https://eth-mainnet.g.alchemy.com/v2/scFv-881VOeTp7qHT88HEZ_EmsJqrGQ0",
            None,
        )
        .unwrap();
        env = env.with_chain_spec(&ETH_MAINNET_CHAIN_SPEC);

        let block_number = env.header().number();
        println!("block_number: {}", block_number);

        let call = ICompound::getAccountLiquidityCall { user };

        let mut contract = Contract::preflight(comptroller, &mut env);
        let returns = contract.call_builder(&call).call().unwrap();

        println!(
            "For block {} calling `{}` on {} returns: {}",
            env.header().number(),
            ICompound::getAccountLiquidityCall::SIGNATURE,
            comptroller,
            returns._1
        );

        let view_call_input = match env.into_input() {
            Ok(input) => input,
            Err(e) => {
                println!("Failed to create input: {:?}", e);
                panic!("Unable to proceed due to previous error.");
            }
        };

        let env = ExecutorEnv::builder()
            .write(&view_call_input)
            .unwrap()
            .write(&user.abi_encode())
            .unwrap()
            .build()
            .unwrap();

        println!("Env type ID: {:?}", &env.type_id());

        // NOTE: Use the executor to run tests without proving.
        let session_info = default_executor().execute(env, super::CHECK_LIQUIDITY_ELF);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        println!("{:?}", &session_info.journal.bytes);
        let x = Address::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(x, user);
    }

    #[test]
    #[should_panic(expected = "liquidity below 0")]
    fn rejects_when_liquidity_is_zero() {
        let user_zero = address!("0000000000000000000000000000000000000000");

        let env = ExecutorEnv::builder()
            .write_slice(&user_zero.abi_encode())
            .build()
            .unwrap();

        // NOTE: Use the executor to run tests without proving.
        default_executor()
            .execute(env, super::CHECK_LIQUIDITY_ELF)
            .unwrap();
    }
}
