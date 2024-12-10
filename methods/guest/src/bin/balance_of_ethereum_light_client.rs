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


use guest_utils::{validators::validate_balance_of_call, types::SequencerCommitment, l1_validation::read_l1_chain_builder_input};
use alloy_primitives::Address;
use risc0_steel::{ethereum::EthEvmInput, serde::RlpHeader};
use risc0_zkvm::guest::env;
use alloy_consensus::Header;

// use consensus_core::types::{Header, Bootstrap, OptimisticUpdate, Update, SyncCommittee, SyncAggregate};
// use alloy_primitives_old::B256 as OldB256;
// sol! {
//     /// ERC-20 balance function signature.
//     interface ICompound {
//         function getAccountLiquidity(address account) external view returns (uint256, uint256, uint256);
//     }
// }

// sol! {
//     struct Journal {
//         Commitment commitment;
//         uint256 liquidity;
//         address user;
//     }
// }


fn main() {

    let (input, account, bootstrap, checkpoint, updates, finality_update) = read_l1_chain_builder_input();
    // let verified_root = L1ChainBuilder::new().build_beacon_chain(bootstrap, checkpoint, updates, finality_update).unwrap();

    // let comptroller_address = address!("3d9819210A31b4961b30EF54bE2aeD79B9c9Cd3B");

    // let env = input.into_env().with_chain_spec(&ETH_MAINNET_CHAIN_SPEC);

    // let comptroller = Contract::new(comptroller_address, &env);

    // let call = ICompound::getAccountLiquidityCall { account };
    // let returns = comptroller.call_builder(&call).call();

    // // assert that the commitment root is the verified root
    // assert_eq!(env.commitment().digest, verified_root);

    // // Commit the journal that will be received by the application contract.
    // // Journal is encoded using Solidity ABI for easy decoding in the app contract.
    // let journal = Journal {
    //     commitment: env.commitment().clone(),
    //     liquidity: returns._1,
    //     user: account,
    // };
    // env::commit_slice(&journal.abi_encode());

}

