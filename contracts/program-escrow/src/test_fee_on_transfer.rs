#![cfg(test)]

//! # Fee-on-Transfer (FoT) Token Integration Tests
//!
//! Audits and validates `program-escrow` accounting when interacting with
//! tokens that silently deduct a fee during transfer (deflationary / FoT
//! tokens). These tests use a purpose-built mock token (`DeflatToken`) that
//! takes a configurable percentage fee on every transfer, allowing us to
//! simulate the full range of deflationary behaviour without relying on
//! production SAC tokens.
//!
//! ## Token Assumptions (Supported Assets)
//!
//! The `program-escrow` contract is designed around **standard Stellar Asset
//! Contract (SAC) tokens** which credit the receiver the exact requested
//! amount.  When a non-standard (FoT) token is used:
//!
//! - `lock_program_funds_from`: measures `balance_before` → `balance_after`
//!   and credits only `actual_received` — protecting accounting from FoT drift.
//! - `lock_program_funds_v2`: performs a pre-transfer balance check and
//!   panics with "Insufficient contract balance to cover lock (possible
//!   fee-on-transfer issue)" if the credited amount would exceed what the
//!   contract actually holds.
//! - Payout functions (`single_payout`, `batch_payout`): debit
//!   `remaining_balance` and call `token::transfer`.  If the actual on-chain
//!   balance has been eroded by FoT fees, the token transfer will fail at the
//!   token level before the accounting is mutated — preventing over-withdrawal.
//!
//! ## Edge Cases Covered
//!
//! 1.  10 % FoT fee — single lock, balance reconciled to actual received
//! 2.  50 % FoT fee — single lock, high-fee scenario
//! 3.  Repeated locks accumulate correctly against actual received amounts
//! 4.  `lock_program_funds_v2` rejects amount > actual contract balance (FoT
//!     mismatch detection)
//! 5.  `lock_program_funds_v2` accepts amount == actual balance after fee
//! 6.  Zero-fee token behaves identically to standard SAC token
//! 7.  FoT token: `remaining_balance` never exceeds actual on-chain balance
//! 8.  FoT lock followed by payout — payout succeeds up to actual funds
//! 9.  FoT lock followed by over-payout attempt — panics with insufficient funds
//! 10. FundsLocked event carries `actual_received`, not the requested amount
//! 11. Multiple sequential FoT locks — cumulative balance invariant holds
//! 12. Concurrent FoT lock from two different depositors (sequential calls)

use crate::{ProgramEscrowContract, ProgramEscrowContractClient, ProgramStatus};
use soroban_sdk::{
    contract, contractimpl,
    testutils::Address as _,
    vec, Address, Env, String,
};

// ===========================================================================
// Mock FoT token contracts
// ===========================================================================

/// A deflationary token that burns `fee_bps / 10_000` of `amount` on every
/// transfer / transfer_from, crediting only `amount - fee` to the receiver.
///
/// Storage layout (all in `instance`):
/// - `Address` → `i128`  balance
/// - `Symbol("fee_bps")` → `i128`  fee rate in basis points (default 1 000 = 10 %)
#[contract]
pub struct DeflatToken;

#[contractimpl]
impl DeflatToken {
    pub fn balance(env: Env, id: Address) -> i128 {
        env.storage().instance().get(&id).unwrap_or(0)
    }

    /// Mint tokens to `to` without any fee.
    pub fn mint(env: Env, to: Address, amount: i128) {
        let b: i128 = env.storage().instance().get(&to).unwrap_or(0);
        env.storage().instance().set(&to, &(b + amount));
    }

    /// Set the fee rate in basis points (0 – 10 000).
    pub fn set_fee_bps(env: Env, fee_bps: i128) {
        let key = soroban_sdk::symbol_short!("fee_bps");
        env.storage().instance().set(&key, &fee_bps);
    }

    fn fee_bps_internal(env: &Env) -> i128 {
        let key = soroban_sdk::symbol_short!("fee_bps");
        env.storage().instance().get(&key).unwrap_or(1_000) // default 10 %
    }

