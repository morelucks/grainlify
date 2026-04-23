use super::*;
use soroban_sdk::testutils::{Events, Ledger};
use soroban_sdk::{
    testutils::{Address as _, LedgerInfo, MockAuth, MockAuthInvoke},
    token, Address, Env, IntoVal, Symbol, TryIntoVal, Val,
};

fn create_token_contract<'a>(
    e: &Env,
    admin: &Address,
) -> (token::Client<'a>, token::StellarAssetClient<'a>) {
    let contract = e.register_stellar_asset_contract_v2(admin.clone());
    let contract_address = contract.address();
    (
        token::Client::new(e, &contract_address),
        token::StellarAssetClient::new(e, &contract_address),
    )
}

fn create_escrow_contract<'a>(e: &Env) -> BountyEscrowContractClient<'a> {
    let contract_id = e.register_contract(None, BountyEscrowContract);
    BountyEscrowContractClient::new(e, &contract_id)
}

struct TestSetup<'a> {
    env: Env,
    #[allow(dead_code)]
    admin: Address,
    depositor: Address,
    contributor: Address,
    #[allow(dead_code)]
    token: token::Client<'a>,
    #[allow(dead_code)]
    token_admin: token::StellarAssetClient<'a>,
    escrow: BountyEscrowContractClient<'a>,
}

impl<'a> TestSetup<'a> {
    fn new() -> Self {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let depositor = Address::generate(&env);
        let contributor = Address::generate(&env);

        let (token, token_admin) = create_token_contract(&env, &admin);
        let escrow = create_escrow_contract(&env);

        escrow.init(&admin, &token.address);
        token_admin.mint(&depositor, &1_000_000);

        Self {
            env,
            admin,
            depositor,
            contributor,
            token,
            token_admin,
            escrow,
        }
    }
}

struct RotationSetup<'a> {
    env: Env,
    admin: Address,
    pending_admin: Address,
    replacement_admin: Address,
    escrow: BountyEscrowContractClient<'a>,
}

impl<'a> RotationSetup<'a> {
    fn new() -> Self {
        let env = Env::default();
        let admin = Address::generate(&env);
        let pending_admin = Address::generate(&env);
        let replacement_admin = Address::generate(&env);
        let (token, _token_admin) = create_token_contract(&env, &admin);
        let escrow = create_escrow_contract(&env);

        authorize_contract_call(
            &env,
            &escrow,
            &admin,
            "init",
            (&admin, &token.address).into_val(&env),
        );
        escrow.init(&admin, &token.address);

        Self {
            env,
            admin,
            pending_admin,
            replacement_admin,
            escrow,
        }
    }

    fn authorize(&self, address: &Address, fn_name: &'static str, args: Val) {
        authorize_contract_call(&self.env, &self.escrow, address, fn_name, args);
    }
}

fn authorize_contract_call(
    env: &Env,
    escrow: &BountyEscrowContractClient<'_>,
    address: &Address,
    fn_name: &'static str,
    args: Val,
) {
    env.mock_auths(&[MockAuth {
        address,
        invoke: &MockAuthInvoke {
            contract: &escrow.address,
            fn_name,
            args,
            sub_invokes: &[],
        },
    }]);
}

#[test]
fn test_refund_eligibility_ineligible_before_deadline_without_approval() {
    let setup = TestSetup::new();
    let bounty_id = 99;
    let amount = 1_000;
    let deadline = setup.env.ledger().timestamp() + 500;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);

    let view = setup.escrow.get_refund_eligibility_view(&bounty_id);
    assert!(!view.eligible);
    assert_eq!(
        view.code,
        RefundEligibilityCode::IneligibleDeadlineNotPassed
    );
    assert_eq!(view.amount, 0);
    assert!(!view.approval_present);
}

#[test]
fn test_refund_eligibility_eligible_after_deadline() {
    let setup = TestSetup::new();
    let bounty_id = 100;
    let amount = 1_200;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.env.ledger().set_timestamp(deadline + 1);

    let view = setup.escrow.get_refund_eligibility_view(&bounty_id);
    assert!(view.eligible);
    assert_eq!(view.code, RefundEligibilityCode::EligibleDeadlinePassed);
    assert_eq!(view.amount, amount);
    assert_eq!(view.recipient, Some(setup.depositor.clone()));
    assert!(!view.approval_present);
}

#[test]
fn test_refund_eligibility_eligible_with_admin_approval_before_deadline() {
    let setup = TestSetup::new();
    let bounty_id = 101;
    let amount = 2_000;
    let deadline = setup.env.ledger().timestamp() + 1_000;
    let custom_recipient = Address::generate(&setup.env);

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.escrow.approve_refund(
        &bounty_id,
        &500,
        &custom_recipient,
        &RefundMode::Partial,
    );

    let view = setup.escrow.get_refund_eligibility_view(&bounty_id);
    assert!(view.eligible);
    assert_eq!(view.code, RefundEligibilityCode::EligibleAdminApproval);
    assert_eq!(view.amount, 500);
    assert_eq!(view.recipient, Some(custom_recipient));
    assert!(view.approval_present);
}

