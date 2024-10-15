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

// This application demonstrates how to send an off-chain proof request
// to the Bonsai proving service and publish the received proofs directly
// to your deployed app contract.

use alloy_primitives::Address;
use alloy_sol_types::{sol, SolCall};
use anyhow::Result;
use clap::Parser;
use host::TxSender;
use methods::CHECK_LIQUIDITY_ELF;
use risc0_ethereum_contracts::groth16::encode;
use risc0_steel::host::BlockNumberOrTag;
use risc0_steel::{ethereum::EthEvmEnv, ethereum::ETH_MAINNET_CHAIN_SPEC, Contract};
use risc0_zkvm::{default_prover, ExecutorEnv, ProverOpts, VerifierContext};
use tokio;
use tracing_subscriber::EnvFilter;
use url::Url;

// `IEvenNumber` interface automatically generated via the alloy `sol!` macro.
sol! {
    interface ICompound {
        function getAccountLiquidity(address user) external view returns (uint256, uint256, uint256);
    }

    interface IUserLiquidity {
        function set(address user, bytes calldata seal) external;
    }
}

/// Arguments of the publisher CLI.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Ethereum chain ID
    #[clap(long)]
    chain_id: u64,

    /// Ethereum Node endpoint.
    #[clap(long, env)]
    eth_wallet_private_key: String,

    /// Ethereum Node endpoint.
    #[clap(long)]
    rpc_url: Url,

    /// Application's contract address on Ethereum
    #[clap(long)]
    contract: Address,

    /// The input to provide to the guest binary
    #[clap(long)]
    account: Address,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing. In order to view logs, run `RUST_LOG=info cargo run`
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    // parse the command line arguments
    let args = Args::parse();

    // Create an EVM environment from an RPC endpoint and a block number. If no block number is
    // provided, the latest block is used.
    let mut env = EthEvmEnv::builder().rpc(args.rpc_url).build().await?;

    //  The `with_chain_spec` method is used to specify the chain configuration.
    env = env.with_chain_spec(&ETH_MAINNET_CHAIN_SPEC);

    let call = ICompound::getAccountLiquidityCall { user: args.account };

    // Preflight the call to execute the function in the guest.
    let mut contract = Contract::preflight(args.contract, &mut env);
    let returns = contract.call_builder(&call).call().await?;
    println!(
        "For block {} calling `{}` on {} returns: {}",
        env.header().inner().number,
        ICompound::getAccountLiquidityCall::SIGNATURE,
        args.contract,
        returns._1
    );

    println!("proving...");
    let view_call_input = env.into_input().await?;
    let env = ExecutorEnv::builder()
        .write(&view_call_input)?
        .write(&args.account)?
        .build()?;

    let receipt = default_prover()
        .prove_with_ctx(
            env,
            &VerifierContext::default(),
            CHECK_LIQUIDITY_ELF,
            &ProverOpts::groth16(),
        )?
        .receipt;
    println!("proving...done");

    // Encode the groth16 seal with the selector
    let seal = encode(receipt.inner.groth16()?.seal.clone())?;

    env_logger::init();
    // Parse CLI Arguments: The application starts by parsing command-line arguments provided by the user.
    let args = Args::parse();

    // Create a new transaction sender using the parsed arguments.
    let tx_sender = TxSender::new(
        args.chain_id,
        args.rpc_url.as_str(),
        &args.eth_wallet_private_key,
        &args.contract.to_string(),
    )?;

    // Construct function call: Using the IEvenNumber interface, the application constructs
    // the ABI-encoded function call for the set function of the EvenNumber contract.
    // This call includes the verified number, the post-state digest, and the seal (proof).
    let calldata = IUserLiquidity::setCall {
        user: args.account,
        seal: seal.into(),
    }
    .abi_encode();

    // Send the calldata to Ethereum.
    println!("sending tx...");
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(tx_sender.send(calldata))?;
    println!("sending tx...done");

    Ok(())
}