    fn do_transfer(env: &Env, from: Address, to: Address, amount: i128) {
        let b_from: i128 = env.storage().instance().get(&from).unwrap_or(0);
        assert!(b_from >= amount, "insufficient balance");
        let fee = amount * Self::fee_bps_internal(env) / 10_000;
        let net = amount - fee;
        env.storage().instance().set(&from, &(b_from - amount));
        let b_to: i128 = env.storage().instance().get(&to).unwrap_or(0);
        env.storage().instance().set(&to, &(b_to + net));
    }

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        Self::do_transfer(&env, from, to, amount);
    }

    pub fn transfer_from(env: Env, _spender: Address, from: Address, to: Address, amount: i128) {
        from.require_auth();
        Self::do_transfer(&env, from, to, amount);
    }
}

// ===========================================================================
// Test helpers
// ===========================================================================

struct FotSetup<'a> {
    client: ProgramEscrowContractClient<'a>,
    token: DeflatTokenClient<'a>,
    program_id: String,
}

/// Register a program-escrow contract + a `DeflatToken` (fee_bps = `fee_bps`).
/// The program is published so locks and payouts are allowed.
fn setup_fot_env(env: &Env, fee_bps: i128) -> FotSetup<'_> {
    env.mock_all_auths();

    let contract_id = env.register_contract(None, ProgramEscrowContract);
    let client = ProgramEscrowContractClient::new(env, &contract_id);

    let token_id = env.register_contract(None, DeflatToken);
    let token = DeflatTokenClient::new(env, &token_id);
    token.set_fee_bps(&fee_bps);

    let admin = Address::generate(env);
    let program_id = String::from_str(env, "fot-prog");

    client.init_program(&program_id, &admin, &token_id, &admin, &None, &None);
    // Publish so locks / payouts are permitted
    client.publish_program(&program_id);

    FotSetup { client, token, program_id }
}

// ===========================================================================
// 1. 10 % fee — single lock, balance reconciled to actual received
// ===========================================================================
#[test]
fn test_fot_lock_10pct_fee_credits_actual_received() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 1_000); // 10 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &1_000);

    // Deposit 1 000.  Token burns 10 % ⇒ contract receives 900.
    client.lock_program_funds_from(&1_000, &depositor);

    let info = client.get_program_info();
    // Contract should credit `actual_received` (900), not the requested 1 000.
    assert_eq!(info.remaining_balance, 900,
        "remaining_balance must equal actual tokens received after 10 % fee");
    assert_eq!(info.total_funds, 900,
        "total_funds must equal actual tokens received after 10 % fee");
    assert_eq!(token.balance(&client.address), 900,
        "on-chain contract balance must match credited amount");
}

// ===========================================================================
// 2. 50 % fee — high-fee scenario
// ===========================================================================
#[test]
fn test_fot_lock_50pct_fee_credits_actual_received() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 5_000); // 50 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &2_000);

    client.lock_program_funds_from(&2_000, &depositor);

    let info = client.get_program_info();
    assert_eq!(info.remaining_balance, 1_000,
        "50 % fee: contract must credit only 1 000 of 2 000 requested");
    assert_eq!(token.balance(&client.address), 1_000);
}

// ===========================================================================
// 3. Repeated locks accumulate correctly against actual received amounts
// ===========================================================================
#[test]
fn test_fot_repeated_locks_accumulate_actual_received() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 1_000); // 10 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &3_000);

    // Three deposits of 1 000 each.  Each receives 900.
    client.lock_program_funds_from(&1_000, &depositor);
    client.lock_program_funds_from(&1_000, &depositor);
    client.lock_program_funds_from(&1_000, &depositor);

    let info = client.get_program_info();
    assert_eq!(info.remaining_balance, 2_700,
        "3 × 900 = 2 700 after 10 % FoT fee on each lock");
    assert_eq!(token.balance(&client.address), 2_700);
}