// Valid transitions: Locked → Released
#[test]
fn test_locked_to_released() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 1000;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::Locked
    );

    setup.escrow.release_funds(&bounty_id, &setup.contributor);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::Released
    );
}

// Valid transitions: Locked → Refunded
#[test]
fn test_locked_to_refunded() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::Locked
    );

    setup.env.ledger().set_timestamp(deadline + 1);
    setup.escrow.refund(&bounty_id);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::Refunded
    );
}

// Valid transitions: Locked → PartiallyRefunded
#[test]
fn test_locked_to_partially_refunded() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::Locked
    );

    // Approve partial refund before deadline
    setup
        .escrow
        .approve_refund(&bounty_id, &500, &setup.depositor, &RefundMode::Partial);
    setup.escrow.refund(&bounty_id);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::PartiallyRefunded
    );
}

// Valid transitions: PartiallyRefunded → Refunded
#[test]
fn test_partially_refunded_to_refunded() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);

    // First partial refund
    setup
        .escrow
        .approve_refund(&bounty_id, &500, &setup.depositor, &RefundMode::Partial);
    setup.escrow.refund(&bounty_id);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::PartiallyRefunded
    );

    // Second refund completes it
    setup.env.ledger().set_timestamp(deadline + 1);
    setup.escrow.refund(&bounty_id);
    assert_eq!(
        setup.escrow.get_escrow_info(&bounty_id).status,
        EscrowStatus::Refunded
    );
}

// Invalid transition: Released → Locked
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_released_to_locked_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 1000;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.escrow.release_funds(&bounty_id, &setup.contributor);

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
}

// Invalid transition: Released → Released
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_released_to_released_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 1000;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.escrow.release_funds(&bounty_id, &setup.contributor);

    setup.escrow.release_funds(&bounty_id, &setup.contributor);
}

// Invalid transition: Released → Refunded
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_released_to_refunded_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.escrow.release_funds(&bounty_id, &setup.contributor);

    setup.env.ledger().set_timestamp(deadline + 1);
    setup.escrow.refund(&bounty_id);
}

// Invalid transition: Released → PartiallyRefunded
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_released_to_partially_refunded_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.escrow.release_funds(&bounty_id, &setup.contributor);

    setup.env.ledger().set_timestamp(deadline + 1);
    setup
        .escrow
        .partial_release(&bounty_id, &setup.contributor, &500);
}

// Invalid transition: Refunded → Locked
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_refunded_to_locked_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.env.ledger().set(LedgerInfo {
        timestamp: deadline + 1,
        protocol_version: 20,
        sequence_number: 0,
        network_id: Default::default(),
        base_reserve: 0,
        min_temp_entry_ttl: 0,
        min_persistent_entry_ttl: 0,
        max_entry_ttl: 0,
    });
    setup.escrow.refund(&bounty_id);

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
}

// Invalid transition: Refunded → Released
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_refunded_to_released_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.env.ledger().set(LedgerInfo {
        timestamp: deadline + 1,
        protocol_version: 20,
        sequence_number: 0,
        network_id: Default::default(),
        base_reserve: 0,
        min_temp_entry_ttl: 0,
        min_persistent_entry_ttl: 0,
        max_entry_ttl: 0,
    });
    setup.escrow.refund(&bounty_id);

    setup.escrow.release_funds(&bounty_id, &setup.contributor);
}

// Invalid transition: Refunded → Refunded
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_refunded_to_refunded_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.env.ledger().set(LedgerInfo {
        timestamp: deadline + 1,
        protocol_version: 20,
        sequence_number: 0,
        network_id: Default::default(),
        base_reserve: 0,
        min_temp_entry_ttl: 0,
        min_persistent_entry_ttl: 0,
        max_entry_ttl: 0,
    });
    setup.escrow.refund(&bounty_id);

    setup.escrow.refund(&bounty_id);
}

// Invalid transition: Refunded → PartiallyRefunded
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_refunded_to_partially_refunded_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.env.ledger().set(LedgerInfo {
        timestamp: deadline + 1,
        protocol_version: 20,
        sequence_number: 0,
        network_id: Default::default(),
        base_reserve: 0,
        min_temp_entry_ttl: 0,
        min_persistent_entry_ttl: 0,
        max_entry_ttl: 0,
    });
    setup.escrow.refund(&bounty_id);

    setup
        .escrow
        .partial_release(&bounty_id, &setup.contributor, &100);
}

