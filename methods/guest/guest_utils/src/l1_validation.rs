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

    pub fn expected_current_slot(&self) -> u64 {
        let now = SystemTime::now();

        expected_current_slot(now, self.genesis_time)
    }
}

pub fn read_l1_chain_builder_input() -> (
    EthEvmInput,
    Address,
    Bootstrap,
    OldB256,
    Vec<Update>,
    OptimisticUpdate,
) {
    // Read the input data for this application.
    let input: EthEvmInput = env::read();
    let account: Address = env::read();

    let bootstrap_header: Header = env::read();
    let bootstrap_current_sync_committee: SyncCommittee = env::read();
    let bootstrap_current_sync_committee_branch: Vec<OldB256> = env::read();

    let checkpoint: OldB256 = env::read();

    let finality_update_attested_header: Header = env::read();
    // let finality_update_finalized_header: Header = env::read();
    // let finality_update_finality_branch: Vec<OldB256> = env::read();
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

    (
        input,
        account,
        bootstrap,
        checkpoint,
        updates,
        finality_update,
    )
}

pub fn validate_ethereum_env_via_sync_committee(
    bootstrap: Bootstrap,
    checkpoint: OldB256,
    updates: Vec<Update>,
    optimistic_update: OptimisticUpdate,
    beacon_hash: B256,
) {
    let verified_root = L1ChainBuilder::new()
        .build_beacon_chain(bootstrap, checkpoint, updates, optimistic_update)
        .unwrap();

    assert_eq!(beacon_hash, verified_root, "beacon hash mismatch");

    // validate_chain_length(ethereum_hash, linking_blocks, l1_hash);
}

// /// Validates the length and integrity of a chain of blocks.
// ///
// /// Ensures that:
// /// 1. The chain length meets minimum reorg protection requirements
// /// 2. All blocks are properly hash-linked
// /// 3. The final hash matches the expected current hash
// ///
// /// # Arguments
// /// * `historical_hash` - The hash of the historical block to start validation from
// /// * `linking_blocks` - Vector of block headers forming the chain
// /// * `current_hash` - The expected hash of the current block
// ///
// /// # Panics
// /// * If the chain length is less than the reorg protection depth
// /// * If blocks are not properly hash-linked
// /// * If the final hash doesn't match the expected current hash
// pub fn validate_chain_length(
//     historical_hash: B256,
//     linking_blocks: Vec<RlpHeader<Header>>,
//     current_hash: B256,
// ) {
//     let chain_length = linking_blocks.len() as u64;
//     assert!(
//         chain_length >= REORG_PROTECTION_DEPTH,
//         "chain length is less than reorg protection"
//     );
//     let mut previous_hash = historical_hash;
//     for header in linking_blocks {
//         let parent_hash = header.parent_hash;
//         assert_eq!(parent_hash, previous_hash, "blocks not hashlinked");
//         previous_hash = header.hash_slow();
//     }
//     assert_eq!(
//         previous_hash, current_hash,
//         "last hash doesnt correspond to current l1 hash"
//     );
// }
