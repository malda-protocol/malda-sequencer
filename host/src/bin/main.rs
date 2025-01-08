// Copyright 2024 RISC Zero, Inc.
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

use std::time::Duration;

use alloy::{
    primitives::{address, utils::parse_ether, Address},
    signers::local::PrivateKeySigner,
};
use anyhow::{bail, ensure, Result};
use boundless_market::{
    client::ClientBuilder,
    contracts::{Input, Offer, Predicate, ProofRequest, Requirements},
    storage::StorageProviderConfig,
};
use clap::Parser;
use methods::{BALANCE_OF_ELF, BALANCE_OF_ID};
use risc0_zkvm::{default_executor, sha::Digestible, ExecutorEnv};
use url::Url;

use malda_rs::{
    constants::*,
    viewcalls::get_user_balance_prove_boundless,
};


#[tokio::main]
async fn main() -> Result<()> {
    let user_linea = address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3");
    let asset = WETH_LINEA;
    let chain_id = LINEA_CHAIN_ID;

    let (journal, seal) = get_user_balance_prove_boundless(user_linea, asset, chain_id).await?;

    Ok(())
}
