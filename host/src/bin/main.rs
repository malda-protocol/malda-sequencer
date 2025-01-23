use alloy::{
    primitives::{Address, U256},
    providers::{
        fillers::{
            CachedNonceManager, ChainIdFiller, GasFiller, JoinFill, NonceFiller,
        },
        Provider, ProviderBuilder, WsConnect,
    },
    rpc::types::{Filter, Log},
    transports::http::reqwest::Url,
    network::EthereumWallet,
    signers::local::PrivateKeySigner,
};

use alloy::{
    consensus::Transaction,
    node_bindings::Anvil,
    primitives::b256,
    rpc::types::{request::TransactionRequest},
};
use std::time::Duration;
use eyre::Result;
use futures_util::StreamExt;
use malda_rs::{
    constants::*,
    viewcalls::{get_proof_data_prove, get_proof_data_exec},
};

pub mod events;

use crate::events::*;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::Mutex;
use risc0_ethereum_contracts::encode_seal;

use rand;
use hex;

pub const WS_URL_ETH_SEPOLIA: &str = "wss://eth-sepolia.g.alchemy.com/v2/uGenJq8d9bfW9gXcaUZln_ZBDhS61oJY";
pub const WS_URL_OPT_SEPOLIA: &str = "wss://opt-sepolia.g.alchemy.com/v2/uGenJq8d9bfW9gXcaUZln_ZBDhS61oJY";
pub const WS_URL_LINEA_SEPOLIA: &str = "wss://linea-sepolia.g.alchemy.com/v2/uGenJq8d9bfW9gXcaUZln_ZBDhS61oJY";

pub const TX_TIMEOUT: Duration = Duration::from_secs(30);


type ProviderType = alloy::providers::fillers::FillProvider<
    JoinFill<
        JoinFill<
            alloy::providers::Identity,
            JoinFill<GasFiller, JoinFill<NonceFiller<CachedNonceManager>, ChainIdFiller>>,
        >,
        alloy::providers::fillers::WalletFiller<EthereumWallet>,
    >,
    alloy::providers::RootProvider<alloy::transports::http::Http<alloy::transports::http::Client>>,
    alloy::transports::http::Http<alloy::transports::http::Client>,
    alloy::network::Ethereum,
>;

// Chain configuration struct
struct ChainConfig {
    ws_url: String,
    is_host: bool,
    contracts: Vec<Address>,
}

alloy::sol! {
    #![sol(rpc, all_derives)]
    interface IMaldaMarket {
        function mintExternal(
            bytes calldata journalData,
            bytes calldata seal,
            uint256[] calldata amount,
            address receiver
        ) external;

        function repayExternal(
            bytes calldata journalData,
            bytes calldata seal,
            uint256[] calldata repayAmount,
            address receiver
        ) external;

        function outHere(bytes calldata journalData, bytes calldata seal, uint256[] memory amounts, address receiver)
        external;
    }
}

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

