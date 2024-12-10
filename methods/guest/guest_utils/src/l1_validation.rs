use consensus_core::{
    apply_bootstrap, apply_optimistic_update, apply_update, expected_current_slot,
    verify_bootstrap, verify_optimistic_update, verify_update,
};

pub use consensus_core::types::{Bootstrap, Forks, LightClientStore, OptimisticUpdate, Update};

use alloy_primitives::{b256, B256};
pub use alloy_primitives_old::{fixed_bytes as old_fixed_bytes, B256 as OldB256};
use eyre::Result;
use std::time::SystemTime;
use tree_hash::TreeHash;

use alloy_primitives::Address;
use risc0_steel::ethereum::EthEvmInput;
use risc0_zkvm::guest::env;

use consensus_core::types::{Header, SyncAggregate, SyncCommittee};

use crate::constants::*;
use crate::cryptography::{recover_signer, signature_from_bytes};
use crate::types::*;
use alloy_sol_types::SolValue;
use risc0_steel::{serde::RlpHeader, Contract};
use alloy_consensus::Header as ConsensusHeader;


#[derive(Debug)]
pub struct L1ChainBuilder {
    pub store: LightClientStore,
    pub last_checkpoint: Option<B256>,
    pub genesis_time: u64,
    pub genesis_root: B256,
    pub forks: Forks,
}

impl L1ChainBuilder {
    pub fn new() -> Self {
        let store = LightClientStore::default();

        let mut forks = Forks::default();
        forks.deneb.epoch = 269568;
        forks.deneb.fork_version = old_fixed_bytes!("04000000");
        let genesis_root =
            b256!("4b363db94e286120d76eb905340fdd4e54bfe9f06bf33ff6cf5ad27f511bfe95");
        let genesis_time = 1606824023;

        L1ChainBuilder {
            store,
            last_checkpoint: None,
            genesis_root,
            forks,
            genesis_time,
        }
    }

    pub fn build_beacon_chain(
        &mut self,
        bootstrap: Bootstrap,
        checkpoint: OldB256,
        updates: Vec<Update>,
        optimistic_update: OptimisticUpdate,
    ) -> Result<B256> {
        self.bootstrap(bootstrap, checkpoint)?;
        self.advance_updates(updates)?;
        self.advance_optimistic_update(optimistic_update)?;
        let latest_beacon_root = self.store.optimistic_header.tree_hash_root();
        Ok(B256::new(latest_beacon_root.0))
    }

    pub fn bootstrap(&mut self, bootstrap: Bootstrap, checkpoint: OldB256) -> Result<()> {
        verify_bootstrap(&bootstrap, checkpoint).unwrap();
        apply_bootstrap(&mut self.store, &bootstrap);
        Ok(())
    }

    pub fn advance_updates(&mut self, updates: Vec<Update>) -> Result<()> {
        for update in updates {
            let res = self.verify_update(&update);
            if res.is_ok() {
                self.apply_update(&update);
            }
        }

        Ok(())
    }

    pub fn advance_optimistic_update(&mut self, update: OptimisticUpdate) -> Result<()> {
        let res = self.verify_optimistic_update(&update);
        if res.is_ok() {
            self.apply_optimistic_update(&update);
        }
        Ok(())
    }

    pub fn verify_update(&self, update: &Update) -> Result<()> {
        verify_update(
            update,
            update.signature_slot,
            &self.store,
            OldB256::from(self.genesis_root.0),
            &self.forks,
        )
    }

    fn verify_optimistic_update(&self, update: &OptimisticUpdate) -> Result<()> {
        verify_optimistic_update(
            update,
            update.signature_slot,
            &self.store,
            OldB256::from(self.genesis_root.0),
            &self.forks,
        )
    }

    pub fn apply_update(&mut self, update: &Update) {
        let new_checkpoint = apply_update(&mut self.store, update);
        if new_checkpoint.is_some() {
            self.last_checkpoint = Some(B256::new(new_checkpoint.unwrap().0));
        }
    }

    fn apply_optimistic_update(&mut self, update: &OptimisticUpdate) {
        let new_checkpoint = apply_optimistic_update(&mut self.store, update);
        if new_checkpoint.is_some() {
            self.last_checkpoint = Some(B256::new(new_checkpoint.unwrap().0));
        }
    }

}

