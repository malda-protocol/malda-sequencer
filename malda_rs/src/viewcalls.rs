//! Ethereum view call utilities for cross-chain view call proof.
//!
//! This module provides functionality to:
//! - Fetch user token balances across different EVM chains
//! - Handle sequencer commitments for L2 chains
//! - Manage L1 block verification
//! - Process linking blocks for reorg protection

use anyhow::Error;

use crate::types::{ExecutionPayload, IL1Block, SequencerCommitment, IERC20};
use alloy_consensus::Header;
use risc0_steel::{
    ethereum::EthEvmEnv, host::BlockNumberOrTag, serde::RlpHeader, Contract, EvmInput,
};
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, ProveInfo, SessionInfo, sha::Digestible};
use tokio;
use url::Url;

use crate::constants::*;
use methods::{BALANCE_OF_ELF, BALANCE_OF_ID};

use std::time::Duration;

use alloy::{
    primitives::{address, utils::parse_ether, Address, Bytes},
    signers::local::PrivateKeySigner,
};
use anyhow::{bail, ensure, Result};
use boundless_market::{
    client::ClientBuilder,
    contracts::{Input, Offer, Predicate, ProofRequest, Requirements},
    storage::StorageProviderConfig,
};
use clap::Parser;


/// Timeout for the transaction to be confirmed.
pub const TX_TIMEOUT: Duration = Duration::from_secs(30);

/// Proves a user's token balance on a specified chain using the RISC Zero prover.
///
/// # Arguments
///
/// * `user` - The user's address to query
/// * `asset` - The token contract address
/// * `chain_id` - The chain identifier
///
/// # Returns
///
/// Returns a `Result` containing the `ProveInfo` or an error
pub async fn get_user_balance_prove(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> Result<ProveInfo, Error> {
    // Move all the work including env creation into the blocking task
    let prove_info = tokio::task::spawn_blocking(move || {
        // Create a new runtime for async operations within the blocking task
        let rt = tokio::runtime::Runtime::new().unwrap();

        // Execute the async env creation in the new runtime
        let env = rt.block_on(get_user_balance_zkvm_env(user, asset, chain_id));

        // Perform the proving
        default_prover().prove(env, BALANCE_OF_ELF)
    })
    .await?;

    prove_info
}

/// Arguments of the publisher CLI.
#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// The number to publish to the EvenNumber contract.
    // #[clap(short, long)]
    // number: u32,
    /// URL of the Ethereum RPC endpoint.
    #[clap(short, long, env)]
    rpc_url: Url,
    /// Private key used to interact with the EvenNumber contract.
    #[clap(short, long, env)]
    wallet_private_key: PrivateKeySigner,
    /// Submit the request offchain via the provided order stream service url.
    #[clap(short, long, requires = "order_stream_url")]
    offchain: bool,
    /// Offchain order stream service URL to submit offchain requests to.
    #[clap(long, env)]
    order_stream_url: Option<Url>,
    /// Storage provider to use
    #[clap(flatten)]
    storage_config: Option<StorageProviderConfig>,
    /// Address of the EvenNumber contract.
    // #[clap(short, long, env)]
    // even_number_address: Address,
    /// Address of the RiscZeroSetVerifier contract.
    #[clap(short, long, env)]
    set_verifier_address: Address,
    /// Address of the BoundlessfMarket contract.
    #[clap(short, long, env)]
    boundless_market_address: Address,
}

