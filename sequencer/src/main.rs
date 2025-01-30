use alloy::{
    primitives::{Address, U256, address, Bytes, TxHash},
    providers::{
        fillers::{
            BlobGasFiller, CachedNonceManager, ChainIdFiller, GasFiller, JoinFill, NonceFiller,
        },
        Provider, ProviderBuilder, WsConnect, Identity, RootProvider,
    },
    rpc::types::{Filter, Log},
    transports::http::reqwest::Url,
    network::EthereumWallet,
    signers::local::PrivateKeySigner,
};

use std::time::Duration;
use eyre::Result;
use futures_util::StreamExt;
use malda_rs::{
    constants::*,
    viewcalls::{get_proof_data_prove},
};

pub mod events;
pub mod types;
pub mod constants;

use crate::{
    events::*,
    types::*,
    constants::*,
};

use risc0_ethereum_contracts::encode_seal;

use hex;

pub const TX_TIMEOUT: Duration = Duration::from_secs(30);
pub const PRIVATE_KEY_SENDER: &str = "0xbc4e6261e470a5f67ec85062c0901cb87a1c9286d1f37712ca1d16a56a81a1bf";

type ProviderType = alloy::providers::fillers::FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        alloy::providers::fillers::WalletFiller<EthereumWallet>,
    >,
    RootProvider<alloy::transports::http::Http<alloy::transports::http::Client>>,
    alloy::transports::http::Http<alloy::transports::http::Client>,
    alloy::network::Ethereum,
>;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    // Markets
    let markets = vec![
        WETH_MARKET_SEPOLIA,
        USDC_MARKET_SEPOLIA,
    ];


    // Chain configurations
    let chain_configs = vec![
        (WS_URL_LINEA_SEPOLIA, LINEA_SEPOLIA_CHAIN_ID, vec![HOST_BORROW_ON_EXTENSION_CHAIN_SIG, HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG]),
        (WS_URL_OPT_SEPOLIA, OPTIMISM_SEPOLIA_CHAIN_ID, vec![EXTENSION_SUPPLIED_SIG]),
        (WS_URL_ETH_SEPOLIA, ETHEREUM_SEPOLIA_CHAIN_ID, vec![EXTENSION_SUPPLIED_SIG]),
    ];

    // Spawn tasks for all combinations
    let mut handles = vec![];
    
    for market in markets {
        for (ws_url, chain_id, events) in chain_configs.iter() {
            for event in events {
                let handle = tokio::spawn(sequence(
                    ws_url,
                    market,
                    event,
                    *chain_id,
                ));
                handles.push(handle);
                
                // Small delay between spawns to avoid overwhelming the connection
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }
        }
    }

    // Wait for all tasks to complete
    for handle in handles {
        if let Err(e) = handle.await {
            eprintln!("Task failed: {:?}", e);
        }
    }

    Ok(())
}

async fn sequence(ws_url: &str, market: Address, event: &str, chain_id: u64) -> Result<(), eyre::Error> {
    let ws_url: Url = ws_url.parse().expect("Invalid WSS URL");
    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new().on_ws(ws).await.unwrap();

    // Contract details
    let filter = Filter::new()
        .event(event)
        .address(market);
    let sub = provider.subscribe_logs(&filter).await?;

    let mut stream = sub.into_stream();

    println!("Listening for events...");

    while let Some(log) = stream.next().await {

        tokio::spawn(async move {

            if let Err(e) = process_event(log, market, chain_id).await {
                eprintln!("Error processing event: {:?}", e);
            }
        });
    }

    Ok(())
}

async fn process_event(log: Log, market: Address, chain_id: u64) -> Result<(), Box<dyn std::error::Error>> {
    let chain_name = match chain_id {
        LINEA_SEPOLIA_CHAIN_ID => "LINEA_SEPOLIA",
        OPTIMISM_SEPOLIA_CHAIN_ID => "OPTIMISM_SEPOLIA",
        ETHEREUM_SEPOLIA_CHAIN_ID => "ETHEREUM_SEPOLIA",
        _ => "UNKNOWN_CHAIN",
    };
    let market_name = match market {
        WETH_MARKET_SEPOLIA => "WETH",
        USDC_MARKET_SEPOLIA => "USDC",
        _ => "UNKNOWN_MARKET",
    };
    println!("Received event on {} for market asset {}", chain_name, market_name);
    let start = std::time::Instant::now();
    tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
    let result = if chain_id == LINEA_SEPOLIA_CHAIN_ID {
        process_event_host(log, market).await
    } else {
        process_event_extension(log, market).await
    };
    let end = std::time::Instant::now();
    println!("Processed event {} for market {} in {:?}", chain_name, market_name, end.duration_since(start));
    result
}