// Invalid transition: PartiallyRefunded → Locked
#[test]
#[should_panic(expected = "Error(Contract, #3)")]
fn test_partially_refunded_to_locked_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup
        .escrow
        .approve_refund(&bounty_id, &500, &setup.depositor, &RefundMode::Partial);
    setup.escrow.refund(&bounty_id);

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
}

// Invalid transition: PartiallyRefunded → Released
#[test]
#[should_panic(expected = "Error(Contract, #5)")]
fn test_partially_refunded_to_released_fails() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;

    setup
        .escrow
        .lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup
        .escrow
        .approve_refund(&bounty_id, &500, &setup.depositor, &RefundMode::Partial);
    setup.escrow.refund(&bounty_id);

    setup.escrow.release_funds(&bounty_id, &setup.contributor);
}

#[test]
fn test_admin_rotation_proposal_sets_pending_state_and_emits_event() {
    let setup = RotationSetup::new();

    assert_eq!(setup.escrow.get_admin(), Some(setup.admin.clone()));
    assert_eq!(setup.escrow.get_pending_admin(), None);
    assert_eq!(
        setup.escrow.get_admin_rotation_timelock_duration(),
        86_400
    );

    let now = setup.env.ledger().timestamp();
    setup.authorize(
        &setup.admin,
        "propose_admin_rotation",
        (setup.pending_admin.clone(),).into_val(&setup.env),
    );
    let execute_after = setup.escrow.propose_admin_rotation(&setup.pending_admin);

    assert_eq!(execute_after, now + 86_400);
    assert_eq!(setup.escrow.get_pending_admin(), Some(setup.pending_admin.clone()));
    assert_eq!(
        setup.escrow.get_admin_rotation_timelock(),
        Some(execute_after)
    );

    let event = setup.env.events().all().last().unwrap().clone();
    let topic_0: Symbol = event.1.get(0).unwrap().into_val(&setup.env);
    assert_eq!(topic_0, Symbol::new(&setup.env, "admrotp"));

    let data: events::AdminRotationProposed = event.2.try_into_val(&setup.env).unwrap();
    assert_eq!(data.version, EVENT_VERSION_V2);
    assert_eq!(data.current_admin, setup.admin);
    assert_eq!(data.pending_admin, setup.pending_admin);
    assert_eq!(data.timelock_duration, 86_400);
    assert_eq!(data.execute_after, execute_after);
    assert_eq!(data.timestamp, now);
}

#[test]
fn test_admin_rotation_accept_requires_elapsed_timelock() {
    let setup = RotationSetup::new();

    setup.authorize(
        &setup.admin,
        "propose_admin_rotation",
        (setup.pending_admin.clone(),).into_val(&setup.env),
    );
    setup.escrow.propose_admin_rotation(&setup.pending_admin);

    setup.authorize(
        &setup.pending_admin,
        "accept_admin_rotation",
        ().into_val(&setup.env),
    );
    let result = setup.escrow.try_accept_admin_rotation();

    assert_eq!(
        result.unwrap_err().unwrap(),
        Error::AdminRotationTimelockActive
    );
    assert_eq!(setup.escrow.get_admin(), Some(setup.admin.clone()));
    assert_eq!(setup.escrow.get_pending_admin(), Some(setup.pending_admin));
}

#[test]
fn test_admin_rotation_accept_requires_pending_proposal() {
    let setup = RotationSetup::new();

    setup.authorize(
        &setup.pending_admin,
        "accept_admin_rotation",
        ().into_val(&setup.env),
    );
    let result = setup.escrow.try_accept_admin_rotation();

    assert_eq!(
        result.unwrap_err().unwrap(),
        Error::AdminRotationNotPending
    );
    assert_eq!(setup.escrow.get_admin(), Some(setup.admin));
    assert_eq!(setup.escrow.get_pending_admin(), None);
}

#[test]
fn test_admin_rotation_accept_updates_admin_and_emits_event() {
    let setup = RotationSetup::new();

    setup.authorize(
        &setup.admin,
        "propose_admin_rotation",
        (setup.pending_admin.clone(),).into_val(&setup.env),
    );
    let execute_after = setup.escrow.propose_admin_rotation(&setup.pending_admin);

    setup.env.ledger().set_timestamp(execute_after);
    setup.authorize(
        &setup.pending_admin,
        "accept_admin_rotation",
        ().into_val(&setup.env),
    );
    let new_admin = setup.escrow.accept_admin_rotation();

    assert_eq!(new_admin, setup.pending_admin);
    assert_eq!(setup.escrow.get_admin(), Some(setup.pending_admin.clone()));
    assert_eq!(setup.escrow.get_pending_admin(), None);
    assert_eq!(setup.escrow.get_admin_rotation_timelock(), None);

    let event = setup.env.events().all().last().unwrap().clone();
    let topic_0: Symbol = event.1.get(0).unwrap().into_val(&setup.env);
    assert_eq!(topic_0, Symbol::new(&setup.env, "admrota"));

    let data: events::AdminRotationAccepted = event.2.try_into_val(&setup.env).unwrap();
    assert_eq!(data.version, EVENT_VERSION_V2);
    assert_eq!(data.previous_admin, setup.admin);
    assert_eq!(data.new_admin, setup.pending_admin);
    assert_eq!(data.timestamp, execute_after);
}

