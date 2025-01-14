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

use rand::{thread_rng, Rng};
use std::{sync::Arc, time::Duration};
use tokio::time::Instant;
use tracing::{error, info};

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

use malda_rs::{constants::*, viewcalls::get_user_balance_prove_boundless};

#[tokio::main]
async fn main() -> Result<()> {
    let asset = WETH_LINEA;
    let chain_id = LINEA_CHAIN_ID;

    let mut handles = vec![];

    for i in 0..20 {
        let random_bytes: [u8; 20] = thread_rng().gen();
        let random_address = Address::from(random_bytes);
        let request_id = i;
        let addr = random_address;

        let handle = tokio::spawn(async move {
            let start_time = Instant::now();
            println!("Request #{} started for address: {:?}", request_id, addr);

            match get_user_balance_prove_boundless(addr, asset, chain_id).await {
                Ok(_) => {
                    let duration = start_time.elapsed();
                    println!(
                        "Request #{} completed successfully after {:?} for address: {:?}",
                        request_id, duration, addr
                    );
                }
                Err(e) => {
                    eprintln!(
                        "Request #{} failed after {:?} for address {:?}: {:?}",
                        request_id,
                        start_time.elapsed(),
                        addr,
                        e
                    );
                }
            }
        });

        handles.push(handle);
        tokio::time::sleep(Duration::from_millis(700)).await;
    }

    for handle in handles {
        if let Err(e) = handle.await {
            eprintln!("Task join error: {:?}", e);
        }
    }

    Ok(())
}