pub async fn get_user_balance_prove_boundless(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> Result<(Bytes, Bytes), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match dotenvy::dotenv() {
        Ok(path) => tracing::debug!("Loaded environment variables from {:?}", path),
        Err(e) if e.not_found() => tracing::debug!("No .env file found"),
        Err(e) => bail!("failed to load .env file: {}", e),
    }
    let args = Args::parse();

    // Create a Boundless client from the provided parameters.
    let boundless_client = ClientBuilder::default()
        .with_rpc_url(args.rpc_url)
        .with_boundless_market_address(args.boundless_market_address)
        .with_set_verifier_address(args.set_verifier_address)
        .with_order_stream_url(args.offchain.then_some(args.order_stream_url).flatten())
        .with_storage_provider_config(args.storage_config)
        .with_private_key(args.wallet_private_key)
        .build()
        .await?;

    // Upload the ELF to the storage provider so that it can be fetched by the market.
    ensure!(
        boundless_client.storage_provider.is_some(),
        "a storage provider is required to upload the zkVM guest ELF"
    );
    let image_url = boundless_client.upload_image(BALANCE_OF_ELF).await?;
    tracing::info!("Uploaded image to {}", image_url);

    // Encode the input and upload it to the storage provider.
    // tracing::info!("Number to publish: {}", args.number);
    let input = get_user_balance_zkvm_input(user, asset, chain_id).await;
    // let input = InputBuilder::new()
    //     .write_slice(&U256::from(args.number).abi_encode())
    //     .build();

    // If the input exceeds 2 kB, upload the input and provide its URL instead, as a rule of thumb.
    let request_input = if input.len() > 2 << 10 {
        let input_url = boundless_client.upload_input(&input).await?;
        tracing::info!("Uploaded input to {}", input_url);
        Input::url(input_url)
    } else {
        tracing::info!("Sending input inline with request");
        Input::inline(input.clone())
    };

    // Dry run the ELF with the input to get the journal and cycle count.
    // This can be useful to estimate the cost of the proving request.
    // It can also be useful to ensure the guest can be executed correctly and we do not send into
    // the market unprovable proving requests. If you have a different mechanism to get the expected
    // journal and set a price, you can skip this step.
    let env = ExecutorEnv::builder().write_slice(&input).build()?;
    let session_info = default_executor().execute(env, BALANCE_OF_ELF)?;
    let mcycles_count = session_info
        .segments
        .iter()
        .map(|segment| 1 << segment.po2)
        .sum::<u64>()
        .div_ceil(1_000_000);
    let journal = session_info.journal;

    // Create a proof request with the image, input, requirements and offer.
    // The ELF (i.e. image) is specified by the image URL.
    // The input can be specified by an URL, as in this example, or can be posted on chain by using
    // the `with_inline` method with the input bytes.
    // The requirements are the image ID and the digest of the journal. In this way, the market can
    // verify that the proof is correct by checking both the committed image id and digest of the
    // journal. The offer specifies the price range and the timeout for the request.
    // Additionally, the offer can also specify:
    // - the bidding start time: the block number when the bidding starts;
    // - the ramp up period: the number of blocks before the price start increasing until reaches
    //   the maxPrice, starting from the the bidding start;
    // - the lockin price: the price at which the request can be locked in by a prover, if the
    //   request is not fulfilled before the timeout, the prover can be slashed.
    let request = ProofRequest::default()
        .with_image_url(&image_url)
        .with_input(request_input)
        .with_requirements(Requirements::new(
            BALANCE_OF_ID,
            Predicate::digest_match(journal.digest()),
        ))
        .with_offer(
            Offer::default()
                // The market uses a reverse Dutch auction mechanism to match requests with provers.
                // Each request has a price range that a prover can bid on. One way to set the price
                // is to choose a desired (min and max) price per million cycles and multiply it
                // by the number of cycles. Alternatively, you can use the `with_min_price` and
                // `with_max_price` methods to set the price directly.
                .with_min_price_per_mcycle(parse_ether("0.0005")?, mcycles_count)
                // NOTE: If your offer is not being accepted, try increasing the max price.
                .with_max_price_per_mcycle(parse_ether("0.001")?, mcycles_count)
                // The timeout is the maximum number of blocks the request can stay
                // unfulfilled in the market before it expires. If a prover locks in
                // the request and does not fulfill it before the timeout, the prover can be
                // slashed.
                .with_timeout(1000),
        );

    // Send the request and wait for it to be completed.
    let (request_id, expires_at) = boundless_client.submit_request(&request).await?;
    tracing::info!("Request 0x{request_id:x} submitted");

    // Wait for the request to be fulfilled by the market, returning the journal and seal.
    tracing::info!("Waiting for 0x{request_id:x} to be fulfilled");
    let (journal, seal) = boundless_client
        .wait_for_request_fulfillment(request_id, Duration::from_secs(5), expires_at)
        .await?;
    tracing::info!("Request 0x{request_id:x} fulfilled");

    Ok((journal, seal))
}

