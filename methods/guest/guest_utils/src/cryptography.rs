//! Cryptographic utilities for Ethereum-style signature operations.
//! 
//! This module provides functionality for signature message creation,
//! signer recovery, and address derivation from public keys using
//! the secp256k1 elliptic curve.

use k256::ecdsa::{Error, RecoveryId, VerifyingKey};
use alloy_primitives::{Address, B256, keccak256, Signature};
use crate::constants::SECP256K1N_HALF;

/// Creates a signature message hash following Ethereum's signing scheme.
///
/// # Arguments
///
/// * `data` - The raw data to be signed
/// * `chain_id` - The blockchain network identifier
///
/// # Returns
///
/// Returns a `B256` containing the final message hash to be signed.
///
/// # Details
///
/// The function concatenates three components:
/// - A domain separator (currently zero)
/// - The chain ID in padded format
/// - The keccak256 hash of the input data
pub fn signature_msg(data: &[u8], chain_id: u64) -> B256 {
    let domain = B256::ZERO;
    let chain_id = B256::left_padding_from(&chain_id.to_be_bytes());
    let payload_hash = keccak256(data);

    let signing_data = [
        domain.as_slice(),
        chain_id.as_slice(),
        payload_hash.as_slice(),
    ];

    keccak256(signing_data.concat()).into()
}

/// Recovers the signer's address from a signature and message hash.
///
/// # Arguments
///
/// * `signature` - The signature to recover from
/// * `sighash` - The hash of the signed message
///
/// # Returns
///
/// Returns `Some(Address)` if recovery is successful, `None` otherwise.
///
/// # Notes
///
/// This function performs signature normalization and validates that the S value
/// is in the lower half of the curve order to prevent signature malleability.
pub fn recover_signer(signature: Signature, sighash: B256) -> Option<Address> {
    if signature.s() > SECP256K1N_HALF {
        return None;
    }

    let mut sig: [u8; 65] = [0; 65];

    sig[0..32].copy_from_slice(&signature.r().to_be_bytes::<32>());
    sig[32..64].copy_from_slice(&signature.s().to_be_bytes::<32>());
    sig[64] = signature.v().y_parity_byte();

    // NOTE: we are removing error from underlying crypto library as it will restrain primitive
    // errors and we care only if recovery is passing or not.
    recover_signer_unchecked(&sig, &sighash.0).ok()
}

/// Internal function to perform the actual signature recovery operation.
///
/// # Arguments
///
/// * `sig` - Raw signature bytes (65 bytes: r[32] || s[32] || v[1])
/// * `msg` - 32-byte message hash
///
/// # Returns
///
/// Returns `Result<Address, Error>` with the recovered signer's address or an error.
///
/// # Notes
///
/// This function handles signature normalization and recovery ID adjustment
/// as needed for proper key recovery.
fn recover_signer_unchecked(sig: &[u8; 65], msg: &[u8; 32]) -> Result<Address, Error> {
    let mut signature = k256::ecdsa::Signature::from_slice(&sig[0..64])?;
    let mut recid = sig[64];

    // normalize signature and flip recovery id if needed.
    if let Some(sig_normalized) = signature.normalize_s() {
        signature = sig_normalized;
        recid ^= 1;
    }
    let recid = RecoveryId::from_byte(recid).expect("recovery ID is valid");

    // recover key
    let recovered_key = VerifyingKey::recover_from_prehash(&msg[..], &signature, recid)?;
    Ok(public_key_to_address(recovered_key))
}

/// Converts a public key to its corresponding Ethereum address.
///
/// # Arguments
///
/// * `public` - The verifying key (public key) to convert
///
/// # Returns
///
/// Returns the 20-byte Ethereum address derived from the public key.
///
/// # Details
///
/// The address is derived by:
/// 1. Taking the uncompressed public key bytes (excluding the prefix byte)
/// 2. Computing the keccak256 hash
/// 3. Taking the last 20 bytes of the hash
fn public_key_to_address(public: VerifyingKey) -> Address {
    let hash = keccak256(&public.to_encoded_point(/* compress = */ false).as_bytes()[1..]);
    Address::from_slice(&hash[12..])
}