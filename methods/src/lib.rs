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

    use alloy_primitives::{address, Address, U256, B256};
    use alloy_sol_types::{sol, SolCall, SolValue};
    use anyhow::Error;

    use risc0_steel::{ethereum::EthEvmEnv, host::BlockNumberOrTag, serde::RlpHeader, Commitment, Contract, EvmInput};
    use risc0_zkvm::{default_executor, ExecutorEnv, SessionInfo};
    use tokio::{self, time::error::Elapsed};
    use url::Url;
    use malda_rs::*;
    use alloy_consensus::Header;

    #[tokio::test]
    async fn proves_balance_on_linea() {
        let user = address!("0A047Ec8c33c7E8e9945662F127A5A32c0730190");
        let asset = WETH_LINEA;
        let chain_id = LINEA_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);
    }

    #[tokio::test]
    async fn proves_balance_on_optimism() {

        let user = address!("C779b1c9B74948623B6048508aB2F1c9b9370791");
        let asset = WETH_OPTIMISM;
        let chain_id = OPTIMISM_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);

    }

    #[tokio::test]
    async fn proves_balance_on_base() {

        let user = address!("C779b1c9B74948623B6048508aB2F1c9b9370791");
        let asset = WETH_BASE;
        let chain_id = BASE_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);

    }

    #[tokio::test]
    async fn proves_balance_on_ethereum_via_op() {

        let user = address!("C779b1c9B74948623B6048508aB2F1c9b9370791");
        let asset = WETH_ETHEREUM;
        let chain_id = ETHEREUM_CHAIN_ID;

        let session_info =
        get_user_balance(user, asset, chain_id)
            .await
            .unwrap();

        let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
        println!("MCycles count: {:?}", mcycles_count);

    }


    async fn get_user_balance(
        user: Address,
        asset: Address,
        chain_id: u64,
    ) -> Result<SessionInfo, Error> {

        let rpc_url = match chain_id {
            BASE_CHAIN_ID => RPC_URL_BASE,
            OPTIMISM_CHAIN_ID => RPC_URL_OPTIMISM,
            LINEA_CHAIN_ID => RPC_URL_LINEA,
            ETHEREUM_CHAIN_ID => RPC_URL_ETHEREUM,
            _ => panic!("Invalid chain ID"),
        };

        let (block, commitment) = if chain_id == OPTIMISM_CHAIN_ID || chain_id == BASE_CHAIN_ID || chain_id == ETHEREUM_CHAIN_ID {
            let (commitment, block) = get_current_sequencer_commitment(chain_id).await;
            (Some(block), Some(commitment))
        } else {
            (None, None)
        };

        

        let (l1_block_call_input, linking_blocks, ethereum_block) = if chain_id == ETHEREUM_CHAIN_ID {
            let (l1_block_call_input, l1_block) = get_l1block_call_input(block.unwrap(), OPTIMISM_CHAIN_ID).await;
            let (linking_blocks, ethereum_block) = get_linking_blocks_ethereum(l1_block).await;
            (
                Some(l1_block_call_input),
                Some(linking_blocks),
                Some(ethereum_block)
            )
        } else {
            (None, None, None)
        };

        let block = match chain_id {
            BASE_CHAIN_ID => block.unwrap(),
            OPTIMISM_CHAIN_ID => block.unwrap(),
            LINEA_CHAIN_ID => BlockNumberOrTag::Latest,
            ETHEREUM_CHAIN_ID => BlockNumberOrTag::Number(ethereum_block.unwrap()),
            _ => panic!("Invalid chain ID"),
        };

        let balance_call_input = get_balance_call_input(rpc_url, block, user, asset).await;


        let mut env_builder = ExecutorEnv::builder();
            env_builder
            .write(&balance_call_input)
            .unwrap()
            .write(&chain_id)
            .unwrap()
            .write(&user)
            .unwrap()
            .write(&asset)
            .unwrap();

        if let Some(commitment) = commitment {
            env_builder.write(&commitment)
            .unwrap();
            };

        if let Some(l1_block_input) = l1_block_call_input {
            env_builder.write(&l1_block_input)
            .unwrap();
            };

        if let Some(linking_blocks) = linking_blocks {
            env_builder.write(&linking_blocks)
            .unwrap();
            };

        
        let env = env_builder.build().unwrap();

        // NOTE: Use the executor to run tests without proving.
        default_executor().execute(env, super::BALANCE_OF_ELF)
    }

    async fn get_balance_call_input(chain_url: &str, block: BlockNumberOrTag, user: Address, asset: Address) -> EvmInput<RlpHeader<Header>> {
        let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(chain_url).unwrap())
        .block_number_or_tag(block)
        .build()
        .await
        .unwrap();

        let call = IERC20::balanceOfCall { account: user };

        let mut contract = Contract::preflight(asset, &mut env);
        let _returns = contract.call_builder(&call).call().await.unwrap();

        env.into_input().await.unwrap()
    }

    async fn get_current_sequencer_commitment(chain_id: u64) -> (SequencerCommitment, BlockNumberOrTag) {
        let req = match chain_id {
            BASE_CHAIN_ID => {
                SEQUENCER_REQUEST_BASE
            }
            OPTIMISM_CHAIN_ID => {
                SEQUENCER_REQUEST_OPTIMISM
            }
            ETHEREUM_CHAIN_ID => {
                SEQUENCER_REQUEST_OPTIMISM
            }
            _ => {
                panic!("Invalid chain ID");
            }
        };
        let commitment = reqwest::get(req)
        .await.unwrap()
        .json::<SequencerCommitment>()
        .await.unwrap();

        let block = ExecutionPayload::try_from(&commitment).unwrap().block_number;

        (commitment, BlockNumberOrTag::Number(block))
    }

    async fn get_l1block_call_input(block: BlockNumberOrTag, chain_id: u64) -> (EvmInput<RlpHeader<Header>>, u64) {
        let rpc_url = match chain_id {
            BASE_CHAIN_ID => {
                RPC_URL_BASE
            }
            OPTIMISM_CHAIN_ID => {
                RPC_URL_OPTIMISM
            }
            _ => {
                panic!("Invalid chain ID");
            }
        };
        let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(rpc_url).unwrap())
        .block_number_or_tag(block)
        .build()
        .await
        .unwrap();

        let call = IL1Block::hashCall { };
        let mut contract = Contract::preflight(L1_BLOCK_ADDRESS_OPTIMISM, &mut env);
        let _l1_block_hash = contract.call_builder(&call).call().await.unwrap()._0;
        let view_call_input_l1_block = env.into_input().await.unwrap();

        let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(rpc_url).unwrap())
        .block_number_or_tag(block)
        .build()
        .await
        .unwrap();

        let call = IL1Block::numberCall { };
        let mut contract = Contract::preflight(L1_BLOCK_ADDRESS_OPTIMISM, &mut env);
        let l1_block = contract.call_builder(&call).call().await.unwrap()._0;

        (view_call_input_l1_block, l1_block)

    }

    async fn get_linking_blocks_ethereum(current_block: u64) -> (Vec<RlpHeader<Header>>, u64) {

        let mut linking_blocks = vec![];

        let start_block = current_block - REORG_PROTECTION_DEPTH + 1;

        for block_nr in (start_block)..=(current_block) {
            let env = EthEvmEnv::builder()
            .rpc(Url::parse(RPC_URL_ETHEREUM).unwrap())
            .block_number_or_tag(BlockNumberOrTag::Number(block_nr))
            .build()
            .await
            .unwrap();
            let header = env.header().inner().clone();
            linking_blocks.push(header);
        }
        (linking_blocks, start_block - 1)
    }


}
