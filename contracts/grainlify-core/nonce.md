# Nonce Semantics (Replay Protection)

This document defines the expected nonce lifecycle for callers of `grainlify-core/src/nonce.rs`.

## Lifecycle

1. Read expected nonce:
   - Global scope: `get_nonce(env, signer)`
   - Domain scope: `get_nonce_with_domain(env, signer, domain)`
2. Include that exact value in the signed payload.
3. In the state-changing entrypoint, validate and consume it exactly once:
   - Global scope: `validate_and_increment_nonce`
   - Domain scope: `validate_and_increment_nonce_with_domain`
4. Continue business logic only if nonce validation succeeds.

## Security Properties

- Strict monotonicity: each successful call advances nonce by exactly `+1`.
- Replay resistance: reused (stale) nonces fail with `NonceError::InvalidNonce`.
- Order enforcement: future nonces also fail with `NonceError::InvalidNonce`.
- Scope isolation:
  - Nonces are isolated per signer.
  - Domain nonces are isolated per `(signer, domain)` pair.
  - Global and domain nonce spaces are independent.
- Overflow safety: if nonce reaches `u64::MAX`, consumption fails with
  `NonceError::NonceExhausted` and no state mutation occurs.

## Entrypoint Integration Guidance

- Any signer-authorized, state-changing entrypoint should consume a nonce before
  mutating contract state.
- Use domain-scoped nonces for independent flows to reduce accidental coupling
  (for example `upgrade`, `escrow`, `governance`).
- Never auto-correct nonce mismatches in-contract; reject mismatches and let the
  caller refresh the latest expected nonce.