/// Executes a user's token balance query on a specified chain using the RISC Zero executor.
///
/// # Arguments
///
/// * `user` - The user's address to query
/// * `asset` - The token contract address
/// * `chain_id` - The chain identifier
///
/// # Returns
///
/// Returns a `Result` containing the `SessionInfo` or an error
pub async fn get_user_balance_exec(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> Result<SessionInfo, Error> {
    let env = get_user_balance_zkvm_env(user, asset, chain_id).await;
    default_executor().execute(env, BALANCE_OF_ELF)
}

/// Creates a RISC Zero executor environment for token balance queries.
///
/// This is a wrapper function that creates the executor environment by getting the necessary
/// input data through `get_user_balance_zkvm_input`.
///
/// # Arguments
///
/// * `user` - The user's address to query the balance for
/// * `asset` - The token contract address to check the balance of
/// * `chain_id` - The chain identifier where the token exists
///
/// # Returns
///
/// Returns an `ExecutorEnv` configured with the serialized input data for balance verification
///
/// # Panics
///
/// Panics if the environment builder fails or if invalid input is provided
pub async fn get_user_balance_zkvm_env(
    user: Address,
    asset: Address,
    chain_id: u64,
) -> ExecutorEnv<'static> {
    let input = get_user_balance_zkvm_input(user, asset, chain_id).await;

    ExecutorEnv::builder().write_slice(&input).build().unwrap()
}