async fn process_event_host(log: Log, market: Address) -> Result<(), Box<dyn std::error::Error>> {
    let event = parse_withdraw_on_extension_chain_event(&log);

    println!("Event: {:?}", event);
    
    let users = vec![vec![event.sender]];
    let markets = vec![vec![market]];
    let amount = vec![event.amount];
    let dst_chain_id = event.dst_chain_id;
    let receiver = Address::ZERO;
    
    let chain_ids = vec![LINEA_SEPOLIA_CHAIN_ID as u64];

    let mut attempts = 0;
    let proof_info = loop {
        match get_proof_data_prove(users.clone(), markets.clone(), vec![vec![dst_chain_id as u64]], chain_ids.clone()).await {
            Ok(proof_info) => break proof_info,
            Err(e) if attempts < 3 => {
                attempts += 1;
                eprintln!("Attempt {} failed: {:?}, retrying...", attempts, e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            },
            Err(e) => {
                return Err(format!("Failed to get proof data after {} attempts: {:?}", attempts, e).into());
            },
        }
    };
    
    let receipt = proof_info.receipt;
    let seal = encode_seal(&receipt)?;
    let journal = receipt.journal.bytes.clone();

    println!("Journal as hex string: {:?}", hex::encode(&journal));
    println!("Seal as hex string: {:?}", hex::encode(&seal));

    let rpc_url_submission = match dst_chain_id as u64 {
        OPTIMISM_SEPOLIA_CHAIN_ID => Url::parse(RPC_URL_OPTIMISM_SEPOLIA)?,
        ETHEREUM_SEPOLIA_CHAIN_ID => Url::parse(RPC_URL_ETHEREUM_SEPOLIA)?,
        _ => return Err(format!("Unsupported destination chain ID: {}", dst_chain_id).into()),
    };
    let private_key_sender = "0xbc4e6261e470a5f67ec85062c0901cb87a1c9286d1f37712ca1d16a56a81a1bf";

    let signer: PrivateKeySigner = private_key_sender
        .parse()
        .expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider: ProviderType = ProviderBuilder::new()
        .filler(JoinFill::new(
            GasFiller,
            JoinFill::new(
                NonceFiller::<CachedNonceManager>::default(),
                ChainIdFiller::default(),
            ),
        ))
        .wallet(wallet)
        .on_http(rpc_url_submission);

    let host_market = IMaldaMarket::new(market, provider.clone());

    let tx_hash = {
        let out_here_action = host_market.outHere(journal.into(), seal.into(), amount, receiver);
        tracing::info!("Broadcasting out here transaction");
        let pending_tx = out_here_action.send().await?;
        tracing::info!("Sent tx {}", pending_tx.tx_hash());
        pending_tx
            .with_timeout(Some(TX_TIMEOUT))
            .watch()
            .await?
    };

    tracing::info!("Tx {:?} confirmed", tx_hash);
    Ok(())
}

async fn process_event_extension(log: Log, market: Address) -> Result<(), Box<dyn std::error::Error>> {
    let event = parse_supplied_event(&log);



    let method_selector = event.linea_method_selector.as_str();
    if method_selector != MINT_EXTERNAL_SELECTOR && method_selector != REPAY_EXTERNAL_SELECTOR {
        println!("Wrong selector specified");
        panic!();
    }
    
    let users = vec![vec![event.from]];
    let markets = vec![vec![market]];
    let amount = vec![event.amount];
    let receiver = Address::ZERO;
    
    let chain_ids = vec![event.src_chain_id as u64];

    let mut attempts = 0;
    let proof_info = loop {
        match get_proof_data_prove(users.clone(), markets.clone(), vec![vec![LINEA_SEPOLIA_CHAIN_ID as u64]], chain_ids.clone()).await {
            Ok(proof_info) => break proof_info,
            Err(e) if attempts < 3 => {
                attempts += 1;
                eprintln!("Attempt {} failed: {:?}, retrying...", attempts, e);
                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
            },
            Err(e) => {
                return Err(format!("Failed to get proof data after {} attempts: {:?}", attempts, e).into());
            },
        }
    };
    
    let receipt = proof_info.receipt;
    let seal = encode_seal(&receipt)?;
    let journal = receipt.journal.bytes.clone();

    println!("Journal as hex string: {:?}", hex::encode(&journal));
    println!("Seal as hex string: {:?}", hex::encode(&seal));

    let rpc_url_submission = Url::parse("https://linea-sepolia.g.alchemy.com/v2/fSI-SMz_VGgi1ZwahhztYMCV51uTaN9e")?;
    let private_key_sender = "0xbc4e6261e470a5f67ec85062c0901cb87a1c9286d1f37712ca1d16a56a81a1bf";

    let signer: PrivateKeySigner = private_key_sender
        .parse()
        .expect("should parse private key");
    let wallet = EthereumWallet::from(signer);

    let provider: ProviderType = ProviderBuilder::new()
        .filler(JoinFill::new(
            GasFiller,
            JoinFill::new(
                NonceFiller::<CachedNonceManager>::default(),
                ChainIdFiller::default(),
            ),
        ))
        .wallet(wallet)
        .on_http(rpc_url_submission);

    let host_market = IMaldaMarket::new(market, provider.clone());

    let tx_hash = match event.linea_method_selector.as_str() {
        MINT_EXTERNAL_SELECTOR => {
            let mint_action = host_market.mintExternal(journal.into(), seal.into(), amount, receiver);
            tracing::info!("Broadcasting mint external transaction");
            let pending_tx = mint_action.send().await?;
            tracing::info!("Sent tx {}", pending_tx.tx_hash());
            pending_tx
                .with_timeout(Some(TX_TIMEOUT))
                .watch()
                .await?
        }
        REPAY_EXTERNAL_SELECTOR => {
            let repay_action = host_market.repayExternal(journal.into(), seal.into(), amount, receiver);
            tracing::info!("Broadcasting repay external transaction");
            let pending_tx = repay_action.send().await?;
            tracing::info!("Sent tx {}", pending_tx.tx_hash());
            pending_tx
                .with_timeout(Some(TX_TIMEOUT))
                .watch()
                .await?
        }
        _ => return Err(format!("Invalid method selector: {}", event.linea_method_selector).into()),
    };

    tracing::info!("Tx {:?} confirmed", tx_hash);
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

// Event parsing functions
fn parse_liquidate_external_event(log: &Log) -> LiquidateExternalEvent {
    LiquidateExternalEvent {
        msg_sender: Address::from_slice(&log.topics()[1][12..]),
        src_sender: Address::from_slice(&log.topics()[2][12..]),
        user_to_liquidate: Address::from_slice(&log.topics()[3][12..]),
        receiver: Address::from_slice(&log.data().data[..32]),
        collateral: Address::from_slice(&log.data().data[32..64]),
        src_chain_id: u32::from_be_bytes(log.data().data[64..68].try_into().unwrap()),
        amount: U256::from_be_slice(&log.data().data[68..100]),
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

