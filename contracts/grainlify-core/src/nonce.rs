//! Nonce helpers for replay protection in signer-authorized flows.
//!
//! # Nonce Lifecycle
//! 1. Callers read the expected nonce with [`get_nonce`] or [`get_nonce_with_domain`].
//! 2. The signed payload must include that exact nonce value.
//! 3. The entrypoint validates and consumes it with
//!    [`validate_and_increment_nonce`] or [`validate_and_increment_nonce_with_domain`].
//! 4. On success, the stored nonce increases by exactly `1`.
//!
//! A consumed nonce can never be reused. Supplying stale or future values fails with
//! [`NonceError::InvalidNonce`]. Domain-scoped nonces are isolated from global nonces and from
//! other domains for the same signer.

use soroban_sdk::{contracterror, contracttype, Address, Env, Symbol};

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum NonceError {
    /// Provided nonce does not match the expected current nonce.
    InvalidNonce = 100,
    /// Nonce reached `u64::MAX` and cannot be incremented anymore.
    NonceExhausted = 101,
}

/// Persistent storage keys used for nonce tracking.
#[contracttype]
#[derive(Clone)]
pub enum NonceKey {
    /// Signer-wide nonce shared by all flows using global nonce validation.
    Signer(Address),
    /// Signer nonce isolated to a logical domain (for example an entrypoint symbol).
    SignerWithDomain(Address, Symbol),
}

/// Returns the current global nonce for `signer`.
///
/// Returns `0` when no nonce has been consumed yet.
pub fn get_nonce(env: &Env, signer: &Address) -> u64 {
    let key = NonceKey::Signer(signer.clone());
    env.storage().persistent().get(&key).unwrap_or(0)
}

/// Returns the current nonce for `signer` inside `domain`.
///
/// Returns `0` when the signer has never consumed a nonce in this domain.
pub fn get_nonce_with_domain(env: &Env, signer: &Address, domain: Symbol) -> u64 {
    let key = NonceKey::SignerWithDomain(signer.clone(), domain);
    env.storage().persistent().get(&key).unwrap_or(0)
}

fn validate_and_increment_nonce_for_key(
    env: &Env,
    key: NonceKey,
    provided_nonce: u64,
) -> Result<(), NonceError> {
    let current_nonce: u64 = env.storage().persistent().get(&key).unwrap_or(0);
    if provided_nonce != current_nonce {
        return Err(NonceError::InvalidNonce);
    }

    let next_nonce = current_nonce
        .checked_add(1)
        .ok_or(NonceError::NonceExhausted)?;
    env.storage().persistent().set(&key, &next_nonce);
    Ok(())
}

/// Validates `provided_nonce` against the signer's global nonce and consumes it.
///
/// # Errors
/// - [`NonceError::InvalidNonce`] when `provided_nonce` is stale or out of order.
/// - [`NonceError::NonceExhausted`] when the stored nonce is already `u64::MAX`.
pub fn validate_and_increment_nonce(
    env: &Env,
    signer: &Address,
    provided_nonce: u64,
) -> Result<(), NonceError> {
    let key = NonceKey::Signer(signer.clone());
    validate_and_increment_nonce_for_key(env, key, provided_nonce)
}

/// Validates `provided_nonce` against the signer's nonce in `domain` and consumes it.
///
/// # Errors
/// - [`NonceError::InvalidNonce`] when `provided_nonce` is stale or out of order.
/// - [`NonceError::NonceExhausted`] when the stored nonce is already `u64::MAX`.
pub fn validate_and_increment_nonce_with_domain(
    env: &Env,
    signer: &Address,
    domain: Symbol,
    provided_nonce: u64,
) -> Result<(), NonceError> {
    let key = NonceKey::SignerWithDomain(signer.clone(), domain);
    validate_and_increment_nonce_for_key(env, key, provided_nonce)
}
