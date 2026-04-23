// Package errors provides a centralised mapping from on-chain contract error
// codes to human-readable messages.
package errors

import "fmt"

// ContractKind identifies which contract produced the error for diagnostic logging,
// though numeric codes are now unique across the entire project.
type ContractKind string

const (
	BountyEscrow   ContractKind = "bounty_escrow"
	Governance     ContractKind = "governance"
	CircuitBreaker ContractKind = "circuit_breaker"
	ProgramEscrow  ContractKind = "program_escrow"
)

type contractErrorEntry struct {
	Name    string // e.g. "AlreadyInitialized"
	Message string // human-readable explanation
}

// ---------------------------------------------------------------------------
// Per-contract Error Registry
// Keep these discriminants stable and in sync with the on-chain contracts.
// ---------------------------------------------------------------------------

var contractErrors = map[ContractKind]map[uint32]contractErrorEntry{
	BountyEscrow: {
		1:  {"AlreadyInitialized", "Contract is already initialized"},
		2:  {"NotInitialized", "Contract has not been initialized"},
		3:  {"Unauthorized", "Unauthorized: caller does not have permission"},
		4:  {"BountyNotFound", "Bounty not found"},
		5:  {"BountyAlreadyExists", "A bounty with this ID already exists"},
		6:  {"DeadlineNotPassed", "Deadline has not passed yet"},
		7:  {"NotAllowed", "Caller is not allowed to perform this operation"},
		8:  {"InvalidFeeRate", "Fee rate is invalid"},
		9:  {"InvalidDeadline", "Deadline is invalid (in the past or too far in the future)"},
		10: {"TicketNotFound", "Claim ticket not found"},
		11: {"TicketExpired", "Claim ticket has expired"},
		12: {"ClaimPending", "Operation blocked by a pending claim or dispute"},
		13: {"InvalidAmount", "Amount is invalid (must be greater than zero)"},
		14: {"InvalidState", "Contract is in an invalid state for this operation"},
		16: {"InsufficientFunds", "Insufficient funds for this operation"},
		17: {"NotApproved", "Operation has not been approved"},
		18: {"Paused", "Operations are currently paused"},
	},
	Governance: {
		1:  {"NotInitialized", "Contract has not been initialized"},
		2:  {"AlreadyInitialized", "Contract is already initialized"},
		3:  {"Unauthorized", "Unauthorized: caller does not have permission"},
		4:  {"ThresholdNotMet", "Governance threshold has not been reached"},
		5:  {"InvalidThreshold", "Governance threshold value is invalid"},
		6:  {"ProposalNotFound", "Proposal not found"},
		7:  {"ProposalNotActive", "Proposal is not currently active"},
		8:  {"VotingNotStarted", "Voting has not started yet for this proposal"},
		9:  {"VotingEnded", "Voting period has ended for this proposal"},
		10: {"ExecutionDelayNotMet", "Execution delay period has not elapsed yet"},
		11: {"AlreadyVoted", "You have already voted on this proposal"},
		12: {"ProposalNotApproved", "Proposal has not been approved"},
		13: {"Overflow", "Numeric overflow occurred during calculation"},
		14: {"ProposalExpired", "Proposal has expired"},
	},
	CircuitBreaker: {
		0:    {"ErrNone", "No error occurred during the operation"},
		1001: {"CircuitOpen", "Circuit breaker is open; operation rejected"},
		1002: {"TransferFailed", "Transfer failed due to underlying token error"},
		1003: {"InsufficientBalance", "Insufficient balance for the requested transfer"},
	},
}

func kindRegistry(kind ContractKind) (map[uint32]contractErrorEntry, bool) {
	reg, ok := contractErrors[kind]
	return reg, ok
}

// ContractErrorMessage returns a human-readable message for the given numeric error code.
func ContractErrorMessage(kind ContractKind, code uint32) string {
	reg, ok := kindRegistry(kind)
	if !ok {
		return fmt.Sprintf("Unknown contract kind %q (code %d)", kind, code)
	}
	if entry, ok := reg[code]; ok {
		return entry.Message
	}
	return fmt.Sprintf("Unknown %s contract error (code %d)", kind, code)
}

// ContractErrorName returns the Rust enum variant name for logging and debugging.
func ContractErrorName(kind ContractKind, code uint32) string {
	reg, ok := kindRegistry(kind)
	if !ok {
		return fmt.Sprintf("Unknown(%d)", code)
	}
	if entry, ok := reg[code]; ok {
		return entry.Name
	}
	return fmt.Sprintf("Unknown(%d)", code)
}

// AllCodes returns every registered numeric code for the given contract kind.
func AllCodes(kind ContractKind) []uint32 {
	reg, ok := kindRegistry(kind)
	if !ok {
		return nil
	}
	codes := make([]uint32, 0, len(reg))
	for c := range reg {
		codes = append(codes, c)
	}
	return codes
}