async fn get_proof_data_prove_with_retry(
    users: Vec<Vec<Address>>,
    markets: Vec<Vec<Address>>,
    dst_chain_ids: Vec<Vec<u64>>,
    src_chain_ids: Vec<u64>,
    max_attempts: u32,
    delay_between_attempts: Duration,
) -> Result<(Bytes, Bytes), Box<dyn std::error::Error>> {
    let mut attempts = 0;
    let proof_info = loop {
        match get_proof_data_prove(users.clone(), markets.clone(), dst_chain_ids.clone(), src_chain_ids.clone()).await {
            Ok(proof_info) => break proof_info,
            Err(e) if attempts < max_attempts => {
                attempts += 1;
                eprintln!("Attempt {} failed: {:?}, retrying...", attempts, e);
                tokio::time::sleep(delay_between_attempts).await;
            },
            Err(e) => {
                return Err(format!("Failed to get proof data after {} attempts: {:?}", attempts, e).into());
            },
        }
    };
    
    let receipt = proof_info.receipt;
    let seal = Bytes::from(encode_seal(&receipt)?);
    let journal = Bytes::from(receipt.journal.bytes);

    println!("Journal as hex string: {:?}", hex::encode(&journal));
    println!("Seal as hex string: {:?}", hex::encode(&seal));

    Ok((journal, seal))
}

async fn create_provider(rpc_url: Url, private_key: &str) -> Result<ProviderType, Box<dyn std::error::Error>> {
    let signer: PrivateKeySigner = private_key
        .parse()
        .expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider = ProviderBuilder::new()
        .with_recommended_fillers()
        .wallet(wallet)
        .on_http(rpc_url);

    Ok(provider)
}

async fn process_event_host(log: Log, market: Address) -> Result<(), Box<dyn std::error::Error>> {
    let event = parse_withdraw_on_extension_chain_event(&log);
    println!("Event: {:?}", event);
    
    let users = vec![vec![event.sender]];
    let markets = vec![vec![market]];
    let amount = vec![event.amount];
    let dst_chain_id = event.dst_chain_id;
    let receiver = Address::ZERO;
    
    let src_chain_ids = vec![LINEA_SEPOLIA_CHAIN_ID as u64];
    let dst_chain_ids = vec![vec![dst_chain_id as u64]];

    let (journal, seal) = get_proof_data_prove_with_retry(
        users,
        markets,
        dst_chain_ids,
        src_chain_ids,
        3,
        Duration::from_secs(1)
    ).await?;
    
    let rpc_url_submission = match dst_chain_id as u64 {
        OPTIMISM_SEPOLIA_CHAIN_ID => Url::parse(RPC_URL_OPTIMISM_SEPOLIA)?,
        ETHEREUM_SEPOLIA_CHAIN_ID => Url::parse(RPC_URL_ETHEREUM_SEPOLIA)?,
        _ => return Err(format!("Unsupported destination chain ID: {}", dst_chain_id).into()),
    };

    let provider = create_provider(rpc_url_submission, PRIVATE_KEY_SENDER).await?;

    let host_market = IMaldaMarket::new(market, provider.clone());

    submit_transaction(
        &provider,
        market,
        journal,
        seal,
        amount,
        receiver,
        "outHere"
    ).await?;

    Ok(())
}

