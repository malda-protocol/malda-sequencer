use alloy::{
    primitives::{Address, TxHash, U256},
    providers::{Provider, ProviderBuilder, WsConnect},
    rpc::types::{Filter, Log},
    transports::http::reqwest::Url,
};
use eyre::{Result, WrapErr};
use futures::future::join_all;
use futures_util::StreamExt;
use malda_rs::constants::*;
use sequencer::constants::*;
use sequencer::events::{parse_batch_process_success_event, BATCH_PROCESS_SUCCESS_SIG};
use sequencer::events::{parse_supplied_event, parse_withdraw_on_extension_chain_event};
use sequencer::events::{
    EXTENSION_SUPPLIED_SIG, HOST_BORROW_ON_EXTENSION_CHAIN_SIG,
    HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG, MINT_EXTERNAL_SELECTOR, REPAY_EXTERNAL_SELECTOR,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::{self, Sender};
use tokio::sync::Mutex;
use tracing::{debug, error, info};
use tracing_subscriber;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessedEvent {
    HostWithdraw {
        tx_hash: TxHash,
        sender: Address,
        dst_chain_id: u32,
        amount: U256,
        market: Address,
    },
    HostBorrow {
        tx_hash: TxHash,
        sender: Address,
        dst_chain_id: u32,
        amount: U256,
        market: Address,
    },
    ExtensionSupply {
        tx_hash: TxHash,
        from: Address,
        amount: U256,
        src_chain_id: u32,
        dst_chain_id: u32,
        market: Address,
        method_selector: String,
    },
}

async fn listen_for_events(
    ws_url: Url,
    chain_id: u64,
    market: Address,
    event_signature: String,
    events_map: Arc<Mutex<HashMap<TxHash, ProcessedEvent>>>,
    removed_events: Arc<Mutex<HashSet<TxHash>>>,
    start_block: u64,
    end_block: u64,
) -> Result<()> {
    debug!("Connecting to WebSocket at {}", ws_url);
    let ws = WsConnect::new(ws_url);
    let provider = ProviderBuilder::new()
        .on_ws(ws)
        .await
        .wrap_err("Failed to connect to WebSocket")?;

    let chunk_size = 1000;
    let mut current_block = start_block;

    while current_block < end_block {
        let chunk_end = (current_block + chunk_size).min(end_block);
        let total_blocks = end_block - start_block;
        let processed_blocks = chunk_end - start_block;
        let progress_percentage = (processed_blocks as f64 / total_blocks as f64 * 100.0) as u64;

        // info!(
        //     "Querying chain {} market {:?} for events with signature {} from block {} to {}",
        //     chain_id, market, event_signature, current_block, chunk_end
        // );

        let filter = Filter::new()
            .event(&event_signature)
            .address(market)
            .from_block(current_block)
            .to_block(chunk_end);

        match provider.get_logs(&filter).await {
            Ok(logs) => {
                info!(
                    "Found {} logs in chunk {} to {} for chain {} market {:?}",
                    logs.len(),
                    current_block,
                    chunk_end,
                    chain_id,
                    market
                );

                for log in logs {
                    let tx_hash = log.transaction_hash.expect("Log should have tx hash");

                    if event_signature == BATCH_PROCESS_SUCCESS_SIG {
                        let event = parse_batch_process_success_event(&log);
                        let mut events = removed_events.lock().await;
                        if events.insert(event.init_hash) {
                            // info!(
                            //     "Removed successfully processed tx {:?} from map, remaining events: {}",
                            //     event.init_hash,
                            //     events.len()
                            // );
                        }
                        continue;
                    }

                    let processed_event = if chain_id == LINEA_SEPOLIA_CHAIN_ID {
                        let event = parse_withdraw_on_extension_chain_event(&log);
                        ProcessedEvent::HostWithdraw {
                            tx_hash,
                            sender: event.sender,
                            dst_chain_id: event.dst_chain_id,
                            amount: event.amount,
                            market,
                        }
                    } else {
                        let event = parse_supplied_event(&log);

                        if event.linea_method_selector != MINT_EXTERNAL_SELECTOR
                            && event.linea_method_selector != REPAY_EXTERNAL_SELECTOR
                        {
                            error!("Invalid method selector: {}", event.linea_method_selector);
                            continue;
                        }

                        ProcessedEvent::ExtensionSupply {
                            tx_hash,
                            from: event.from,
                            amount: event.amount,
                            src_chain_id: event.src_chain_id,
                            dst_chain_id: event.dst_chain_id,
                            market,
                            method_selector: event.linea_method_selector,
                        }
                    };

                    let mut events = events_map.lock().await;
                    events.insert(tx_hash, processed_event);
                    // info!(
                    //     "Added event for tx {:?}, total events in map: {} (Progress: {}%)",
                    //     tx_hash,
                    //     events.len(),
                    //     progress_percentage
                    // );
                }
            }
            Err(e) => {
                error!(
                    "Failed to get logs for blocks {} to {} (Progress: {}%): {}",
                    current_block, chunk_end, progress_percentage, e
                );
            }
        }

        current_block = chunk_end + 1;
    }

    info!(
        "Completed processing all blocks from {} to {} for chain {} market {:?}",
        start_block, end_block, chain_id, market
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_file(true)
        .with_line_number(true)
        .with_thread_ids(true)
        .with_target(false)
        .init();

    let events_map: Arc<Mutex<HashMap<TxHash, ProcessedEvent>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let removed_events: Arc<Mutex<HashSet<TxHash>>> = Arc::new(Mutex::new(HashSet::new()));
    let batch_size = 10000;

    let chain_configs = vec![
        // ("Linea", WS_URL_LINEA_SEPOLIA, LINEA_SEPOLIA_CHAIN_ID, 9326672, 10697267),
        // ("Optimism", WS_URL_OPT_SEPOLIA, OPTIMISM_SEPOLIA_CHAIN_ID, 23810846, 25231118),
        // ("Ethereum", WS_URL_ETH_SEPOLIA, ETHEREUM_SEPOLIA_CHAIN_ID, 7691371, 7925098
        (
            "Linea",
            WS_URL_LINEA_SEPOLIA,
            LINEA_SEPOLIA_CHAIN_ID,
            10697267 - 10000,
            10697267,
        ),
        (
            "Optimism",
            WS_URL_OPT_SEPOLIA,
            OPTIMISM_SEPOLIA_CHAIN_ID,
            25231118 - 10000,
            25231118,
        ),
        (
            "Ethereum",
            WS_URL_ETH_SEPOLIA,
            ETHEREUM_SEPOLIA_CHAIN_ID,
            7925098 - 10000,
            7925098,
        ),
    ];

    let mut current_blocks: Vec<_> = chain_configs
        .iter()
        .map(|(_, _, _, start, _)| *start)
        .collect();
    let end_blocks: Vec<_> = chain_configs.iter().map(|(_, _, _, _, end)| *end).collect();

    while current_blocks
        .iter()
        .zip(&end_blocks)
        .any(|(&current, &end)| current < end)
    {
        let mut tasks = Vec::new();

        // Add tasks for all chains
        for (idx, &(chain_name, ws_url, chain_id, _, _)) in chain_configs.iter().enumerate() {
            if current_blocks[idx] >= end_blocks[idx] {
                continue;
            }

            let batch_end = (current_blocks[idx] + batch_size).min(end_blocks[idx]);
            let current_block = current_blocks[idx];

            // Regular event listeners
            let configs = if chain_id == LINEA_SEPOLIA_CHAIN_ID {
                vec![
                    (USDC_MOCK_MARKET_SEPOLIA, HOST_BORROW_ON_EXTENSION_CHAIN_SIG),
                    (
                        USDC_MOCK_MARKET_SEPOLIA,
                        HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG,
                    ),
                    (
                        WSTETH_MOCK_MARKET_SEPOLIA,
                        HOST_BORROW_ON_EXTENSION_CHAIN_SIG,
                    ),
                    (
                        WSTETH_MOCK_MARKET_SEPOLIA,
                        HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG,
                    ),
                ]
            } else {
                vec![
                    (USDC_MOCK_MARKET_SEPOLIA, EXTENSION_SUPPLIED_SIG),
                    (WSTETH_MOCK_MARKET_SEPOLIA, EXTENSION_SUPPLIED_SIG),
                ]
            };

            for (market, event_sig) in configs {
                let ws_url = ws_url.parse::<Url>()?;
                let events_map = Arc::clone(&events_map);
                let removed_events = Arc::clone(&removed_events);
                let event_signature = event_sig.to_string();
                let chain_name = chain_name.to_string();

                let task = tokio::spawn(async move {
                    if let Err(e) = listen_for_events(
                        ws_url,
                        chain_id,
                        market,
                        event_signature,
                        events_map,
                        removed_events,
                        current_block,
                        batch_end,
                    )
                    .await
                    {
                        error!(
                            "Event listener failed for {} chain {} market {:?}: {}",
                            chain_name, chain_id, market, e
                        );
                    }
                });
                tasks.push(task);
            }

            // Batch success event listener
            let ws_url = ws_url.parse::<Url>()?;
            let events_map = Arc::clone(&events_map);
            let removed_events = Arc::clone(&removed_events);
            let chain_name = chain_name.to_string();

            let task = tokio::spawn(async move {
                if let Err(e) = listen_for_events(
                    ws_url,
                    chain_id,
                    BATCH_SUBMITTER,
                    BATCH_PROCESS_SUCCESS_SIG.to_string(),
                    events_map,
                    removed_events,
                    current_block,
                    batch_end + 5000,
                )
                .await
                {
                    error!(
                        "Batch success event listener failed for {}: {}",
                        chain_name, e
                    );
                }
            });
            tasks.push(task);
        }

        // Wait for all tasks to complete
        join_all(tasks).await;

        // Print statistics for this batch
        let added_count = events_map.lock().await.len();
        let removed_count = removed_events.lock().await.len();
        let remaining_count = added_count - removed_count;

        info!(
            "Batch complete. Added: {}, Removed: {}, Remaining: {}",
            added_count, removed_count, remaining_count
        );

        // Update current blocks
        for (idx, current) in current_blocks.iter_mut().enumerate() {
            if *current < end_blocks[idx] {
                *current = (*current + batch_size).min(end_blocks[idx]) + 1;
            }
        }
    }

    // Final statistics
    let total_added = events_map.lock().await.len();
    let total_removed = removed_events.lock().await.len();
    let final_count = total_added - total_removed;

    info!("Final Statistics:");
    info!("Total events added: {}", total_added);
    info!("Total events removed: {}", total_removed);
    info!("Final unprocessed events: {}", final_count);
    println!("Total events added: {}", total_added);
    println!("Total events removed: {}", total_removed);
    println!("Final unprocessed events: {}", final_count);

    // Inject events in batches
    // let mut events = events_map.lock().await;
    // let total_events = events.len();
    // let batch_size = 200;
    // let mut processed = 0;

    // for chunk in events.values().collect::<Vec<_>>().chunks(batch_size) {
    //     for event in chunk {
    //         if let Err(e) = inject_event(event.clone().clone()).await {
    //             error!("Failed to inject event: {:?}", e);
    //             continue;
    //         }
    //         processed += 1;
    //         info!(
    //             "Injected event {}/{} ({:.2}%)",
    //             processed,
    //             total_events,
    //             (processed as f64 / total_events as f64) * 100.0
    //         );
    //     }

    //     info!("Waiting 20 seconds before next batch...");
    //     tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;
    // }

    // info!("Finished injecting all events");
    Ok(())
}

async fn inject_event(event: ProcessedEvent) -> Result<()> {
    let socket_path = "/tmp/sequencer.sock";
    let mut stream = tokio::net::UnixStream::connect(socket_path).await?;

    // Serialize and send the event
    let json = serde_json::to_string(&event)?;
    stream.write_all(json.as_bytes()).await?;
    stream.flush().await?;

    Ok(())
}
