//! Rust SDK for the Malda protocol
//!
//! Code for host/client and zkVM guest program including constants,
//! view calls, cryptographic operations, type definitions, and validation logic.

pub mod constants;

pub mod viewcalls;

pub mod viewcalls_ethereum_light_client;

#[path = "../../methods/guest/guest_utils/src/cryptography.rs"]
pub mod cryptography;

#[path = "../../methods/guest/guest_utils/src/types.rs"]
pub mod types;

#[path = "../../methods/guest/guest_utils/src/validators.rs"]
pub mod validators;

#[path = "../../methods/guest/guest_utils/src/validators_ethereum_light_client.rs"]
pub mod validators_ethereum_light_client;
