#![cfg(test)]

use crate::{ProgramEscrowContract, ProgramEscrowContractClient, ProgramStatus, ProgramPublishedEvent};
use soroban_sdk::{
    testutils::{Address as _, Events, Ledger},
    token, Address, Env, IntoVal, String, Symbol, TryIntoVal, vec,
};

fn create_token_contract<'a>(env: &Env, admin: &Address) -> token::Client<'a> {
    let token_contract = env.register_stellar_asset_contract_v2(admin.clone());
    let token_address = token_contract.address();
    token::Client::new(env, &token_address)
}

fn setup_test_env<'a>(env: &Env) -> (ProgramEscrowContractClient<'a>, Address, Address, token::Client<'a>, String) {
    let contract_id = env.register_contract(None, ProgramEscrowContract);
    let client = ProgramEscrowContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    let payout_key = Address::generate(env);
    let token_admin = Address::generate(env);
    let token_client = create_token_contract(env, &token_admin);
    let program_id = String::from_str(env, "test-prog");

    env.mock_all_auths();
    
    // Initialize the contract first
    client.initialize_contract(&admin);

    client.init_program(
        &program_id,
        &payout_key,
        &token_client.address,
        &admin,
        &None,
        &None,
    );

    (client, admin, payout_key, token_client, program_id)
}

#[test]
fn test_program_starts_in_draft_status() {
    let env = Env::default();
    let (client, _admin, _payout_key, _token, _program_id) = setup_test_env(&env);

    let program_data = client.get_program_info();
    assert_eq!(program_data.status, ProgramStatus::Draft);
}

#[test]
#[should_panic(expected = "Program is in Draft status. Publish the program first.")]
fn test_lock_fails_in_draft_status() {
    let env = Env::default();
    let (client, _admin, _payout_key, token, _program_id) = setup_test_env(&env);

    let depositor = Address::generate(&env);
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&depositor, &1000);
    
    // Attempt to lock funds
    client.lock_program_funds_from(&1000, &depositor);
}

#[test]
#[should_panic(expected = "Program is in Draft status. Publish the program first.")]
fn test_single_payout_fails_in_draft_status() {
    let env = Env::default();
    let (client, _admin, _payout_key, _token, _program_id) = setup_test_env(&env);

    let recipient = Address::generate(&env);
    client.single_payout(&recipient, &100);
}

#[test]
#[should_panic(expected = "Program is in Draft status. Publish the program first.")]
fn test_batch_payout_fails_in_draft_status() {
    let env = Env::default();
    let (client, _admin, _payout_key, _token, _program_id) = setup_test_env(&env);

    let recipient = Address::generate(&env);
    client.batch_payout(&vec![&env, recipient], &vec![&env, 100]);
}

#[test]
#[should_panic(expected = "Program is in Draft status. Publish the program first.")]
fn test_create_schedule_fails_in_draft_status() {
    let env = Env::default();
    let (client, _admin, _payout_key, _token, _program_id) = setup_test_env(&env);

    let recipient = Address::generate(&env);
    client.create_program_release_schedule(&recipient, &100, &1000);
}

#[test]
fn test_publish_program_success() {
    let env = Env::default();
    let (client, _admin, _payout_key, _token, program_id) = setup_test_env(&env);

    env.ledger().with_mut(|li| {
        li.timestamp = 12345;
    });

    client.publish_program(&program_id);

    let program_data = client.get_program_info();
    assert_eq!(program_data.status, ProgramStatus::Active);

    // Verify event
    let events = env.events().all();
    let emitted = events.iter().last().unwrap();
    let topics = emitted.1;
    let topic_0: Symbol = topics.get(0).unwrap().into_val(&env);
    assert_eq!(topic_0, Symbol::new(&env, "PrgPub"));

    let data: ProgramPublishedEvent = emitted.2.try_into_val(&env).unwrap();
    assert_eq!(data.program_id, program_id);
    assert_eq!(data.published_at, 12345);
    assert_eq!(data.version, 2);
}

#[test]
#[should_panic(expected = "Program already published")]
fn test_publish_already_active_fails() {
    let env = Env::default();
    let (client, _admin, _payout_key, _token, program_id) = setup_test_env(&env);

    client.publish_program(&program_id);
    client.publish_program(&program_id); // Should panic
}

#[test]
fn test_operations_succeed_after_publish() {
    let env = Env::default();
    let (client, _admin, _payout_key, token, program_id) = setup_test_env(&env);

    client.publish_program(&program_id);

    let depositor = Address::generate(&env);
    let token_admin_client = soroban_sdk::token::StellarAssetClient::new(&env, &token.address);
    token_admin_client.mint(&depositor, &5000);
    
    // Approve the contract to spend depositor's tokens
    token.approve(&depositor, &client.address, &5000, &99999);
    
    // Now lock should work
    client.lock_program_funds_from(&5000, &depositor);
    
    let program_data = client.get_program_info();
    assert_eq!(program_data.remaining_balance, 5000);

    // Payout should work
    let recipient = Address::generate(&env);
    client.single_payout(&recipient, &1000);
    assert_eq!(token.balance(&recipient), 1000);
}
