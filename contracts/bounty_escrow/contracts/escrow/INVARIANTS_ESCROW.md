## Monitoring & Verification

### On-Chain Verification

Two public view functions expose invariant health to external callers:
```rust
// Lightweight boolean check — suitable for watchtower polling
let healthy: bool = contract.verify_all_invariants();

// Detailed structured report
let report = contract.check_invariants();
// Returns InvariantReport with:
// - healthy: bool
// - sum_remaining: i128
// - token_balance: i128
// - per_escrow_failures: u32
// - orphaned_index_entries: u32
// - refund_inconsistencies: u32
// - violations: Vec<String>
```

Both functions are read-only and require no authorization. They may be
called at any ledger sequence without side effects.

### Hot-Path Assertions

INV-2 is verified automatically at every state-mutating boundary:

| Operation             | Assertion called                  |
|-----------------------|-----------------------------------|
| `lock_funds`          | `assert_after_lock`               |
| `lock_funds_anonymous`| `assert_after_lock`               |
| `publish`             | `assert_after_lock`               |
| `release_funds`       | `assert_after_disbursement`       |
| `refund`              | `assert_after_disbursement`       |
| `refund_resolved`     | `assert_after_disbursement`       |
| `batch_lock_funds`    | (validated via index + INV-1)     |
| `batch_release_funds` | `assert_after_disbursement`       |

### Off-Chain Monitoring
```typescript
// Watchtower service — poll every 5 minutes
async function monitorInvariants(contractId: string) {
    const healthy = await contract.verifyAllInvariants();

    if (!healthy) {
        const report = await contract.checkInvariants();
        alert(`CRITICAL: Invariant violation detected`);
        alert(`Sum remaining: ${report.sum_remaining}`);
        alert(`Token balance: ${report.token_balance}`);
        alert(`Per-escrow failures: ${report.per_escrow_failures}`);
        alert(`Orphaned entries: ${report.orphaned_index_entries}`);
        alert(`Refund inconsistencies: ${report.refund_inconsistencies}`);
        for (const v of report.violations) {
            alert(`Violation: ${v}`);
        }
    }
}
```

### Disabling Invariant Checks (Testing Only)

In test environments the `InvOff` instance storage flag can disable
hot-path assertions to allow deliberate state corruption in invariant
violation tests:
```rust
env.storage().instance().set(
    &soroban_sdk::Symbol::new(&env, "InvOff"),
    &true
);
```

This flag has no effect in production builds where the `#[cfg(test)]`
guard is not active.