#[test]
fn test_admin_rotation_rejects_current_admin_as_target() {
    let setup = RotationSetup::new();

    setup.authorize(
        &setup.admin,
        "propose_admin_rotation",
        (setup.admin.clone(),).into_val(&setup.env),
    );
    let result = setup.escrow.try_propose_admin_rotation(&setup.admin);

    assert_eq!(
        result.unwrap_err().unwrap(),
        Error::InvalidAdminRotationTarget
    );
    assert_eq!(setup.escrow.get_pending_admin(), None);
    assert_eq!(setup.escrow.get_admin_rotation_timelock(), None);
}

#[test]
fn test_admin_rotation_cancel_clears_pending_state() {
    let setup = RotationSetup::new();

    setup.authorize(
        &setup.admin,
        "propose_admin_rotation",
        (setup.pending_admin.clone(),).into_val(&setup.env),
    );
    setup.escrow.propose_admin_rotation(&setup.pending_admin);

    setup.authorize(
        &setup.admin,
        "cancel_admin_rotation",
        ().into_val(&setup.env),
    );
    setup.escrow.cancel_admin_rotation();

    assert_eq!(setup.escrow.get_admin(), Some(setup.admin.clone()));
    assert_eq!(setup.escrow.get_pending_admin(), None);
    assert_eq!(setup.escrow.get_admin_rotation_timelock(), None);

    let event = setup.env.events().all().last().unwrap().clone();
    let topic_0: Symbol = event.1.get(0).unwrap().into_val(&setup.env);
    assert_eq!(topic_0, Symbol::new(&setup.env, "admrotc"));

    let data: events::AdminRotationCancelled = event.2.try_into_val(&setup.env).unwrap();
    assert_eq!(data.version, EVENT_VERSION_V2);
    assert_eq!(data.admin, setup.admin);
    assert_eq!(data.cancelled_pending_admin, setup.pending_admin);
}

#[test]
fn test_admin_rotation_rejects_second_pending_proposal() {
    let setup = RotationSetup::new();

    setup.authorize(
        &setup.admin,
        "propose_admin_rotation",
        (setup.pending_admin.clone(),).into_val(&setup.env),
    );
    setup.escrow.propose_admin_rotation(&setup.pending_admin);

    setup.authorize(
        &setup.admin,
        "propose_admin_rotation",
        (setup.replacement_admin.clone(),).into_val(&setup.env),
    );
    let result = setup
        .escrow
        .try_propose_admin_rotation(&setup.replacement_admin);

    assert_eq!(
        result.unwrap_err().unwrap(),
        Error::AdminRotationAlreadyPending
    );
    assert_eq!(setup.escrow.get_pending_admin(), Some(setup.pending_admin));
}

#[test]
fn test_admin_rotation_timelock_duration_update_has_bounds_and_event() {
    let setup = RotationSetup::new();
    let new_duration = 172_800u64;

    setup.authorize(
        &setup.admin,
        "set_admin_rotation_timelock_duration",
        (new_duration,).into_val(&setup.env),
    );
    setup
        .escrow
        .set_admin_rotation_timelock_duration(&new_duration);

    assert_eq!(
        setup.escrow.get_admin_rotation_timelock_duration(),
        new_duration
    );

    let event = setup.env.events().all().last().unwrap().clone();
    let topic_0: Symbol = event.1.get(0).unwrap().into_val(&setup.env);
    assert_eq!(topic_0, Symbol::new(&setup.env, "admtlcfg"));

    let data: events::AdminRotationTimelockUpdated =
        event.2.try_into_val(&setup.env).unwrap();
    assert_eq!(data.version, EVENT_VERSION_V2);
    assert_eq!(data.admin, setup.admin);
    assert_eq!(data.previous_duration, 86_400);
    assert_eq!(data.new_duration, new_duration);

    setup.authorize(
        &setup.admin,
        "set_admin_rotation_timelock_duration",
        (3_599u64,).into_val(&setup.env),
    );
    let result = setup
        .escrow
        .try_set_admin_rotation_timelock_duration(&3_599u64);

    assert_eq!(
        result.unwrap_err().unwrap(),
        Error::InvalidAdminRotationTimelock
    );
}
