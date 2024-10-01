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
            function accountLiquidityOf(address user) external view returns (uint256, uint256, uint256);
            
        }

        interface IERC20 {
            function balanceOf(address account) external view returns (uint256);
        }

        interface IUserLiquidity {
            function set(address user, bytes calldata seal) external;
        }

        struct Journal {
            SolCommitment commitment;
            uint256 liquidity;
            address user;
            uint256 chain_id;
            address comptroller;
        }

        struct BalanceJournal {
            SolCommitment commitment;
            uint256 balance;
            address user;
            address asset;
        }
    }

    const COMPTROLLER_MAIN: Address = address!("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd3B");
    const COMPTROLLER_LINEA: Address = address!("43Eac5BFEa14531B8DE0B334E123eA98325de866");
    const COMPTROLLER_SCROLL: Address = address!("EC53c830f4444a8A56455c6836b5D2aA794289Aa");
    const WETH_LINEA: Address = address!("e5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f");
    const WETH_ARBITRUM: Address = address!("82aF49447D8a07e3bd95BD0d56f35241523fBab1");
    const WETH_OPTIMISM: Address = address!("4200000000000000000000000000000000000006");
    const WETH_BASE: Address = address!("4200000000000000000000000000000000000006");
    const WETH_SCROLL: Address = address!("5300000000000000000000000000000000000004");

    const RPC_URL_LINEA: &str = "https://linea-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";
    const RPC_URL_SCROLL: &str = "https://scroll-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";
    const RPC_URL_MAINNET: &str = "https://eth-mainnet.g.alchemy.com/v2/scFv-881VOeTp7qHT88HEZ_EmsJqrGQ0";
    const RPC_URL_BASE: &str = "https://base-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";
    const RPC_URL_OPTIMISM: &str = "https://opt-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";
    const RPC_URL_ARBITRUM: &str = "https://arb-mainnet.g.alchemy.com/v2/vmrjfc4W2PsqVyDmvEHsZeNAQpRI5icv";


    #[test]
    fn proves_when_liquidity_is_non_zero_linea() {
        // choose random user with positive liquidity from etherscan
        let chain_url = RPC_URL_LINEA;
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let block = 10144608; // we fix this in case account removes liquidity
        let expected_liquidity = U256::from::<u128>(2597964494811397427); // liquidity of account at given block

        let session_info = get_users_liquidity_at_block_and_chain_url(user, block, chain_url, COMPTROLLER_LINEA);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        println!("{:?}", &session_info.journal.bytes);
        let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.liquidity, expected_liquidity);
    }

    #[test]
    fn proves_when_liquidity_is_zero_linea() {
        // choose random user with positive liquidity from etherscan
        let chain_url = RPC_URL_LINEA;
        let user = address!("0A047Ec8c33c7E8e9845662F127A5A32c0730190");
        let block = 10144608; // we fix this in case account removes liquidity
        let expected_liquidity = U256::from::<u128>(0); // liquidity of account at given block

        let session_info = get_users_liquidity_at_block_and_chain_url(user, block, chain_url, COMPTROLLER_LINEA);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        println!("{:?}", &session_info.journal.bytes);
        let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.liquidity, expected_liquidity);
    }

    // #[test]
    // fn proves_when_liquidity_is_non_zero_scroll() {
    //     // choose random user with positive liquidity from etherscan
    //     let chain_url = RPC_URL_SCROLL;
    //     let user = address!("55b82EEB5d2AFDC77dae6362ac0251C6A2bDfA4D");
    //     let block = 9750462; // we fix this in case account removes liquidity
    //     let expected_liquidity = U256::from::<u128>(546838304739424099106); // liquidity of account at given block

    //     let session_info = get_users_liquidity_at_block_and_chain_url(user, block, chain_url, COMPTROLLER_SCROLL);

    //     println!("Session info: {:?}", &session_info);

    //     let session_info = session_info.unwrap();

    //     println!("{:?}", &session_info.journal.bytes);
    //     let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
    //     assert_eq!(journal.liquidity, expected_liquidity);
    // }

    // #[test]
    // fn proves_when_liquidity_is_zero_scroll() {
    //     // choose random user with positive liquidity from etherscan
    //     let chain_url = RPC_URL_SCROLL;
    //     let user = address!("57fEe8983600aCc4d551D5383fCCf37D1131a72B");
    //     let block = 9750462; // we fix this in case account removes liquidity
    //     let expected_liquidity = U256::from::<u128>(0); // liquidity of account at given block

    //     let session_info = get_users_liquidity_at_block_and_chain_url(user, block, chain_url, COMPTROLLER_SCROLL);

    //     println!("Session info: {:?}", &session_info);

    //     let session_info = session_info.unwrap();

    //     println!("{:?}", &session_info.journal.bytes);
    //     let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
    //     assert_eq!(journal.liquidity, expected_liquidity);
    // }

    // #[test]
    // fn proves_when_liquidity_is_non_zero() {
    //     // choose random user with positive liquidity from etherscan
    //     let chain_url = RPC_URL_MAINNET;
    //     let user = address!("a66d568cD146C01ac44034A01272C69C2d9e4BaB");
    //     let block = 20770922; // we fix this in case account removes liquidity
    //     let expected_liquidity = U256::from::<u128>(16853630641732729601194); // liquidity of account at given block

    //     let session_info = get_users_liquidity_at_block_and_chain_url(user, block, chain_url, COMPTROLLER_MAIN);

    //     println!("Session info: {:?}", &session_info);

    //     let session_info = session_info.unwrap();

    //     println!("{:?}", &session_info.journal.bytes);
    //     let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
    //     assert_eq!(journal.liquidity, expected_liquidity);
    // }

    // #[test]
    // fn proves_when_liquidity_is_zero() {
    //     // address to have zero liquidity
    //     let chain_url = RPC_URL_MAINNET;
    //     let user = address!("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd2B");
    //     let block = 20770922; // we fix this in case account removes liquidity
    //     let expected_liquidity = U256::from::<u128>(0); // liquidity of account at given block

    //     let session_info = get_users_liquidity_at_block_and_chain_url(user, block, chain_url, COMPTROLLER_MAIN);

    //     println!("Session info: {:?}", &session_info);

    //     let session_info = session_info.unwrap();

    //     println!("{:?}", &session_info.journal.bytes);
    //     let journal = Journal::abi_decode(&session_info.journal.bytes, true).unwrap();
    //     assert_eq!(journal.liquidity, expected_liquidity);
    // }

    // helper function to reuse in both tests
    fn get_users_liquidity_at_block_and_chain_url(user: Address, block: u64, chain_url: &str, comptroller: Address) -> Result<SessionInfo, Error> {
        println!("User: {}", user);

        let mut env = EthEvmEnv::from_rpc(
            chain_url,
            Some(block), // we fix this in case account removes liquidity
        )
        .unwrap();

        let block_number = env.header().number();
        println!("block_number: {}", block_number);

        let call = ICompound::accountLiquidityOfCall { user };

        let mut contract = Contract::preflight(comptroller, &mut env);
        let returns = contract.call_builder(&call).call().unwrap();

        println!(
            "For block {} calling `{}` on {} returns: {}",
            env.header().number(),
            ICompound::accountLiquidityOfCall::SIGNATURE,
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
            .write(&comptroller)
            .unwrap()
            .build()
            .unwrap();

        println!("Env type ID: {:?}", &env.type_id());

        // NOTE: Use the executor to run tests without proving.
        default_executor().execute(env, super::CHECK_LIQUIDITY_CROSSCHAIN_ELF)
    }

    #[test]
    fn proves_balance_on_linea() {
        // choose random user with positive liquidity from etherscan
        let chain_url = RPC_URL_LINEA;
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let block = 10144608; // we fix this in case account removes liquidity
        let expected_balance = U256::from::<u128>(0); // balance of account at given block

        let session_info = get_users_balance_at_block_and_chain_url(user, block, chain_url, WETH_LINEA);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        let journal = BalanceJournal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.balance, expected_balance);
    }

    #[test]
    fn proves_balance_on_scroll() {
        // choose random user with positive liquidity from etherscan
        let chain_url = RPC_URL_SCROLL;
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let block = 9768100; // we fix this in case account removes liquidity
        let expected_balance = U256::from::<u128>(0); // balance of account at given block

        let session_info = get_users_balance_at_block_and_chain_url(user, block, chain_url, WETH_SCROLL);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        let journal = BalanceJournal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.balance, expected_balance);
    }

    #[test]
    fn proves_balance_on_base() {
        // choose random user with positive liquidity from etherscan
        let chain_url = RPC_URL_BASE;
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let block = 20488476; // we fix this in case account removes liquidity
        let expected_balance = U256::from::<u128>(0); // balance of account at given block

        let session_info = get_users_balance_at_block_and_chain_url(user, block, chain_url, WETH_BASE);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();


        let journal = BalanceJournal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.balance, expected_balance);
    }

    #[test]
    fn proves_balance_on_optimism() {
        // choose random user with positive liquidity from etherscan
        let chain_url = RPC_URL_OPTIMISM;
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let block = 126083815; // we fix this in case account removes liquidity
        let expected_balance = U256::from::<u128>(0); // balance of account at given block

        let session_info = get_users_balance_at_block_and_chain_url(user, block, chain_url, WETH_OPTIMISM);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        let journal = BalanceJournal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.balance, expected_balance);
    }

    #[test]
    fn proves_balance_on_arbitrum() {
        // choose random user with positive liquidity from etherscan
        let chain_url = RPC_URL_ARBITRUM;
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let block = 259187737; // we fix this in case account removes liquidity
        let expected_balance = U256::from::<u128>(0); // balance of account at given block

        let session_info = get_users_balance_at_block_and_chain_url(user, block, chain_url, WETH_ARBITRUM);

        println!("Session info: {:?}", &session_info);

        let session_info = session_info.unwrap();

        let journal = BalanceJournal::abi_decode(&session_info.journal.bytes, true).unwrap();
        assert_eq!(journal.balance, expected_balance);
    }

        // helper function to reuse in both tests
    fn get_users_balance_at_block_and_chain_url(user: Address, block: u64, chain_url: &str, asset: Address) -> Result<SessionInfo, Error> {
            println!("User: {}", user);
    
            let mut env = EthEvmEnv::from_rpc(
                chain_url,
                Some(block), // we fix this in case account removes liquidity
            )
            .unwrap();
    
            let block_number = env.header().number();
            println!("block_number: {}", block_number);
    
            let call = IERC20::balanceOfCall { account: user };
    
            let mut contract = Contract::preflight(asset, &mut env);
            let returns = contract.call_builder(&call).call().unwrap();
    
            println!(
                "For block {} calling `{}` on {} returns: {}",
                env.header().number(),
                IERC20::balanceOfCall::SIGNATURE,
                asset,
                returns._0
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
                .write(&asset)
                .unwrap()
                .build()
                .unwrap();
    
            println!("Env type ID: {:?}", &env.type_id());
    
            // NOTE: Use the executor to run tests without proving.
            default_executor().execute(env, super::BALANCE_OF_ELF)
        }
}
