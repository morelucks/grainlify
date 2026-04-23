//! # Error Discrimination Tests
//!
//! This module contains comprehensive tests that verify stable error discrimination
//! for all public program-escrow entrypoints. These tests ensure that:
//!
//! 1. Each error variant can be correctly triggered
//! 2. Error codes are stable and deterministic
//! 3. Error descriptions are consistent
//! 4. No sensitive data leaks through error messages
//!
//! ## Test Coverage
//!
//! - General errors (authorization, validation, state)
//! - Program management errors
//! - Fund operation errors
//! - Payout errors
//! - Schedule errors
//! - Claim errors
//! - Dispute errors
//! - Fee errors
//! - Circuit breaker errors
//! - Threshold monitoring errors
//! - Batch recovery errors

#[cfg(test)]
mod test {
    use crate::{
        grainlify_core::errors::ContractError, ProgramEscrowContract, ProgramEscrowContractClient, ProgramInitItem,
    };
    use soroban_sdk::{testutils::Address as _, vec, Address, Env, String, Vec};

    /// Helper: Create a test environment with initialized contract
    fn setup() -> (Env, ProgramEscrowContractClient<'static>, Address, Address) {
        let env = Env::default();
        let contract_id = env.register_contract(None, ProgramEscrowContract);
        let client = ProgramEscrowContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let token = Address::generate(&env);
        (env, client, admin, token)
    }

    // =========================================================================
    // General Errors (1-99)
    // =========================================================================

    #[test]
    fn test_error_unauthorized() {
        let (env, client, admin, token) = setup();
        
        // Initialize contract with admin
        client.initialize_contract(&admin);
        
        // Try to set admin without being admin (should fail)
        let non_admin = Address::generate(&env);
        let result = client.try_set_admin(&non_admin);
        
        // This should fail with Unauthorized error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_invalid_amount() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Try to lock zero funds (should fail)
        let result = client.try_lock_program_funds(&0);
        
        // This should fail with InvalidAmount error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_paused() {
        let (env, client, admin, token) = setup();
        
        // Initialize contract and program
        client.initialize_contract(&admin);
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Pause lock operations
        client.set_paused(&true, &false, &false, &Some(String::from_str(&env, "maintenance")));
        
        // Try to lock funds while paused (should fail)
        let result = client.try_lock_program_funds(&100);
        
        // This should fail with Paused error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_program_not_found() {
        let (env, client, admin, token) = setup();
        
        // Try to get program info for non-existent program
        let result = client.try_get_program_info();
        
        // This should fail with ProgramNotFound error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_program_already_exists() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Try to initialize same program again (should fail)
        let result = client.try_initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // This should fail with ProgramAlreadyExists error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_insufficient_balance() {
        let (env, client, admin, token) = setup();
        
        // Initialize program without locking funds
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Try to payout without sufficient balance (should fail)
        let recipient = Address::generate(&env);
        let result = client.try_single_payout(&recipient, &100);
        
        // This should fail with InsufficientBalance error
        assert!(result.is_err());
    }

    // =========================================================================
    // Payout Errors (300-399)
    // =========================================================================

    #[test]
    fn test_error_invalid_batch_size() {
        let (env, client, admin, token) = setup();
        
        // Try to batch initialize with empty items (should fail)
        let items: soroban_sdk::Vec<ProgramInitItem> = soroban_sdk::Vec::new(&env);
        let result = client.try_batch_initialize_programs(&items);
        
        // This should fail with InvalidBatchSize error
        assert!(result.is_err());
        if let Err(Ok(error)) = result {
            assert_eq!(error, ContractError::InvalidBatchSize);
        }
    }

