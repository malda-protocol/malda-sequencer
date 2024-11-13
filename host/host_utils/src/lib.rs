//! Host utilities for the Malda protocol implementation
//! 
//! This crate provides host-side functionality and utilities, including constants,
//! view calls, cryptographic operations, type definitions, and validation logic.
//! It also incorporates guest utilities to ensure comprehensive protocol coverage.

/// Constants used throughout the host implementation
pub mod constants;

/// View call implementations for the host
pub mod viewcalls;

// Re-exported guest utilities for comprehensive protocol implementation
/// Cryptographic operations and utilities
#[path = "../../../methods/guest/guest_utils/src/cryptography.rs"]
pub mod cryptography;

/// Common type definitions shared between host and guest
#[path = "../../../methods/guest/guest_utils/src/types.rs"]
pub mod types;

/// Validation utilities and functions
#[path = "../../../methods/guest/guest_utils/src/validators.rs"]
pub mod validators;
