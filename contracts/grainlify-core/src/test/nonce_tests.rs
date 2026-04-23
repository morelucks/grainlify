#![cfg(test)]

use crate::nonce::{
    get_nonce, get_nonce_with_domain, validate_and_increment_nonce,
    validate_and_increment_nonce_with_domain, NonceError, NonceKey,
};
use crate::GrainlifyContract;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

#[test]
fn nonce_defaults_to_zero_for_new_signer() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer = Address::generate(&env);
        assert_eq!(get_nonce(&env, &signer), 0);
    });
}

#[test]
fn nonce_replay_is_rejected_after_successful_consume() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer = Address::generate(&env);

        assert_eq!(validate_and_increment_nonce(&env, &signer, 0), Ok(()));
        assert_eq!(get_nonce(&env, &signer), 1);

        assert_eq!(
            validate_and_increment_nonce(&env, &signer, 0),
            Err(NonceError::InvalidNonce)
        );
        assert_eq!(get_nonce(&env, &signer), 1);
    });
}

#[test]
fn nonce_rejects_future_value() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer = Address::generate(&env);
        assert_eq!(
            validate_and_increment_nonce(&env, &signer, 1),
            Err(NonceError::InvalidNonce)
        );
        assert_eq!(get_nonce(&env, &signer), 0);
    });
}

#[test]
fn nonces_are_isolated_per_signer() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer_a = Address::generate(&env);
        let signer_b = Address::generate(&env);

        assert_eq!(validate_and_increment_nonce(&env, &signer_a, 0), Ok(()));
        assert_eq!(get_nonce(&env, &signer_a), 1);
        assert_eq!(get_nonce(&env, &signer_b), 0);
        assert_eq!(validate_and_increment_nonce(&env, &signer_b, 0), Ok(()));
    });
}

#[test]
fn domain_nonce_isolation_and_replay_rejection() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer = Address::generate(&env);
        let upgrade = Symbol::new(&env, "upgrade");
        let payout = Symbol::new(&env, "payout");

        assert_eq!(
            validate_and_increment_nonce_with_domain(&env, &signer, upgrade.clone(), 0),
            Ok(())
        );
        assert_eq!(get_nonce_with_domain(&env, &signer, upgrade.clone()), 1);
        assert_eq!(get_nonce_with_domain(&env, &signer, payout.clone()), 0);

        assert_eq!(
            validate_and_increment_nonce_with_domain(&env, &signer, upgrade.clone(), 0),
            Err(NonceError::InvalidNonce)
        );
        assert_eq!(
            validate_and_increment_nonce_with_domain(&env, &signer, payout.clone(), 0),
            Ok(())
        );
    });
}

#[test]
fn global_and_domain_nonces_are_independent() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer = Address::generate(&env);
        let domain = Symbol::new(&env, "upgrade");

        assert_eq!(validate_and_increment_nonce(&env, &signer, 0), Ok(()));
        assert_eq!(get_nonce(&env, &signer), 1);
        assert_eq!(get_nonce_with_domain(&env, &signer, domain.clone()), 0);

        assert_eq!(
            validate_and_increment_nonce_with_domain(&env, &signer, domain.clone(), 0),
            Ok(())
        );
        assert_eq!(get_nonce_with_domain(&env, &signer, domain), 1);
        assert_eq!(get_nonce(&env, &signer), 1);
    });
}

#[test]
fn nonce_exhaustion_is_reported_without_wrapping_global() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer = Address::generate(&env);
        let key = NonceKey::Signer(signer.clone());
        env.storage().persistent().set(&key, &u64::MAX);

        assert_eq!(
            validate_and_increment_nonce(&env, &signer, u64::MAX),
            Err(NonceError::NonceExhausted)
        );
        assert_eq!(get_nonce(&env, &signer), u64::MAX);
    });
}

#[test]
fn nonce_exhaustion_is_reported_without_wrapping_domain() {
    let env = Env::default();
    let contract_id = env.register_contract(None, GrainlifyContract);

    env.as_contract(&contract_id, || {
        let signer = Address::generate(&env);
        let domain = Symbol::new(&env, "upgrade");
        let key = NonceKey::SignerWithDomain(signer.clone(), domain.clone());
        env.storage().persistent().set(&key, &u64::MAX);

        assert_eq!(
            validate_and_increment_nonce_with_domain(&env, &signer, domain.clone(), u64::MAX),
            Err(NonceError::NonceExhausted)
        );
        assert_eq!(get_nonce_with_domain(&env, &signer, domain), u64::MAX);
    });
}
