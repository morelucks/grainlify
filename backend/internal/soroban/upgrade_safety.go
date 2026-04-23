package soroban

import (
	"context"
	"fmt"
)

// UpgradeSafetyReport represents the result of an upgrade safety check
type UpgradeSafetyReport struct {
	IsSafe       bool              `json:"is_safe"`
	ChecksPassed uint32            `json:"checks_passed"`
	ChecksFailed uint32            `json:"checks_failed"`
	Warnings     []UpgradeWarning `json:"warnings"`
	Errors       []UpgradeError   `json:"errors"`
}

// UpgradeWarning represents a warning during safety check
type UpgradeWarning struct {
	Code    uint32 `json:"code"`
	Message string `json:"message"`
}

// UpgradeError represents an error during safety check
type UpgradeError struct {
	Code    uint32 `json:"code"`
	Message string `json:"message"`
}

// SafetyCheckCodes defines the codes for each safety check
var SafetyCheckCodes = map[uint32]string{
	1001: "Storage Layout Compatibility",
	1002: "Contract Initialization",
	1003: "Escrow State Consistency",
	1004: "Pending Claims Verification",
	1005: "Admin Authority",
	1006: "Token Configuration",
	1007: "Feature Flags Readiness",
	1008: "Reentrancy Lock",
	1009: "Version Compatibility",
	1010: "Balance Sanity",
}

// UpgradeSafetyClient provides methods for upgrade safety checks
type UpgradeSafetyClient struct {
	client        *Client
	contractAddr  string
	sourceSecret  string
	retryConfig   RetryConfig
}

// NewUpgradeSafetyClient creates a new upgrade safety client
func NewUpgradeSafetyClient(client *Client, contractAddress string, sourceSecret string) *UpgradeSafetyClient {
	return &UpgradeSafetyClient{
		client:       client,
		contractAddr: contractAddress,
		sourceSecret: sourceSecret,
		retryConfig:  DefaultRetryConfig(),
	}
}

// SimulateUpgrade performs a dry-run of the upgrade safety checks
// This does not modify any state but validates all pre-conditions
func (u *UpgradeSafetyClient) SimulateUpgrade(ctx context.Context) (*UpgradeSafetyReport, error) {
	// NOTE: The current Soroban transaction submission flow in this repo uses Horizon
	// submission and does not expose per-operation return values from simulation.
	// Until we plumb Soroban RPC simulation results through, treat this as unsupported.
	_ = ctx
	return &UpgradeSafetyReport{
		IsSafe:       false,
		ChecksPassed: 0,
		ChecksFailed: 1,
		Errors: []UpgradeError{
			{Code: 0, Message: "Upgrade safety simulation is not supported by this backend build"},
		},
	}, nil
}

// ValidateUpgrade performs the actual upgrade with safety checks
// This will fail if any safety check fails
func (u *UpgradeSafetyClient) ValidateUpgrade(ctx context.Context, newWasmHash uint32) error {
	// First, run safety simulation
	report, err := u.SimulateUpgrade(ctx)
	if err != nil {
		return fmt.Errorf("safety check failed: %w", err)
	}

	if !report.IsSafe {
		return fmt.Errorf("upgrade safety checks failed: %d errors, %d warnings",
			len(report.Errors), len(report.Warnings))
	}

	// This method is intentionally conservative: if safety checks aren't supported,
	// ValidateUpgrade will never attempt an on-chain upgrade.
	_ = newWasmHash
	return nil
}

// GetUpgradeSafetyStatus checks if safety checks are enabled
func (u *UpgradeSafetyClient) GetUpgradeSafetyStatus(ctx context.Context) (bool, error) {
	_ = ctx
	return false, nil
}

// SetUpgradeSafety enables or disables safety checks
func (u *UpgradeSafetyClient) SetUpgradeSafety(ctx context.Context, enabled bool) error {
	_ = ctx
	_ = enabled
	return fmt.Errorf("set upgrade safety is not supported by this backend build")
}

// FormatSafetyReport creates a human-readable string from the report
func FormatSafetyReport(report *UpgradeSafetyReport) string {
	var status string
	if report.IsSafe {
		status = "✓ SAFE TO UPGRADE"
	} else {
		status = "✗ UNSAFE TO UPGRADE"
	}

	output := fmt.Sprintf(`
══════════════════════════════════════════════════════════════════
  UPGRADE SAFETY REPORT
══════════════════════════════════════════════════════════════════
  Status: %s
  Checks Passed: %d
  Checks Failed: %d
══════════════════════════════════════════════════════════════════
`, status, report.ChecksPassed, report.ChecksFailed)

	if len(report.Errors) > 0 {
		output += "\nERRORS:\n"
		for _, err := range report.Errors {
			name := SafetyCheckCodes[err.Code]
			if name == "" {
				name = "Unknown"
			}
			output += fmt.Sprintf("  [%d] %s: %s\n", err.Code, name, err.Message)
		}
	}

	if len(report.Warnings) > 0 {
		output += "\nWARNINGS:\n"
		for _, warn := range report.Warnings {
			name := SafetyCheckCodes[warn.Code]
			if name == "" {
				name = "Unknown"
			}
			output += fmt.Sprintf("  [%d] %s: %s\n", warn.Code, name, warn.Message)
		}
	}

	output += "\n══════════════════════════════════════════════════════════════════\n"

	return output
}