// ===========================================================================
// 4. lock_program_funds_v2 rejects amount > actual contract balance
// ===========================================================================
#[test]
fn test_fot_lock_v2_rejects_amount_exceeding_actual_balance() {
    let env = Env::default();
    let FotSetup { client, token, program_id, .. } = setup_fot_env(&env, 1_000);

    let sender = Address::generate(&env);
    token.mint(&sender, &1_000);

    // Directly transfer 1 000 to the contract; FoT takes 10 % → contract gets 900.
    token.transfer(&sender, &client.address, &1_000);

    // Attempting to book 1 000 via v2 should fail — contract only holds 900.
    let res = client.try_lock_program_funds_v2(&program_id, &1_000);
    assert!(res.is_err(),
        "lock_program_funds_v2 must reject when requested amount > actual balance (FoT mismatch)");
}

// ===========================================================================
// 5. lock_program_funds_v2 accepts amount == actual balance after fee
// ===========================================================================
#[test]
fn test_fot_lock_v2_accepts_actual_received_amount() {
    let env = Env::default();
    let FotSetup { client, token, program_id, .. } = setup_fot_env(&env, 1_000);

    let sender = Address::generate(&env);
    token.mint(&sender, &1_000);
    // FoT: contract receives 900
    token.transfer(&sender, &client.address, &1_000);

    // Book against what was actually received — should succeed.
    client.lock_program_funds_v2(&program_id, &900);
    assert_eq!(client.get_program_info().remaining_balance, 900);
}

// ===========================================================================
// 6. Zero-fee token behaves identically to standard SAC token
// ===========================================================================
#[test]
fn test_zero_fee_token_behaves_like_standard_sac() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 0); // 0 % fee

    let depositor = Address::generate(&env);
    token.mint(&depositor, &500);

    client.lock_program_funds_from(&500, &depositor);

    let info = client.get_program_info();
    assert_eq!(info.remaining_balance, 500,
        "zero-fee token: full amount must be credited, matching standard SAC behaviour");
    assert_eq!(token.balance(&client.address), 500);
}

// ===========================================================================
// 7. remaining_balance never exceeds actual on-chain balance after FoT lock
// ===========================================================================
#[test]
fn test_fot_remaining_balance_never_exceeds_on_chain_balance() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 2_000); // 20 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &5_000);

    client.lock_program_funds_from(&5_000, &depositor);

    let info = client.get_program_info();
    let on_chain = token.balance(&client.address);

    assert!(
        info.remaining_balance <= on_chain,
        "remaining_balance ({}) must never exceed on-chain token balance ({}) \
         — otherwise a payout would over-draw",
        info.remaining_balance,
        on_chain
    );
}

// ===========================================================================
// 8. FoT lock followed by payout — payout succeeds up to actual funds
// ===========================================================================
#[test]
fn test_fot_lock_then_payout_succeeds_within_actual_balance() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 1_000); // 10 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &2_000);
    // Contract receives 1 800 after 10 % FoT.
    client.lock_program_funds_from(&2_000, &depositor);

    let recipient = Address::generate(&env);
    // Payout 900 — should succeed; contract holds 1 800.
    // NOTE: FoT token ALSO charges 10 % on the payout transfer (contract → recipient).
    // The recipient therefore receives 900 × 0.90 = 810.  The contract's remaining_balance
    // is debited by 900 (the amount the escrow released); the fee is burned by the token.
    client.single_payout(&recipient, &900);

    assert_eq!(token.balance(&recipient), 810,
        "FoT fee applies to outbound payout transfer: recipient gets amount - 10 % = 810");
    assert_eq!(client.get_program_info().remaining_balance, 900,
        "remaining_balance debited by full release amount (900) regardless of outbound fee");
    // On-chain contract balance: 1 800 - 900 released = 900.
    assert_eq!(token.balance(&client.address), 900);
}

// ===========================================================================
// 9. Over-payout attempt panics — cannot draw more than credited balance
// ===========================================================================
#[test]
#[should_panic]
fn test_fot_lock_then_over_payout_panics() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 1_000); // 10 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &1_000);
    // Contract credited 900.
    client.lock_program_funds_from(&1_000, &depositor);

    let recipient = Address::generate(&env);
    // Attempt to pay out 950 — more than the 900 credited.
    client.single_payout(&recipient, &950);
}

