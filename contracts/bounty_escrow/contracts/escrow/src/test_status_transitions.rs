use super::*;
use soroban_sdk::testutils::Ledger;
use soroban_sdk::{
    testutils::{Address as _, LedgerInfo},
    token, Address, Env,
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

// ============================================================================
// RISK FLAGS GOVERNANCE TESTS
// ============================================================================

#[test]
fn test_update_risk_flags_success() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 1000;

    // Lock funds to create the initial escrow
    setup.escrow.lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);

    // Verify initial risk flags are 0 (no metadata existed yet, fallback applied)
    assert_eq!(setup.escrow.get_risk_flags(&bounty_id), 0);

    // Update risk flags (e.g., HIGH_RISK = 1, UNDER_REVIEW = 2) -> Bitmask 3
    let new_flags = 3;
    setup.escrow.update_risk_flags(&bounty_id, &new_flags);

    // Verify flags persisted in the EscrowMetadata struct
    assert_eq!(setup.escrow.get_risk_flags(&bounty_id), new_flags);
    
    // Clear the flags
    setup.escrow.update_risk_flags(&bounty_id, &0);
    assert_eq!(setup.escrow.get_risk_flags(&bounty_id), 0);
}

#[test]
#[should_panic(expected = "Error(Contract, #202)")]
fn test_update_risk_flags_bounty_not_found() {
    let setup = TestSetup::new();
    let missing_bounty_id = 999;
    
    // Attempting to flag an escrow that does not exist should throw BountyNotFound (202)
    setup.escrow.update_risk_flags(&missing_bounty_id, &1);
}

#[test]
#[should_panic(expected = "Error(Contract, #202)")]
fn test_get_risk_flags_bounty_not_found() {
    let setup = TestSetup::new();
    let missing_bounty_id = 999;
    
    // Attempting to read flags from a missing escrow should fail
    setup.escrow.get_risk_flags(&missing_bounty_id);
}

// ============================================================================
// MAINTENANCE MODE HARDENING TESTS
// ============================================================================

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_maintenance_mode_halts_lock() {
    let setup = TestSetup::new();
    let reason = soroban_sdk::String::from_str(&setup.env, "Emergency upgrade");
    setup.escrow.set_maintenance_mode(&true, &Some(reason));
    
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 1000;
    
    // Should panic with FundsPaused (18)
    setup.escrow.lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_maintenance_mode_halts_release() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 1000;
    
    setup.escrow.lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    
    setup.escrow.set_maintenance_mode(&true, &None);
    
    // Should panic with FundsPaused (18)
    setup.escrow.release_funds(&bounty_id, &setup.contributor);
}

#[test]
#[should_panic(expected = "Error(Contract, #18)")]
fn test_maintenance_mode_halts_refund() {
    let setup = TestSetup::new();
    let bounty_id = 1;
    let amount = 1000;
    let deadline = setup.env.ledger().timestamp() + 100;
    
    setup.escrow.lock_funds(&setup.depositor, &bounty_id, &amount, &deadline);
    setup.env.ledger().set_timestamp(deadline + 1);
    
    setup.escrow.set_maintenance_mode(&true, &None);
    
    // Should panic with FundsPaused (18)
    setup.escrow.refund(&bounty_id);
}

#[test]
fn test_maintenance_mode_toggles_correctly() {
    let setup = TestSetup::new();
    let reason = soroban_sdk::String::from_str(&setup.env, "Routine sync");
    
    assert_eq!(setup.escrow.is_maintenance_mode(), false);
    
    setup.escrow.set_maintenance_mode(&true, &Some(reason));
    assert_eq!(setup.escrow.is_maintenance_mode(), true);
    
    setup.escrow.set_maintenance_mode(&false, &None);
    assert_eq!(setup.escrow.is_maintenance_mode(), false);
}

// ============================================================================
// ADMIN ROTATION TESTS
// ============================================================================