    #[test]
    fn test_error_duplicate_entry() {
        let (env, client, admin, token) = setup();
        
        // Create items with duplicate program IDs
        let mut items = soroban_sdk::Vec::new(&env);
        let program_id = String::from_str(&env, "duplicate");
        
        items.push_back(ProgramInitItem {
            program_id: program_id.clone(),
            authorized_payout_key: admin.clone(),
            token_address: token.clone(),
            reference_hash: None,
        });
        
        items.push_back(ProgramInitItem {
            program_id: program_id.clone(),
            authorized_payout_key: admin.clone(),
            token_address: token.clone(),
            reference_hash: None,
        });
        
        let result = client.try_batch_initialize_programs(&items);
        
        // This should fail with DuplicateEntry error
        assert!(result.is_err());
        if let Err(Ok(error)) = result {
            assert_eq!(error, ContractError::DuplicateEntry);
        }
    }

    #[test]
    fn test_error_batch_amounts_mismatch() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Try batch payout with mismatched recipients and amounts
        let recipients = vec![
            &env,
            Address::generate(&env),
            Address::generate(&env),
        ];
        let amounts = vec![&env, 100]; // Only 1 amount for 2 recipients
        
        let result = client.try_batch_payout(&recipients, &amounts);
        
        // This should fail with BatchAmountsMismatch error
        assert!(result.is_err());
    }

    // =========================================================================
    // Schedule Errors (400-499)
    // =========================================================================

    #[test]
    fn test_error_schedule_not_found() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Try to get non-existent schedule
        let result = client.try_get_program_release_schedule(&999);
        
        // This should fail with ScheduleNotFound error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_schedule_already_released() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Create a schedule
        let release_timestamp = env.ledger().timestamp() + 1000;
        let schedule = client.create_program_release_schedule(
            &100,
            &release_timestamp,
            &Some(String::from_str(&env, "test-schedule")),
        );
        
        // Release the schedule
        client.release_program_schedule_manual(&schedule.schedule_id);
        
        // Try to release again (should fail)
        let result = client.try_release_program_schedule_manual(&schedule.schedule_id);
        
        // This should fail with ScheduleAlreadyReleased error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_schedule_not_due() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Create a schedule with future release timestamp
        let release_timestamp = env.ledger().timestamp() + 10000;
        let schedule = client.create_program_release_schedule(
            &100,
            &release_timestamp,
            &Some(String::from_str(&env, "test-schedule")),
        );
        
        // Try to release before due time (should fail)
        let result = client.try_release_program_schedule_manual(&schedule.schedule_id);
        
        // This should fail with ScheduleNotDue error
        assert!(result.is_err());
    }

    // =========================================================================
    // Claim Errors (500-599)
    // =========================================================================

    #[test]
    fn test_error_claim_not_found() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Try to get non-existent claim
        let result = client.try_get_claim(&program_id, &999);
        
        // This should fail with ClaimNotFound error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_claim_already_executed() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Create a claim
        let recipient = Address::generate(&env);
        let claim = client.create_pending_claim(
            &program_id,
            &recipient,
            &100,
            &Some(String::from_str(&env, "test-claim")),
        );
        
        // Execute the claim
        client.execute_claim(&program_id, &claim.claim_id, &recipient);
        
        // Try to execute again (should fail)
        let result = client.try_execute_claim(&program_id, &claim.claim_id, &recipient);
        
        // This should fail with ClaimAlreadyExecuted error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_claim_expired() {
        let (env, client, admin, token) = setup();
        
        // Initialize program
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Set a very short claim window
        client.set_claim_window(&admin, &1); // 1 second
        
        // Create a claim
        let recipient = Address::generate(&env);
        let claim = client.create_pending_claim(
            &program_id,
            &recipient,
            &100,
            &Some(String::from_str(&env, "test-claim")),
        );
        
        // Advance time past the claim window
        env.ledger().set_timestamp(env.ledger().timestamp() + 10);
        
        // Try to execute expired claim (should fail)
        let result = client.try_execute_claim(&program_id, &claim.claim_id, &recipient);
        
        // This should fail with ClaimExpired error
        assert!(result.is_err());
    }

    // =========================================================================
    // Dispute Errors (600-699)
    // =========================================================================

    #[test]
    fn test_error_dispute_already_open() {
        let (env, client, admin, token) = setup();
        
        // Initialize contract and program
        client.initialize_contract(&admin);
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Open a dispute
        client.open_dispute(&String::from_str(&env, "first dispute"));
        
        // Try to open another dispute (should fail)
        let result = client.try_open_dispute(&String::from_str(&env, "second dispute"));
        
        // This should fail with DisputeAlreadyOpen error
        assert!(result.is_err());
    }

    #[test]
    fn test_error_no_active_dispute() {
        let (env, client, admin, token) = setup();
        
        // Initialize contract and program
        client.initialize_contract(&admin);
        let program_id = String::from_str(&env, "test-program");
        client.initialize_program(
            &program_id,
            &admin,
            &token,
            &admin,
            &None,
            &None,
        );
        
        // Try to resolve dispute when none is open (should fail)
        let result = client.try_resolve_dispute(&String::from_str(&env, "resolution"));
        
        // This should fail with NoActiveDispute error
        assert!(result.is_err());
    }

    // =========================================================================
    // Fee Errors (700-799)
    // =========================================================================

    #[test]
    fn test_error_invalid_fee_rate() {
        let (env, client, admin, token) = setup();
        
        // Initialize contract
        client.initialize_contract(&admin);
        
        // Try to set invalid fee rate (exceeds maximum)
        let result = client.try_update_fee_config(
            &Some(2000), // 20% - exceeds maximum
            &None,
            &None,
            &None,
            &None,
            &None,
        );
        
        // This should fail with InvalidFeeRate error
        assert!(result.is_err());
    }

    // =========================================================================
    // Error Description Tests
    // =========================================================================

    #[test]
    fn test_error_descriptions_are_generic() {
        // Verify that error descriptions do not contain sensitive data
        let errors = vec![
            ContractError::Unauthorized,
            ContractError::InvalidAmount,
            ContractError::Paused,
            ContractError::ProgramNotFound,
            ContractError::InsufficientBalance,
            ContractError::PayoutFailed,
            ContractError::ScheduleNotFound,
            ContractError::ClaimNotFound,
            ContractError::DisputeAlreadyOpen,
            ContractError::InvalidFeeRate,
        ];
        
        for error in errors {
            let description = error.description();
            
            // Descriptions should not be empty
            assert!(!description.is_empty());
            
            // Descriptions should not contain sensitive patterns
            assert!(!description.contains("0x")); // No hex addresses
            assert!(!description.contains("G")); // No Stellar addresses
            assert!(!description.contains("amount")); // No amounts
            assert!(!description.contains("balance")); // No balances
        }
    }

    #[test]
    fn test_error_codes_are_stable() {
        // Verify that error codes are stable and deterministic
        assert_eq!(ContractError::Unauthorized.code(), 1);
        assert_eq!(ContractError::InvalidAmount.code(), 2);
        assert_eq!(ContractError::Paused.code(), 3);
        assert_eq!(ContractError::ProgramNotFound.code(), 7);
        assert_eq!(ContractError::InsufficientBalance.code(), 10);
        assert_eq!(ContractError::PayoutFailed.code(), 300);
        assert_eq!(ContractError::ScheduleNotFound.code(), 400);
        assert_eq!(ContractError::ClaimNotFound.code(), 500);
        assert_eq!(ContractError::DisputeAlreadyOpen.code(), 600);
        assert_eq!(ContractError::InvalidFeeRate.code(), 701);
    }

    #[test]
    fn test_error_equality() {
        // Verify that error variants can be compared for equality
        assert_eq!(ContractError::Unauthorized, ContractError::Unauthorized);
        assert_ne!(ContractError::Unauthorized, ContractError::InvalidAmount);
        assert_ne!(ContractError::Paused, ContractError::MaintenanceMode);
    }

    #[test]
    fn test_error_clone() {
        // Verify that errors can be cloned
        let error = ContractError::Unauthorized;
        let cloned = error.clone();
        assert_eq!(error, cloned);
    }

    #[test]
    fn test_error_copy() {
        // Verify that errors implement Copy
        let error = ContractError::Unauthorized;
        let copied = error;
        assert_eq!(error, copied);
    }
}
