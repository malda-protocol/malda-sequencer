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
    use anyhow::Error;

    use risc0_steel::{
        config::ETH_MAINNET_CHAIN_SPEC, ethereum::EthEvmEnv, Contract, EvmBlockHeader,
        SolCommitment,
    };
    use risc0_zkvm::{default_executor, ExecutorEnv, SessionInfo};

    sol! {
        interface ICompound {
            function getAccountLiquidity(address user) external view returns (uint256, uint256, uint256);
        }

        interface IUserLiquidity {
            function set(address user, bytes calldata seal) external;
        }

        struct Journal {
            SolCommitment commitment;
            uint256 liquidity;
            address user;
        }
    }

    #[test]
    fn proves_when_liquidity_is_non_zero() {
        // choose random user with positive liquidity from etherscan
        let user = address!("a66d568cD146C01ac44034A01272C69C2d9e4BaB");
        let block = 20770922; // we fix this in case account removes liquidity
        let expected_liquidity = U256::from::<u128>(16853630641732729601194); // liquidity of account at given block

        let session_info = get_users_liquidity_at_block(user, block);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        println!("{:?}", &session_info.journal.bytes);
        let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.liquidity, expected_liquidity);
    }

    #[test]
    fn proves_when_liquidity_is_zero() {
        // address to have zero liquidity
        let user = address!("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd2B");
        let block = 20770922; // we fix this in case account removes liquidity
        let expected_liquidity = U256::from::<u128>(0); // liquidity of account at given block

        let session_info = get_users_liquidity_at_block(user, block);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        println!("{:?}", &session_info.journal.bytes);
        let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.liquidity, expected_liquidity);
    }

    // helper function to reuse in both tests
    fn get_users_liquidity_at_block(user: Address, block: u64) -> Result<SessionInfo, Error> {
        println!("User: {}", user);
        let comptroller = address!("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd3B");

        let mut env = EthEvmEnv::from_rpc(
            "https://eth-mainnet.g.alchemy.com/v2/scFv-881VOeTp7qHT88HEZ_EmsJqrGQ0",
            Some(block), // we fix this in case account removes liqudidity
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
            .write(&user)
            .unwrap()
            .build()
            .unwrap();

        println!("Env type ID: {:?}", &env.type_id());

        // NOTE: Use the executor to run tests without proving.
        default_executor().execute(env, super::CHECK_LIQUIDITY_ELF)
    }
}