#[test]
fn test_admin_rotation_lifecycle_success() {
    let setup = TestSetup::new();
    let new_admin = Address::generate(&setup.env);
    
    // 1. Configure Timelock to 1 hour (3600 seconds)
    setup.escrow.set_admin_timelock(&3600);
    assert_eq!(setup.escrow.get_admin_timelock(), 3600);

    // 2. Propose Admin
    let current_time = setup.env.ledger().timestamp();
    setup.escrow.propose_admin(&new_admin);
    
    let pending = setup.escrow.get_pending_admin().unwrap();
    assert_eq!(pending.proposed_admin, new_admin);
    assert_eq!(pending.available_at, current_time + 3600);

    // 3. Fast forward time past the timelock
    setup.env.ledger().set_timestamp(current_time + 3601);

    // 4. Accept Admin (must be signed by the new admin)
    setup.escrow.accept_admin();

    // 5. Verify the transfer state is cleaned up
    assert!(setup.escrow.get_pending_admin().is_none());
}

#[test]
#[should_panic(expected = "Error(Contract, #6)")] // DeadlineNotPassed
fn test_admin_rotation_fails_before_timelock() {
    let setup = TestSetup::new();
    let new_admin = Address::generate(&setup.env);
    
    setup.escrow.propose_admin(&new_admin);
    
    // Try to accept immediately without advancing the ledger timestamp
    setup.escrow.accept_admin();
}

#[test]
fn test_admin_rotation_cancel() {
    let setup = TestSetup::new();
    let new_admin = Address::generate(&setup.env);
    
    setup.escrow.propose_admin(&new_admin);
    assert!(setup.escrow.get_pending_admin().is_some());
    
    setup.escrow.cancel_admin_transfer();
    assert!(setup.escrow.get_pending_admin().is_none());
}

#[test]
#[should_panic(expected = "Error(Contract, #7)")] // Unauthorized
fn test_accept_admin_unauthorized_if_none_pending() {
    let setup = TestSetup::new();
    // Try to accept when nothing is proposed
    setup.escrow.accept_admin();
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")] // InvalidAmount
fn test_admin_timelock_enforces_minimum() {
    let setup = TestSetup::new();
    // Try to set timelock to 1 second (minimum is 300)
    setup.escrow.set_admin_timelock(&1);
}

// ============================================================================
// BATCH SIZE GOVERNANCE TESTS
// ============================================================================

#[test]
fn test_batch_size_governance_lifecycle() {
    let setup = TestSetup::new();
    
    // 1. Verify default batch size is 20
    assert_eq!(setup.escrow.get_max_batch_size(), 20);

    // 2. Admin updates the batch size to 50
    setup.escrow.set_max_batch_size(&50);
    assert_eq!(setup.escrow.get_max_batch_size(), 50);
    
    // 3. Admin updates the batch size down to 5
    setup.escrow.set_max_batch_size(&5);
    assert_eq!(setup.escrow.get_max_batch_size(), 5);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")] // InvalidAmount
fn test_batch_size_enforces_zero_bound() {
    let setup = TestSetup::new();
    // Cannot set batch size to 0
    setup.escrow.set_max_batch_size(&0);
}

#[test]
#[should_panic(expected = "Error(Contract, #13)")] // InvalidAmount
fn test_batch_size_enforces_hard_limit() {
    let setup = TestSetup::new();
    // Cannot set batch size above HARD_LIMIT_BATCH_SIZE (100)
    setup.escrow.set_max_batch_size(&101);
}

#[test]
#[should_panic(expected = "Error(Contract, #10)")] // InvalidBatchSize
fn test_batch_lock_fails_when_exceeding_dynamic_cap() {
    let setup = TestSetup::new();
    
    // Set cap to 1
    setup.escrow.set_max_batch_size(&1);
    
    let deadline = setup.env.ledger().timestamp() + 1000;
    let mut items = soroban_sdk::Vec::new(&setup.env);
    items.push_back(LockFundsItem {
        bounty_id: 1,
        depositor: setup.depositor.clone(),
        amount: 100,
        deadline,
    });
    items.push_back(LockFundsItem {
        bounty_id: 2,
        depositor: setup.depositor.clone(),
        amount: 100,
        deadline,
    });
    
    // Attempting to batch lock 2 items when the cap is 1 should panic
    setup.escrow.batch_lock_funds(&items);
}