/// Prepares the input data required for token balance verification in RISC Zero.
///
/// This function handles the complex logic of gathering all necessary chain data:
/// 1. Determines the appropriate RPC URL based on the chain ID
/// 2. For L2 chains (Optimism/Base), fetches sequencer commitments
/// 3. For Ethereum mainnet/testnet, fetches L1 block information
/// 4. Gathers linking blocks for reorg protection
/// 5. Prepares the balance call input
///
/// The collected data is serialized into a byte vector containing:
/// - Balance call input
/// - Chain ID
/// - User address
/// - Asset address
/// - Sequencer commitment (if applicable)
/// - L1 block call input (if applicable)
/// - Linking blocks for reorg protection
///
/// # Arguments
///
/// * `user` - The user's address to query the balance for
/// * `asset` - The token contract address to check the balance of
/// * `chain_id` - The chain identifier where the token exists
///
/// # Returns
///
/// Returns a `Vec<u8>` containing the serialized input data
///
/// # Panics
///
/// Panics if:
/// - An invalid chain ID is provided
/// - RPC requests fail
/// - Data serialization fails
pub async fn get_user_balance_zkvm_input(user: Address, asset: Address, chain_id: u64) -> Vec<u8> {
    let rpc_url = match chain_id {
        BASE_CHAIN_ID => RPC_URL_BASE,
        OPTIMISM_CHAIN_ID => RPC_URL_OPTIMISM,
        LINEA_CHAIN_ID => RPC_URL_LINEA,
        ETHEREUM_CHAIN_ID => RPC_URL_ETHEREUM,
        OPTIMISM_SEPOLIA_CHAIN_ID => RPC_URL_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => RPC_URL_BASE_SEPOLIA,
        LINEA_SEPOLIA_CHAIN_ID => RPC_URL_LINEA_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => RPC_URL_ETHEREUM_SEPOLIA,
        _ => panic!("Invalid chain ID"),
    };

    let (block, commitment) = if chain_id == OPTIMISM_CHAIN_ID
        || chain_id == BASE_CHAIN_ID
        || chain_id == ETHEREUM_CHAIN_ID
        || chain_id == OPTIMISM_SEPOLIA_CHAIN_ID
        || chain_id == BASE_SEPOLIA_CHAIN_ID
        || chain_id == ETHEREUM_SEPOLIA_CHAIN_ID
    {
        let (commitment, block) = get_current_sequencer_commitment(chain_id).await;
        (Some(block), Some(commitment))
    } else {
        let block = EthEvmEnv::builder()
            .rpc(Url::parse(rpc_url).unwrap())
            .block_number_or_tag(BlockNumberOrTag::Latest)
            .build()
            .await
            .unwrap()
            .header()
            .inner()
            .inner()
            .number;
        (Some(block), None)
    };

    let (l1_block_call_input, ethereum_block) =
        if chain_id == ETHEREUM_CHAIN_ID || chain_id == ETHEREUM_SEPOLIA_CHAIN_ID {
            let chain_id = if chain_id == ETHEREUM_CHAIN_ID {
                OPTIMISM_CHAIN_ID
            } else {
                OPTIMISM_SEPOLIA_CHAIN_ID
            };
            let (l1_block_call_input, ethereum_block) =
                get_l1block_call_input(BlockNumberOrTag::Number(block.unwrap()), chain_id).await;

            (Some(l1_block_call_input), Some(ethereum_block))
        } else {
            (None, None)
        };

    let block = match chain_id {
        BASE_CHAIN_ID => block.unwrap(),
        OPTIMISM_CHAIN_ID => block.unwrap(),
        LINEA_CHAIN_ID => block.unwrap(),
        ETHEREUM_CHAIN_ID => ethereum_block.unwrap(),
        ETHEREUM_SEPOLIA_CHAIN_ID => ethereum_block.unwrap(),
        BASE_SEPOLIA_CHAIN_ID => block.unwrap(),
        OPTIMISM_SEPOLIA_CHAIN_ID => block.unwrap(),
        LINEA_SEPOLIA_CHAIN_ID => block.unwrap(),
        _ => panic!("Invalid chain ID"),
    };

    let linking_blocks = get_linking_blocks(chain_id, rpc_url, block).await;
    let balance_call_input = get_balance_call_input(chain_id, rpc_url, block, user, asset).await;

    let input: Vec<u8> = bytemuck::pod_collect_to_vec(
        &risc0_zkvm::serde::to_vec(&(
            &balance_call_input,
            &chain_id,
            &user,
            &asset,
            &commitment,
            &l1_block_call_input,
            &linking_blocks,
        ))
        .unwrap(),
    );

    input
}

/// Constructs an EVM input for a balance query.
///
/// # Arguments
///
/// * `chain_url` - RPC endpoint URL for the target chain
/// * `block` - Block number or tag (latest) to query
/// * `user` - Address of the user
/// * `asset` - Token contract address
///
/// # Returns
///
/// Returns an `EvmInput` containing the encoded balance call
pub async fn get_balance_call_input(
    chain_id: u64,
    chain_url: &str,
    block: u64,
    user: Address,
    asset: Address,
) -> EvmInput<RlpHeader<Header>> {
    let reorg_protection_depth = match chain_id {
        OPTIMISM_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM,
        BASE_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE,
        LINEA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA,
        ETHEREUM_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM,
        SCROLL_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL,
        OPTIMISM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE_SEPOLIA,
        LINEA_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM_SEPOLIA,
        SCROLL_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL_SEPOLIA,
        _ => panic!("invalid chain id"),
    };

    let block_reorg_protected = block - reorg_protection_depth;

    let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(chain_url).unwrap())
        .block_number_or_tag(BlockNumberOrTag::Number(block_reorg_protected))
        .build()
        .await
        .unwrap();

    let call = IERC20::balanceOfCall { account: user };

    let mut contract = Contract::preflight(asset, &mut env);
    let _returns = contract.call_builder(&call).call().await.unwrap();

    env.into_input().await.unwrap()
}