async fn process_event_extension(log: Log, market: Address) -> Result<(), Box<dyn std::error::Error>> {
    let event = parse_supplied_event(&log);
    println!("Event: {:?}", event);

    let method_selector = event.linea_method_selector.as_str();
    if method_selector != MINT_EXTERNAL_SELECTOR && method_selector != REPAY_EXTERNAL_SELECTOR {
        println!("Wrong selector specified");
        panic!();
    }
    
    let users = vec![vec![event.from]];
    let markets = vec![vec![market]];
    let amount = vec![event.amount];
    let receiver = Address::ZERO;
    let src_chain_ids = vec![event.src_chain_id as u64];
    let dst_chain_ids = vec![vec![LINEA_SEPOLIA_CHAIN_ID as u64]];

    if event.src_chain_id == ETHEREUM_SEPOLIA_CHAIN_ID as u32 {
        tokio::time::sleep(tokio::time::Duration::from_secs(72)).await;
    }

    let (journal, seal) = get_proof_data_prove_with_retry(
        users,
        markets,
        dst_chain_ids,
        src_chain_ids,
        3,
        Duration::from_secs(1)
    ).await?;
    
    let rpc_url_submission = Url::parse("https://linea-sepolia.g.alchemy.com/v2/fSI-SMz_VGgi1ZwahhztYMCV51uTaN9e")?;

    let provider = create_provider(rpc_url_submission, PRIVATE_KEY_SENDER).await?;

    let host_market = IMaldaMarket::new(market, provider.clone());

    let method = if method_selector == MINT_EXTERNAL_SELECTOR {
        "mintExternal"
    } else if method_selector == REPAY_EXTERNAL_SELECTOR {
        "repayExternal"
    } else {
        return Err("Invalid method selector".into());
    };

    submit_transaction(
        &provider,
        market,
        journal,
        seal,
        amount,
        receiver,
        method
    ).await?;

    Ok(())
}

fn parse_supplied_event(log: &alloy::rpc::types::Log) -> SuppliedEvent {
    let from = Address::from_slice(&log.topics()[1][12..]);
    
    // The non-indexed parameters are packed in the data field
    let data = log.data().data.clone();
    
    SuppliedEvent {
        from,
        acc_amount_in: U256::from_be_slice(&data[0..32]),
        acc_amount_out: U256::from_be_slice(&data[32..64]),
        amount: U256::from_be_slice(&data[64..96]),
        src_chain_id: u32::from_be_bytes(data[124..128].try_into().unwrap()),
        dst_chain_id: u32::from_be_bytes(data[156..160].try_into().unwrap()),
        linea_method_selector: hex::encode(&data[160..164]),
    }
}

fn parse_withdraw_on_extension_chain_event(log: &Log) -> WithdrawOnExtensionChainEvent {
    WithdrawOnExtensionChainEvent {
        sender: Address::from_slice(&log.topics()[1][12..]),
        // Chain ID is padded to 32 bytes, we want the last 4 bytes
        dst_chain_id: u32::from_be_bytes(log.data().data[28..32].try_into().unwrap()),
        amount: U256::from_be_slice(&log.data().data[32..64]),
    }
}

async fn submit_transaction(
    provider: &ProviderType,
    market_address: Address,
    journal: Bytes,
    seal: Bytes,
    amount: Vec<U256>,
    receiver: Address,
    method: &str,
) -> Result<TxHash, Box<dyn std::error::Error>> {
    let market = IMaldaMarket::new(market_address, provider.clone());

    let tx_hash = match method {
        "outHere" => {
            let action = market.outHere(journal.into(), seal.into(), amount, receiver)
                .from(address!("2693946791da99dA78Ac441abA6D5Ce2Bccd96D3"));
            tracing::info!("Broadcasting out here transaction");
            println!("Broadcasting out here transaction");
            let pending_tx = action.send().await?;
            tracing::info!("Sent tx {}", pending_tx.tx_hash());
            println!("Sent tx {:?}", pending_tx.tx_hash());
            pending_tx
                .with_timeout(Some(TX_TIMEOUT))
                .watch()
                .await?
        },
        "mintExternal" => {
            let action = market.mintExternal(journal.into(), seal.into(), amount, receiver)
                .from(address!("2693946791da99dA78Ac441abA6D5Ce2Bccd96D3"));
            tracing::info!("Broadcasting mint external transaction");
            println!("Broadcasting mint external transaction");
            let pending_tx = action.send().await?;
            tracing::info!("Sent tx {}", pending_tx.tx_hash());
            println!("Sent tx {:?}", pending_tx.tx_hash());
            pending_tx
                .with_timeout(Some(TX_TIMEOUT))
                .watch()
                .await?
        },
        "repayExternal" => {
            let action = market.repayExternal(journal.into(), seal.into(), amount, receiver);
            tracing::info!("Broadcasting repay external transaction");
            let pending_tx = action.send().await?;
            tracing::info!("Sent tx {}", pending_tx.tx_hash());
            pending_tx
                .with_timeout(Some(TX_TIMEOUT))
                .watch()
                .await?
        },
        _ => return Err(format!("Invalid method: {}", method).into()),
    };

    tracing::info!("Tx {:?} confirmed", tx_hash);
    Ok(tx_hash)
}