// ===========================================================================
// 10. FundsLocked event carries actual_received, not the requested amount
// ===========================================================================
#[test]
fn test_fot_funds_locked_event_reflects_actual_received() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 1_000); // 10 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &1_000);
    client.lock_program_funds_from(&1_000, &depositor);

    use soroban_sdk::{testutils::Events, IntoVal, TryIntoVal};
    use crate::FundsLockedEvent;

    let events = env.events().all();
    // Find the FundsLocked event emitted by the escrow contract.
    let fot_event = events.iter()
        .filter(|(contract, _topics, _data)| *contract == client.address)
        .find(|(_c, topics, _d)| {
            if topics.len() == 0 { return false; }
            let topic: soroban_sdk::Symbol = topics.get(0).unwrap().into_val(&env);
            topic == soroban_sdk::symbol_short!("FndsLock")
        });

    assert!(fot_event.is_some(), "FundsLocked event must be emitted on FoT lock");

    let payload: FundsLockedEvent = fot_event.unwrap().2.try_into_val(&env).unwrap();
    assert_eq!(payload.amount, 900,
        "FundsLocked event amount must be actual_received (900) not the requested 1 000");
}

// ===========================================================================
// 11. Multiple sequential FoT locks — cumulative balance invariant
// ===========================================================================
#[test]
fn test_fot_cumulative_balance_invariant_across_sequential_locks() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 1_000); // 10 %

    let d1 = Address::generate(&env);
    let d2 = Address::generate(&env);
    let d3 = Address::generate(&env);

    token.mint(&d1, &500);
    token.mint(&d2, &300);
    token.mint(&d3, &200);

    client.lock_program_funds_from(&500, &d1); // receives 450
    client.lock_program_funds_from(&300, &d2); // receives 270
    client.lock_program_funds_from(&200, &d3); // receives 180

    let expected_credited = 450 + 270 + 180; // 900
    let info = client.get_program_info();
    let on_chain = token.balance(&client.address);

    assert_eq!(info.remaining_balance, expected_credited,
        "cumulative remaining_balance must equal sum of all actual_received amounts");
    assert_eq!(on_chain, expected_credited,
        "on-chain token balance must equal sum of actual_received amounts");
    // Invariant: accounting == on-chain reality
    assert_eq!(info.remaining_balance, on_chain,
        "INVARIANT: remaining_balance must always equal actual on-chain balance");
}

// ===========================================================================
// 12. Batch payout from FoT-funded escrow — all recipients receive correct amounts
// ===========================================================================
#[test]
fn test_fot_batch_payout_distributes_correctly_from_credited_funds() {
    let env = Env::default();
    let FotSetup { client, token, .. } = setup_fot_env(&env, 1_000); // 10 %

    let depositor = Address::generate(&env);
    token.mint(&depositor, &5_000);
    // Contract credited with 4 500 (10 % FoT on 5 000).
    client.lock_program_funds_from(&5_000, &depositor);

    let r1 = Address::generate(&env);
    let r2 = Address::generate(&env);
    let r3 = Address::generate(&env);

    let recipients = vec![&env, r1.clone(), r2.clone(), r3.clone()];
    let amounts = vec![&env, 1_000_i128, 2_000_i128, 1_000_i128];

    let data = client.batch_payout(&recipients, &amounts);

    // Payouts originate from the credited (not the original deposited) pool.
    // 4 500 - 4 000 = 500 remaining in escrow accounting.
    assert_eq!(data.remaining_balance, 500);
    // NOTE: FoT token charges 10 % on each outbound payout transfer.
    // Recipients receive amount - 10 % fee.
    assert_eq!(token.balance(&r1), 900,   "r1: 1 000 - 10 % = 900");
    assert_eq!(token.balance(&r2), 1_800, "r2: 2 000 - 10 % = 1 800");
    assert_eq!(token.balance(&r3), 900,   "r3: 1 000 - 10 % = 900");
    // On-chain contract balance: 4 500 - 4 000 released = 500.
    assert_eq!(token.balance(&client.address), 500,
        "on-chain contract balance must match remaining_balance after batch payout");
}