/// Fetches the current sequencer commitment for L2 chains.
///
/// # Arguments
///
/// * `chain_id` - The chain identifier (Base or Optimism)
///
/// # Returns
///
/// Returns a tuple of (SequencerCommitment, BlockNumberOrTag)
///
/// # Panics
///
/// Panics if an invalid chain ID is provided
pub async fn get_current_sequencer_commitment(chain_id: u64) -> (SequencerCommitment, u64) {
    let req = match chain_id {
        BASE_CHAIN_ID => SEQUENCER_REQUEST_BASE,
        OPTIMISM_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM,
        ETHEREUM_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM,
        OPTIMISM_SEPOLIA_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => SEQUENCER_REQUEST_BASE_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => SEQUENCER_REQUEST_OPTIMISM_SEPOLIA,
        _ => {
            panic!("Invalid chain ID");
        }
    };
    let commitment = reqwest::get(req)
        .await
        .unwrap()
        .json::<SequencerCommitment>()
        .await
        .unwrap();

    let block = ExecutionPayload::try_from(&commitment)
        .unwrap()
        .block_number;

    (commitment, block)
}

/// Retrieves L1 block information for L2 chains.
///
/// # Arguments
///
/// * `block` - Block number or tag to query
/// * `chain_id` - The chain identifier
///
/// # Returns
///
/// Returns a tuple containing the L1 block call input and block number
///
/// # Panics
///
/// Panics if an invalid chain ID is provided
pub async fn get_l1block_call_input(
    block: BlockNumberOrTag,
    chain_id: u64,
) -> (EvmInput<RlpHeader<Header>>, u64) {
    let rpc_url = match chain_id {
        BASE_CHAIN_ID => RPC_URL_BASE,
        OPTIMISM_CHAIN_ID => RPC_URL_OPTIMISM,
        BASE_SEPOLIA_CHAIN_ID => RPC_URL_BASE_SEPOLIA,
        OPTIMISM_SEPOLIA_CHAIN_ID => RPC_URL_OPTIMISM_SEPOLIA,

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

    let call = IL1Block::hashCall {};
    let mut contract = Contract::preflight(L1_BLOCK_ADDRESS_OPTIMISM, &mut env);
    let _l1_block_hash = contract.call_builder(&call).call().await.unwrap()._0;
    let view_call_input_l1_block = env.into_input().await.unwrap();

    let mut env = EthEvmEnv::builder()
        .rpc(Url::parse(rpc_url).unwrap())
        .block_number_or_tag(block)
        .build()
        .await
        .unwrap();

    let call = IL1Block::numberCall {};
    let mut contract = Contract::preflight(L1_BLOCK_ADDRESS_OPTIMISM, &mut env);
    let l1_block = contract.call_builder(&call).call().await.unwrap()._0;

    (view_call_input_l1_block, l1_block)
}

/// Fetches a sequence of Ethereum blocks for reorg protection.
///
/// # Arguments
///
/// * `current_block` - The latest block number to start from
///
/// # Returns
///
/// Returns a tuple containing:
/// - Vector of block headers for the reorg protection window
/// - The block number before the start of the window
pub async fn get_linking_blocks(
    chain_id: u64,
    rpc_url: &str,
    current_block: u64,
) -> Vec<RlpHeader<Header>> {
    let reorg_protection_depth = match chain_id {
        OPTIMISM_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM,
        BASE_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE,
        LINEA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA,
        ETHEREUM_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM,
        SCROLL_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL,
        OPTIMISM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_OPTIMISM_SEPOLIA,
        BASE_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_BASE_SEPOLIA,
        LINEA_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_LINEA_SEPOLIA,
        ETHEREUM_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_ETHEREUM_SEPOLIA,
        SCROLL_SEPOLIA_CHAIN_ID => REORG_PROTECTION_DEPTH_SCROLL_SEPOLIA,
        _ => panic!("invalid chain id"),
    };

    let mut linking_blocks = vec![];

    let start_block = current_block - reorg_protection_depth + 1;

    for block_nr in (start_block)..=(current_block) {
        let env = EthEvmEnv::builder()
            .rpc(Url::parse(rpc_url).unwrap())
            .block_number_or_tag(BlockNumberOrTag::Number(block_nr))
            .build()
            .await
            .unwrap();
        let header = env.header().inner().clone();
        linking_blocks.push(header);
    }
    linking_blocks
}
