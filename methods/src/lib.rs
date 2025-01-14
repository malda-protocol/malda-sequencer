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

    use core::panic;

    use alloy::{
        eips::BlockNumberOrTag,
        providers::{Provider, ProviderBuilder},
        transports::http::reqwest::Url,
    };
    use alloy_primitives::{address, Address, B256};
    use malda_rs::{
        constants::*,
        viewcalls::{
            get_current_sequencer_commitment, get_user_balance_batch_exec,
            get_user_balance_batch_prove, get_user_balance_exec, get_user_balance_prove,
        },
        viewcalls_ethereum_light_client::get_user_balance_exec as get_user_balance_exec_ethereum_light_client,
    };
    use std::io::Write;

    // Common ERC20 tokens on Optimism
    const ASSETS_OPTIMISM: [Address; 10] = [
        address!("4200000000000000000000000000000000000006"), // WETH
        address!("7F5c764cBc14f9669B88837ca1490cCa17c31607"), // USDC
        address!("94b008aA00579c1307B0EF2c499aD98a8ce58e58"), // USDT
        address!("DA10009cBd5D07dd0CeCc66161FC93D7c9000da1"), // DAI
        address!("4200000000000000000000000000000000000042"), // OP
        address!("B0B195aEFA3650A6908f15CdaC7D92F8a5791B0B"), // BOB
        address!("350a791Bfc2C21F9Ed5d10980Dad2e2638ffa7f6"), // LINK
        address!("8700dAec35aF8Ff88c16BdF0418774CB3D7599B4"), // SNX
        address!("99C59ACeBFEF3BBFB7129DC90D1a11DB0E91187f"), // rETH
        address!("1F32b1c2345538c0c6f582fCB022739c4A194Ebb"), // wstETH
    ];

    // Common ERC20 tokens on Base
    const ASSETS_BASE: [Address; 10] = [
        address!("4200000000000000000000000000000000000006"), // WETH
        address!("833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"), // USDC
        address!("50c5725949A6F0c72E6C4a641F24049A917DB0Cb"), // DAI
        address!("A99F6e6785Da0F5d6fB42495Fe424BCE029Eeb3E"), // USDbC
        address!("d9aAEc86B65D86f6A7B5B1b0c42FFA531710b6CA"), // USDBC
        address!("940181a94A35A4569E4529A3CDfB74e38FD98631"), // USDT
        address!("3992B27dA26848C2b19CeA6Fd25ad5568B68AB98"), // TBTC
        address!("ecAc9C5F704e954931349Da37F60E39f515c11c1"), // cbETH
        address!("b79dd08ea68a908a97220c76d19a6aa9cbde4376"), // UNI
        address!("d07379a755A8f11B57610154861D694b2A0f615a"), // COMP
    ];

    // Common ERC20 tokens on Linea
    const ASSETS_LINEA: [Address; 10] = [
        address!("e5D7C2a44FfDDf6b295A15c148167daaAf5Cf34f"), // WETH
        address!("176211869cA2b568f2A7D4EE941E073a821EE1ff"), // USDC
        address!("A219439258ca9da29E9Cc4cE5596924745e12B93"), // USDT
        address!("43E8809ea748EFf3204ee01F08872F063e44065f"), // DAI
        address!("7d43AABC515C356145049227CeE54B608342c0ad"), // BUSD
        address!("B97F21D1f2508fF5c73E7B5AF02847640B1ff75d"), // LINK
        address!("74A0EEA77e342323aA463098e959612d3Fe6E686"), // BNB
        address!("3aab2285ddcddad8edf438c1bab47e1a9d05a9b4"), // WBTC
        address!("0e076aafd86a71dceac65508daf975425c9d0cb6"), // MATIC
        address!("150b1e51738CdF0cCfe472594C62d7D6074921CA"), // UNI
    ];

    // Common ERC20 tokens on Ethereum
    const ASSETS_ETHEREUM: [Address; 10] = [
        address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"), // WETH
        address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"), // USDC
        address!("dAC17F958D2ee523a2206206994597C13D831ec7"), // USDT
        address!("6B175474E89094C44Da98b954EedeAC495271d0F"), // DAI
        address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599"), // WBTC
        address!("514910771AF9Ca656af840dff83E8264EcF986CA"), // LINK
        address!("7D1AfA7B718fb893dB30A3aBc0Cfc608AaCfeBB0"), // MATIC
        address!("1f9840a85d5aF5bf1D1762F925BDADdC4201F984"), // UNI
        address!("7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9"), // AAVE
        address!("ae7ab96520DE3A18E5e111B5EaAb095312D7fE84"), // stETH
    ];

    #[tokio::test]
    async fn test_guest_proves_balance_on_linea() {
        let user_linea = address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3");
        let asset = WETH_LINEA;
        let chain_id = LINEA_CHAIN_ID;

        let session_info = get_user_balance_exec(user_linea, asset, chain_id)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn test_sepolia_guest_proves_balance_on_linea() {
        let user_linea = address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3");
        let asset = WETH_LINEA_SEPOLIA;
        let chain_id = LINEA_SEPOLIA_CHAIN_ID;

        let session_info = get_user_balance_exec(user_linea, asset, chain_id)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn test_guest_proves_balance_on_optimism() {
        let user_optimism = address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8");
        let asset = WETH_OPTIMISM;
        let chain_id = OPTIMISM_CHAIN_ID;

        let session_info = get_user_balance_exec(user_optimism, asset, chain_id)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn test_sepolia_guest_proves_balance_on_optimism() {
        let user_optimism = address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8");
        let asset = WETH_OPTIMISM_SEPOLIA;
        let chain_id = OPTIMISM_SEPOLIA_CHAIN_ID;

        let session_info = get_user_balance_exec(user_optimism, asset, chain_id)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn test_guest_proves_balance_ba1tch() {
        // Single chain test (Linea)
        let users = vec![vec![address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3")]];
        let assets = vec![vec![WETH_LINEA]];
        let chain_ids = vec![LINEA_CHAIN_ID];

        let session_info = get_user_balance_batch_exec(users, assets, chain_ids)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("SINGLE BALANCE CALL PER CHAIN");
        println!("Linea");
        println!("Cycles: {}", cycles);

        // Test with Linea + Optimism
        let users = vec![
            vec![address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3")],
            vec![address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8")],
        ];
        let assets = vec![vec![WETH_LINEA], vec![WETH_OPTIMISM]];
        let chain_ids = vec![LINEA_CHAIN_ID, OPTIMISM_CHAIN_ID];

        let session_info = get_user_balance_batch_exec(users, assets, chain_ids)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("\nLinea + Optimism");
        println!("Cycles: {}", cycles);

        // Test with Linea + Optimism + Base
        let users = vec![
            vec![address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3")],
            vec![address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8")],
            vec![address!("6446021F4E396dA3df4235C62537431372195D38")],
        ];
        let assets = vec![vec![WETH_LINEA], vec![WETH_OPTIMISM], vec![WETH_BASE]];
        let chain_ids = vec![LINEA_CHAIN_ID, OPTIMISM_CHAIN_ID, BASE_CHAIN_ID];

        let session_info = get_user_balance_batch_exec(users, assets, chain_ids)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("\nLinea + Optimism + Base");
        println!("Cycles: {}", cycles);

        // Test with Linea + Optimism + Base + Ethereum
        let users = vec![
            vec![address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3")],
            vec![address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8")],
            vec![address!("6446021F4E396dA3df4235C62537431372195D38")],
            vec![address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E")],
        ];
        let assets = vec![
            vec![WETH_LINEA],
            vec![WETH_OPTIMISM],
            vec![WETH_BASE],
            vec![WETH_ETHEREUM],
        ];
        let chain_ids = vec![
            LINEA_CHAIN_ID,
            OPTIMISM_CHAIN_ID,
            BASE_CHAIN_ID,
            ETHEREUM_CHAIN_ID,
        ];

        let session_info = get_user_balance_batch_exec(users, assets, chain_ids)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("\nLinea + Optimism + Base + Ethereum via OP");
        println!("Cycles: {}", cycles);

        panic!();
    }

    #[tokio::test]
    async fn test_guest_proves_balance_batch_stats() {
        use rand::Rng;
        use std::time::Instant;

        let chain_ids = vec![
            LINEA_CHAIN_ID,
            OPTIMISM_CHAIN_ID,
            BASE_CHAIN_ID,
            ETHEREUM_CHAIN_ID,
        ];

        let available_assets = [
            &ASSETS_LINEA,
            &ASSETS_OPTIMISM,
            &ASSETS_BASE,
            &ASSETS_ETHEREUM,
        ];

        // Run the test 5 times
        for iteration in 0..1 {
            println!("\nIteration {}", iteration + 1);
            
            let mut users = Vec::new();
            let mut assets = Vec::new();

            // Generate random data
            let mut rng = rand::thread_rng();
            for (idx, _chain_id) in chain_ids.iter().enumerate() {
                let size = 20;

                let chain_users: Vec<Address> = (0..size)
                    .map(|_| {
                        let random_bytes: [u8; 20] = rng.gen();
                        Address::from(random_bytes)
                    })
                    .collect();

                let chain_assets: Vec<Address> = (0..size)
                    .map(|_| available_assets[idx][rng.gen_range(0..available_assets[idx].len())])
                    .collect();

                users.push(chain_users);
                assets.push(chain_assets);
            }

            let start_time = Instant::now();
            let prove_info = get_user_balance_batch_prove(users.clone(), assets.clone(), chain_ids.clone())
                .await
                .unwrap();
            let duration = start_time.elapsed();

            // Create log entry
            let mut log_entry = String::new();

            // Add metrics for each chain
            for (idx, &chain_id) in chain_ids.iter().enumerate() {
                let chain_name = match chain_id {
                    LINEA_CHAIN_ID => "linea",
                    OPTIMISM_CHAIN_ID => "optimism",
                    BASE_CHAIN_ID => "base",
                    ETHEREUM_CHAIN_ID => "ethereum",
                    _ => "unknown",
                };

                let num_users = users[idx].len();
                let num_unique_assets = assets[idx]
                    .iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len();

                log_entry.push_str(&format!("users_{} {} assets_{} {} ", 
                    chain_name, num_users,
                    chain_name, num_unique_assets));
            }

            // Add total metrics
            let total_users: usize = users.iter().map(|u| u.len()).sum();
            let total_assets: usize = assets.iter()
                .flat_map(|a| a.iter())
                .collect::<std::collections::HashSet<_>>()
                .len();

            log_entry.push_str(&format!("total_users {} total_assets {} mcycles {} duration_s {:.2}\n",
                total_users,
                total_assets,
                prove_info.stats.total_cycles / 1_000_000,
                duration.as_secs_f64()));

            // Append to file (using append instead of write)
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open("batch_logs.txt")
                .unwrap()
                .write_all(log_entry.as_bytes())
                .unwrap();
        }

        panic!();
    }

    #[tokio::test]
    async fn test_sepolia_guest_proves_balance_on_base() {
        let user_base = address!("6446021F4E396dA3df4235C62537431372195D38");
        let asset = WETH_BASE_SEPOLIA;
        let chain_id = BASE_SEPOLIA_CHAIN_ID;

        let session_info = get_user_balance_exec(user_base, asset, chain_id)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn test_guest_proves_balance_on_ethereum_via_op() {
        let user_ethereum = address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E");
        let asset = WETH_ETHEREUM;
        let chain_id = ETHEREUM_CHAIN_ID;

        let session_info = get_user_balance_exec(user_ethereum, asset, chain_id)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn test_sepolia_guest_proves_balance_on_ethereum_via_op() {
        let user_ethereum = address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E");
        let asset = WETH_ETHEREUM_SEPOLIA;
        let chain_id = ETHEREUM_SEPOLIA_CHAIN_ID;

        let session_info = get_user_balance_exec(user_ethereum, asset, chain_id)
            .await
            .unwrap();

        let cycles = session_info.segments.iter().map(|s| s.cycles).sum::<u32>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn test_guest_proves_balance_on_ethereum_via_light_client() {
        let user_ethereum = address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E");
        let asset = WETH_ETHEREUM;
        let chain_id = ETHEREUM_CHAIN_ID;

        // update this to recent available checkpoint
        let trusted_hash_bytes: [u8; 32] = [
            0xec, 0x00, 0x6a, 0x34, 0x19, 0x2a, 0x3f, 0x07, 0x2e, 0x7a, 0x50, 0x23, 0xa7, 0x5d,
            0xb3, 0xc6, 0x36, 0xf1, 0x8c, 0x48, 0xc4, 0x33, 0x51, 0xa3, 0x31, 0x10, 0xff, 0xad,
            0x85, 0xa2, 0xd4, 0x83,
        ];
        let trusted_hash = B256::from(trusted_hash_bytes);

        let session_info = get_user_balance_exec_ethereum_light_client(
            user_ethereum,
            asset,
            chain_id,
            trusted_hash,
        )
        .await
        .unwrap();

        let cycles = session_info
            .segments
            .iter()
            .map(|s| s.cycles as u64)
            .sum::<u64>();
        println!("Cycles: {}", cycles);
    }

    #[tokio::test]
    async fn benchmark_prove_all_chains() {
        let user_linea = address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3");
        let user_optimism = address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8");
        let user_base = address!("6446021F4E396dA3df4235C62537431372195D38");
        let user_ethereum = address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E");

        println!("Benchmarking with new k256 accelerator");
        println!("-------------------------------------");
        println!("Benchmarking Linea...");
        let asset = WETH_LINEA;
        let chain_id = LINEA_CHAIN_ID;

        let start_time = std::time::Instant::now();
        let prove_info = get_user_balance_prove(user_linea, asset, chain_id)
            .await
            .unwrap();
        let duration = start_time.elapsed();

        println!("MCycles: {}", prove_info.stats.total_cycles / 1000000);
        println!("e2e time: {:?}", duration);

        panic!();

        println!("Benchmarking Optimism...");
        let asset = WETH_OPTIMISM;
        let chain_id = OPTIMISM_CHAIN_ID;
        let start_time = std::time::Instant::now();
        let prove_info = get_user_balance_prove(user_optimism, asset, chain_id)
            .await
            .unwrap();
        let duration = start_time.elapsed();

        println!("MCycles: {}", prove_info.stats.total_cycles / 1000000);
        println!("e2e time: {:?}", duration);

        println!("Benchmarking Base...");
        let asset = WETH_BASE;
        let chain_id = BASE_CHAIN_ID;
        let start_time = std::time::Instant::now();
        let prove_info = get_user_balance_prove(user_base, asset, chain_id)
            .await
            .unwrap();
        let duration = start_time.elapsed();

        println!("MCycles: {}", prove_info.stats.total_cycles / 1000000);
        println!("e2e time: {:?}", duration);

        println!("Benchmarking Ethereum via Optimism...");
        let asset = WETH_ETHEREUM;
        let chain_id = ETHEREUM_CHAIN_ID;
        let start_time = std::time::Instant::now();
        let prove_info = get_user_balance_prove(user_ethereum, asset, chain_id)
            .await
            .unwrap();
        let duration = start_time.elapsed();

        println!("MCycles: {}", prove_info.stats.total_cycles / 1000000);
        println!("e2e time: {:?}", duration);

        panic!();
    }

    #[tokio::test]
    async fn benchmark_block_delay_opstack_sequencer_commitment() {
        let http_url: Url = RPC_URL_OPTIMISM.parse().unwrap();
        let provider = ProviderBuilder::new().on_http(http_url);
        let block_from_provider = provider
            .get_block_by_number(BlockNumberOrTag::Latest, false.into())
            .await
            .unwrap()
            .unwrap()
            .header
            .number;

        let (_, block_from_commitment) = get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;

        println!("OPTIMISM BLOCKCHAIN:");
        println!("Block from provider: {}", block_from_provider);
        println!("Block from commitment: {}", block_from_commitment);
        println!(
            "Sequencer lag: {}",
            block_from_provider - block_from_commitment
        );

        let http_url: Url = RPC_URL_BASE.parse().unwrap();
        let provider = ProviderBuilder::new().on_http(http_url);
        let block_from_provider = provider
            .get_block_by_number(BlockNumberOrTag::Latest, false.into())
            .await
            .unwrap()
            .unwrap()
            .header
            .number;

        let (_, block_from_commitment) = get_current_sequencer_commitment(BASE_CHAIN_ID).await;

        println!("BASE BLOCKCHAIN:");
        println!("Block from provider: {}", block_from_provider);
        println!("Block from commitment: {}", block_from_commitment);
        println!(
            "Sequencer lag: {}",
            block_from_provider - block_from_commitment
        );
    }
}
