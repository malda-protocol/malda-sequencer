#[cfg(test)]
mod tests {

    use alloy::{
        eips::BlockNumberOrTag,
        providers::{Provider, ProviderBuilder},
        transports::http::reqwest::Url,
    };
    use alloy_primitives::{address, Address};
    use malda_rs::{constants::*, validators::*, viewcalls::*};
    use risc0_steel::{host::BlockNumberOrTag as BlockRisc0, serde::RlpHeader};

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

        let http_url: Url = RPC_URL_OPTIMISM.parse().unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);
        let correct_hash = provider
            .get_block_by_number(BlockNumberOrTag::Number(block_number), false)
            .await
            .unwrap()
            .unwrap()
            .header
            .hash;

        validate_opstack_env(OPTIMISM_CHAIN_ID, &sequencer_commitment, correct_hash);
    }

    #[tokio::test]
    async fn test_validate_optimism_env_wrong_hash_panics() {
        let (sequencer_commitment, block) =
            get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;

        let http_url: Url = RPC_URL_OPTIMISM.parse().unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let wrong_hash = provider
            .get_block_by_number(BlockNumberOrTag::Number(block_number - 1), false)
            .await
            .unwrap()
            .unwrap()
            .header
            .hash;

        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(OPTIMISM_CHAIN_ID, &sequencer_commitment, wrong_hash);
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_optimism_env_wrong_chain_id_panics() {
        let (sequencer_commitment, block) =
            get_current_sequencer_commitment(OPTIMISM_CHAIN_ID).await;

        let http_url: Url = RPC_URL_OPTIMISM.parse().unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let correct_hash = provider
            .get_block_by_number(BlockNumberOrTag::Number(block_number), false)
            .await
            .unwrap()
            .unwrap()
            .header
            .hash;

        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(OPTIMISM_CHAIN_ID + 1, &sequencer_commitment, correct_hash);
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_optimism_env_wrong_commitment_panics() {
        // get commitment from base chain here
        let (sequencer_commitment, block) = get_current_sequencer_commitment(BASE_CHAIN_ID).await;

        let http_url: Url = RPC_URL_OPTIMISM.parse().unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let correct_hash = provider
            .get_block_by_number(BlockNumberOrTag::Number(block_number), false)
            .await
            .unwrap()
            .unwrap()
            .header
            .hash;

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

        let http_url: Url = RPC_URL_OPTIMISM.parse().unwrap();

        let block_number: u64 = match block {
            BlockRisc0::Number(n) => n,
            _ => panic!(""),
        };
        let provider = ProviderBuilder::new().on_http(http_url);

        // get hash of previous block here
        let correct_hash = provider
            .get_block_by_number(BlockNumberOrTag::Number(block_number), false)
            .await
            .unwrap()
            .unwrap()
            .header
            .hash;

        // fails when either signature or data has been modified
        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(
                OPTIMISM_CHAIN_ID,
                &manipulated_commitment_signature,
                correct_hash,
            );
        })
        .is_err());

        assert!(std::panic::catch_unwind(|| {
            validate_opstack_env(
                OPTIMISM_CHAIN_ID,
                &manipulated_commitment_data,
                correct_hash,
            );
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_chain_length_input_correct() {
        let block_number = 21193475;
        let (linking_blocks, _) = get_linking_blocks(RPC_URL_ETHEREUM, block_number).await;
        let historical_hash = linking_blocks[0].inner().parent_hash;
        let current_hash = linking_blocks[linking_blocks.len() - 1].hash_slow();
        validate_chain_length(historical_hash, linking_blocks, current_hash);
    }

    #[tokio::test]
    async fn test_validate_chain_length_panics_if_chain_too_short() {
        let block_number = 21193475;
        let (linking_blocks, _) = get_linking_blocks(RPC_URL_ETHEREUM, block_number).await;
        let historical_hash = linking_blocks[0].inner().parent_hash;
        let current_hash = linking_blocks[linking_blocks.len() - 1].hash_slow();

        assert!(std::panic::catch_unwind(|| {
            validate_chain_length(
                historical_hash,
                linking_blocks[0..linking_blocks.len() - 2].to_vec(),
                current_hash,
            );
        })
        .is_err());
    }

    #[tokio::test]
    async fn test_validate_chain_length_panics_if_hash_doesnt_match() {
        let block_number = 21193475;
        let (linking_blocks, _) = get_linking_blocks(RPC_URL_ETHEREUM, block_number).await;
        let historical_hash = linking_blocks[0].inner().parent_hash;

        assert!(std::panic::catch_unwind(|| {
            validate_chain_length(
                historical_hash,
                linking_blocks[0..linking_blocks.len() - 2].to_vec(),
                historical_hash,
            );
        })
        .is_err());
    }
}
