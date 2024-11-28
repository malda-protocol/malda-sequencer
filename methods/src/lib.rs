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
    use alloy_primitives::address;
    use malda_rs::{
        constants::*,
        viewcalls::{
            get_current_sequencer_commitment, get_user_balance_exec, get_user_balance_prove,
        },
    };
    use risc0_steel::host::BlockNumberOrTag as BlockRisc0;

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
    async fn test_guest_proves_balance_on_base() {
        let user_base = address!("6446021F4E396dA3df4235C62537431372195D38");
        let asset = WETH_BASE;
        let chain_id = BASE_CHAIN_ID;

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
    async fn benchmark_prove_all_chains() {
        let user_linea = address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3");
        let user_optimism = address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8");
        let user_base = address!("6446021F4E396dA3df4235C62537431372195D38");
        let user_ethereum = address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E");

        println!("Benchmarking with k256 accelerator");
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
            .get_block_by_number(BlockNumberOrTag::Latest, false)
            .await
            .unwrap()
            .unwrap()
            .header
            .number;

        let (_, block_from_commitment) = get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;
        let block_from_commitment = match block_from_commitment {
            BlockRisc0::Number(n) => n,
            _ => panic!("Expected a block number"),
        };
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
            .get_block_by_number(BlockNumberOrTag::Latest, false)
            .await
            .unwrap()
            .unwrap()
            .header
            .number;

        let (_, block_from_commitment) = get_current_sequencer_commitment(BASE_CHAIN_ID).await;
        let block_from_commitment = match block_from_commitment {
            BlockRisc0::Number(n) => n,
            _ => panic!("Expected a block number"),
        };
        println!("BASE BLOCKCHAIN:");
        println!("Block from provider: {}", block_from_provider);
        println!("Block from commitment: {}", block_from_commitment);
        println!(
            "Sequencer lag: {}",
            block_from_provider - block_from_commitment
        );
    }
}