pub fn read_l1_chain_builder_input() -> (Bootstrap, OldB256, Vec<Update>, OptimisticUpdate, EthEvmInput) {
    let bootstrap_header: Header = env::read();
    let bootstrap_current_sync_committee: SyncCommittee = env::read();
    let bootstrap_current_sync_committee_branch: Vec<OldB256> = env::read();

    let checkpoint: OldB256 = env::read();

    let finality_update_attested_header: Header = env::read();
    let finality_update_sync_aggregate: SyncAggregate = env::read();
    let finality_update_signature_slot: u64 = env::read();

    let update_len: usize = env::read();
    let mut updates: Vec<Update> = Vec::new();
    for _ in 0..update_len {
        let update_attested_header: Header = env::read();
        let update_next_sync_committee: SyncCommittee = env::read();
        let update_next_sync_committee_branch: Vec<OldB256> = env::read();
        let update_finalized_header: Header = env::read();
        let update_finality_branch: Vec<OldB256> = env::read();
        let update_sync_aggregate: SyncAggregate = env::read();
        let update_signature_slot: u64 = env::read();

        let update = Update {
            attested_header: update_attested_header,
            next_sync_committee: update_next_sync_committee,
            next_sync_committee_branch: update_next_sync_committee_branch,
            finalized_header: update_finalized_header,
            finality_branch: update_finality_branch,
            sync_aggregate: update_sync_aggregate,
            signature_slot: update_signature_slot,
        };
        updates.push(update);
    }

    let bootstrap = Bootstrap {
        header: bootstrap_header,
        current_sync_committee: bootstrap_current_sync_committee,
        current_sync_committee_branch: bootstrap_current_sync_committee_branch,
    };

    let finality_update = OptimisticUpdate {
        attested_header: finality_update_attested_header,
        // finalized_header: finality_update_finalized_header,
        // finality_branch: finality_update_finality_branch,
        sync_aggregate: finality_update_sync_aggregate,
        signature_slot: finality_update_signature_slot,
    };

    let beacon_input: EthEvmInput = env::read();

    (bootstrap, checkpoint, updates, finality_update, beacon_input)
}

pub fn validate_balance_of_call(
    chain_id: u64,
    account: Address,
    asset: Address,
    env_input: EthEvmInput,
    sequencer_commitment: Option<SequencerCommitment>,
    op_env_input: Option<EthEvmInput>,
    linking_blocks: Vec<RlpHeader<ConsensusHeader>>,
) {
    let env = env_input.into_env();

    let erc20_contract = Contract::new(asset, &env);

    let call = IERC20::balanceOfCall { account: account };
    let balance = erc20_contract.call_builder(&call).call()._0;

    let last_block = linking_blocks[linking_blocks.len() - 1].clone();

    let (bootstrap, checkpoint, updates, finality_update, beacon_input) = read_l1_chain_builder_input();

    // let (current_beacon_hash, new_checkpoint) = validate_ethereum_env_via_sync_committee(bootstrap, checkpoint, updates, finality_update);
    let current_beacon_hash = B256::new(finality_update.attested_header.tree_hash_root().0);
    validate_chain_length(
        chain_id,
        env.header().seal(),
        linking_blocks,
        last_block.hash_slow(),
    );

    let env = beacon_input.into_env();
    let exec_commit = env.header().seal();
    let beacon_commit = env.commitment().digest;

    assert_eq!(beacon_commit, current_beacon_hash, "beacon commit doesnt correspond to current beacon hash");
    assert_eq!(exec_commit, last_block.hash_slow(), "exec commit doesnt correspond to last block hash");


    let journal = Journal {
        balance,
        account,
        asset,
    };
    env::commit_slice(&journal.abi_encode());
}

pub fn validate_ethereum_env_via_sync_committee(
    bootstrap: Bootstrap,
    checkpoint: OldB256,
    updates: Vec<Update>,
    optimistic_update: OptimisticUpdate,
) -> (B256, B256) {
    let mut l1_chain_builder = L1ChainBuilder::new();
    let verified_root = l1_chain_builder
        .build_beacon_chain(bootstrap, checkpoint, updates, optimistic_update)
        .unwrap();

    let verified_root = B256::new(verified_root.0);

    let new_checkpoint = B256::new(l1_chain_builder.last_checkpoint.unwrap().0);

    (verified_root, new_checkpoint)
}

pub fn validate_chain_length(
    chain_id: u64,
    historical_hash: B256,
    linking_blocks: Vec<RlpHeader<ConsensusHeader>>,
    current_hash: B256,
) {
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
    let chain_length = linking_blocks.len() as u64;
    assert!(
        chain_length >= reorg_protection_depth,
        "chain length is less than reorg protection"
    );
    let mut previous_hash = historical_hash;
    for header in linking_blocks {
        let parent_hash = header.parent_hash;
        assert_eq!(parent_hash, previous_hash, "blocks not hashlinked");
        println!("check passed");
        previous_hash = header.hash_slow();
    }
    assert_eq!(
        previous_hash, current_hash,
        "last hash doesnt correspond to current l1 hash"
    );
}