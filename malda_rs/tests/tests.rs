#[cfg(test)]
mod tests {

    use alloy::{
        eips::BlockNumberOrTag,
        providers::{Provider, ProviderBuilder},
        transports::http::reqwest::Url,
    };
    use alloy_primitives::{address, Address};
    use malda_rs::{constants::*, types::*, validators::*, viewcalls::*};
    use risc0_steel::{serde::RlpHeader, host::BlockNumberOrTag as BlockRisc0};

    // Arbitrary values for testing
    const USER: Address = address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3");
    const LATEST_BLOCK: BlockRisc0 = BlockRisc0::Latest;

    #[tokio::test]
    async fn test_validate_linea_env_correct_input() {
        let balance_call_input =
            get_balance_call_input(RPC_URL_LINEA, LATEST_BLOCK, USER, WETH_LINEA).await;
        let env = balance_call_input.into_env();
        validate_linea_env(env.header().inner().clone());
    }

    #[tokio::test]
    async fn test_validate_linea_env_input_of_wrong_chain_panics() {
        let balance_call_input =
            get_balance_call_input(RPC_URL_OPTIMISM, LATEST_BLOCK, USER, WETH_OPTIMISM).await;
        let env = balance_call_input.into_env();
        assert!(std::panic::catch_unwind(|| {
            validate_linea_env(env.header().inner().clone());
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_linea_env_input_manipulated_panics() {
        let balance_call_input =
            get_balance_call_input(RPC_URL_LINEA, LATEST_BLOCK, USER, WETH_LINEA).await;
        let env = balance_call_input.into_env();
        let mut header = env.header().inner().inner().clone();
        header.number = 1;
        assert!(std::panic::catch_unwind(|| {
            validate_linea_env(RlpHeader::new(header));
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_optimism_env_correct_input() {
        let (sequencer_commitment, block) =
            get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;

        let http_url: Url =
            RPC_URL_OPTIMISM
                .parse()
                .unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);
        let correct_hash = provider.get_block_by_number(BlockNumberOrTag::Number(block_number), false).await.unwrap().unwrap().header.hash;

        validate_opstack_env(OPTIMISM_CHAIN_ID, &sequencer_commitment, correct_hash);
    }

    #[tokio::test]
    async fn test_validate_optimism_env_wrong_hash_panics() {
        let (sequencer_commitment, block) =
            get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;

        let http_url: Url =
            RPC_URL_OPTIMISM
                .parse()
                .unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let wrong_hash = provider.get_block_by_number(BlockNumberOrTag::Number(block_number - 1), false).await.unwrap().unwrap().header.hash;

        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(OPTIMISM_CHAIN_ID, &sequencer_commitment, wrong_hash);
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_optimism_env_wrong_chain_id_panics() {
        let (sequencer_commitment, block) =
            get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;

        let http_url: Url =
            RPC_URL_OPTIMISM
                .parse()
                .unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let correct_hash = provider.get_block_by_number(BlockNumberOrTag::Number(block_number), false).await.unwrap().unwrap().header.hash;

        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(OPTIMISM_CHAIN_ID + 1, &sequencer_commitment, correct_hash);
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_optimism_env_wrong_commitment_panics() {

        // get commitment from base chain here
        let (sequencer_commitment, block) =
            get_current_sequencer_commitment(BASE_CHAIN_ID).await;

        let http_url: Url =
            RPC_URL_OPTIMISM
                .parse()
                .unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let correct_hash = provider.get_block_by_number(BlockNumberOrTag::Number(block_number), false).await.unwrap().unwrap().header.hash;

        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(OPTIMISM_CHAIN_ID, &sequencer_commitment, correct_hash);
        })
        .is_err());
    }


    #[tokio::test]
    async fn test_validate_optimism_env_manipulated_commitment_panics() {

        let (sequencer_commitment, block) =
            get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;


        let (wrong_sequencer_commitment, block) =
        get_current_sequencer_commitment(BASE_CHAIN_ID).await;

        let mut manipulated_commitment_signature = sequencer_commitment.clone();
        manipulated_commitment_signature.signature = wrong_sequencer_commitment.signature;

        let mut manipulated_commitment_data = sequencer_commitment.clone();
        manipulated_commitment_data.data = wrong_sequencer_commitment.data;

        let http_url: Url =
            RPC_URL_OPTIMISM
                .parse()
                .unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let correct_hash = provider.get_block_by_number(BlockNumberOrTag::Number(block_number), false).await.unwrap().unwrap().header.hash;

        // fails when either signature or data has been modified
        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(OPTIMISM_CHAIN_ID, &manipulated_commitment_signature, correct_hash);
        })
        .is_err());

        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(OPTIMISM_CHAIN_ID, &manipulated_commitment_data, correct_hash);
        })
        .is_err());
    }

    // #[tokio::test]
    // async fn proves_balance_on_optimism() {
    //     let user_optimism = address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8");
    //     let asset = WETH_OPTIMISM;
    //     let chain_id = OPTIMISM_CHAIN_ID;

    //     let _session_info = get_user_balance_exec(user_optimism, asset, chain_id)
    //         .await
    //         .unwrap();
    // }

    // #[tokio::test]
    // async fn proves_balance_on_base() {
    //     let user_base = address!("6446021F4E396dA3df4235C62537431372195D38");
    //     let asset = WETH_BASE;
    //     let chain_id = BASE_CHAIN_ID;

    //     let _session_info = get_user_balance_exec(user_base, asset, chain_id)
    //         .await
    //         .unwrap();
    // }

    // #[tokio::test]
    // async fn proves_balance_on_ethereum_via_op() {
    //     let user_ethereum = address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E");
    //     let asset = WETH_ETHEREUM;
    //     let chain_id = ETHEREUM_CHAIN_ID;

    //     let _session_info = get_user_balance_exec(user_ethereum, asset, chain_id)
    //         .await
    //         .unwrap();
    // }

    // #[tokio::test]
    // async fn benchmark_all() {
    //     let user_linea = address!("Ad7f33984bed10518012013D4aB0458D37FEE6F3");
    //     let user_optimism = address!("e50fA9b3c56FfB159cB0FCA61F5c9D750e8128c8");
    //     let user_base = address!("6446021F4E396dA3df4235C62537431372195D38");
    //     let user_ethereum = address!("F04a5cC80B1E94C69B48f5ee68a08CD2F09A7c3E");

    //     println!("Benchmarking Linea...");
    //     let asset = WETH_LINEA;
    //     let chain_id = LINEA_CHAIN_ID;
    //     get_user_balance_prove(user_linea, asset, chain_id)
    //         .await
    //         .unwrap();

    //     println!("Benchmarking Optimism...");
    //     let asset = WETH_OPTIMISM;
    //     let chain_id = OPTIMISM_CHAIN_ID;
    //     get_user_balance_prove(user_optimism, asset, chain_id)
    //         .await
    //         .unwrap();

    //     println!("Benchmarking Base...");
    //     let asset = WETH_BASE;
    //     let chain_id = BASE_CHAIN_ID;
    //     get_user_balance_prove(user_base, asset, chain_id)
    //         .await
    //         .unwrap();

    //     println!("Benchmarking Ethereum via Optimism...");
    //     let asset = WETH_ETHEREUM;
    //     let chain_id = ETHEREUM_CHAIN_ID;
    //     get_user_balance_prove(user_ethereum, asset, chain_id)
    //         .await
    //         .unwrap();
    // }
}
