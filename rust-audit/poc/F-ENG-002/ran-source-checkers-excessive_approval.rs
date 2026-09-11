//! Detection of functionally unlimited token allowances.

use super::Checker;
use crate::{
    contracts::target_effects::{EffectKind, decode_target_effects},
    engine::{CheckContext, RuleId, SafeTransaction, Verdict},
};
use alloy::primitives::U256;

/// Denies functionally unlimited token approvals.
pub struct ExcessiveApprovalChecker;

#[async_trait::async_trait]
impl Checker for ExcessiveApprovalChecker {
    fn name(&self) -> &'static str {
        "excessive_approval"
    }

    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        for effect in decode_target_effects(transaction) {
            let unlimited = match effect.kind {
                EffectKind::Erc20Approval { amount } => amount == U256::MAX,
                EffectKind::OperatorApproval { approved } => approved,
                _ => false,
            };
            if unlimited {
                return Verdict::Insecure {
                    rule: RuleId::R4_5ExcessiveApproval,
                };
            }
        }
        Verdict::Abstain
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::{primitives::Address, sol_types::SolCall as _};

    alloy::sol! {
        function approve(address spender, uint256 amount) external;
        function setApprovalForAll(address operator, bool approved) external;
    }

    const TOKEN: Address = Address::new([1u8; 20]);
    const SPENDER: Address = Address::new([2u8; 20]);

    #[tokio::test]
    async fn denies_unlimited_erc20_approval() {
        let transaction = SafeTransaction {
            to: TOKEN,
            data: approveCall {
                spender: SPENDER,
                amount: U256::MAX,
            }
            .abi_encode()
            .into(),
            ..Default::default()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            }
        );
    }

    #[tokio::test]
    async fn abstains_on_bounded_erc20_approval() {
        let transaction = SafeTransaction {
            to: TOKEN,
            data: approveCall {
                spender: SPENDER,
                amount: U256::from(1_000u64),
            }
            .abi_encode()
            .into(),
            ..Default::default()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Abstain
        );
    }

    #[tokio::test]
    async fn denies_operator_approval_for_all() {
        let transaction = SafeTransaction {
            to: TOKEN,
            data: setApprovalForAllCall {
                operator: SPENDER,
                approved: true,
            }
            .abi_encode()
            .into(),
            ..Default::default()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            }
        );
    }

    #[tokio::test]
    async fn abstains_on_operator_approval_revocation() {
        let transaction = SafeTransaction {
            to: TOKEN,
            data: setApprovalForAllCall {
                operator: SPENDER,
                approved: false,
            }
            .abi_encode()
            .into(),
            ..Default::default()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Abstain
        );
    }
}
// PoC for F-ENG-002 — `ExcessiveApprovalChecker` denies every
// `setApprovalForAll(operator, true)`, a Charter-*conditional* case, so the
// reference engine deterministically votes `insecure` on routine honest
// NFT-marketplace listings.
//
// NOTE THE DIRECTION. Unlike the Criticals in this audit, this is a wrong
// vote in the DENYING direction, against honest traffic. The fixture below is
// an HONEST transaction that gets denied — the opposite shape. It is also the
// error direction that costs the engine's own operator money: see this
// directory's README on Charter § 2.15 and `SentinelOracleRequests.sol`.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/excessive_approval.rs`. It is a
// sibling of the existing `#[cfg(test)] mod tests`. F-ENG-036's PoC appends
// to the same file; the two modules do not collide and may both be applied.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_002
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_002 {
    use super::*;
    use crate::{
        contracts::bindings::erc721::setApprovalForAllCall,
        engine::{Operation, SafeTransaction},
    };
    use alloy::{
        primitives::{Address, address},
        sol_types::SolCall as _,
    };

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// A canonical ERC-721 collection the Safe holds.
    const COLLECTION: Address = address!("0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
    /// OpenSea's Seaport 1.6 conduit — the operator every listing flow
    /// approves. A canonical marketplace address, not an attacker's.
    const SEAPORT_CONDUIT: Address = address!("0x1E0049783F008A0085193E00003D00cd54003c71");

    /// The honest transaction: the mandatory first step of listing an NFT
    /// from a Safe on any major marketplace.
    fn marketplace_listing_approval() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: COLLECTION,
            value: U256::ZERO,
            data: setApprovalForAllCall {
                operator: SEAPORT_CONDUIT,
                approved: true,
            }
            .abi_encode()
            .into(),
            operation: Operation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    /// (1) PINS TODAY'S BEHAVIOUR. Passes on unfixed code.
    #[tokio::test]
    async fn poc_f_eng_002_denies_a_routine_marketplace_listing_today() {
        assert_eq!(
            ExcessiveApprovalChecker
                .check(&marketplace_listing_approval(), &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
            "unfixed behaviour: `EffectKind::OperatorApproval {{ approved }} => approved` denies on \
             the boolean alone, with no reference to the interaction the approval belongs to"
        );
    }

    /// (2) CONTROL — the ERC-20 arm is correct and must not be touched.
    ///
    /// Charter § 2.5 makes max-`uint256` ERC-20 approval *unconditionally*
    /// functionally unlimited, and R-4.5's "immediate failure" branch names
    /// exactly that case. `excessive_approval.rs:22` implements it exactly.
    /// Any fix for this finding that changes this test's outcome has
    /// over-corrected.
    #[tokio::test]
    async fn poc_f_eng_002_control_the_erc20_max_arm_stays_a_denial() {
        use crate::contracts::bindings::erc20::approveCall;

        let transaction = SafeTransaction {
            data: approveCall {
                spender: SEAPORT_CONDUIT,
                amount: U256::MAX,
            }
            .abi_encode()
            .into(),
            ..marketplace_listing_approval()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
        );
    }

    /// (3) CONTROL — revocation must stay non-denying. Passes today
    /// (implied by `excessive_approval.rs:23` but pinned by the existing
    /// suite too); restated here so the pair `approved: true` /
    /// `approved: false` is visible in one place.
    #[tokio::test]
    async fn poc_f_eng_002_control_revocation_abstains() {
        let transaction = SafeTransaction {
            data: setApprovalForAllCall {
                operator: SEAPORT_CONDUIT,
                approved: false,
            }
            .abi_encode()
            .into(),
            ..marketplace_listing_approval()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Abstain,
        );
    }

    /// (4) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// Charter § 2.5 makes an ERC-721/1155 operator approval functionally
    /// unlimited "unless plausibly required for the stated interaction",
    /// where the stated interaction is determined from onchain data (§ 2.8)
    /// and protocol-recorded purpose (§ 2.11). A *conditional* rule cannot be
    /// discharged by an unconditional denial, and R-4.5's immediate-failure
    /// branch names only the max-`uint256` ERC-20 case.
    ///
    /// The assertion is `!= Insecure` rather than `== Abstain` so that it is
    /// satisfied by remediation option 1 (abstain) **and** by options 2 and 3
    /// (deny only an unrecognised operator) — the fix must not be prejudged.
    ///
    /// Failure output on unfixed code:
    ///   assertion failed: verdict != Verdict::Insecure { rule: R4_5ExcessiveApproval }
    #[tokio::test]
    async fn poc_f_eng_002_a_conditional_rule_must_not_be_an_immediate_failure() {
        let verdict = ExcessiveApprovalChecker
            .check(&marketplace_listing_approval(), &CheckContext::default())
            .await;

        assert_ne!(
            verdict,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
            "§ 2.5 conditions operator approval-for-all on the stated interaction; the engine \
             has gathered no evidence about that interaction and must not deny on the boolean"
        );
    }
}
// PoC for F-ENG-036 — R-4.5 is implemented as an exact `U256::MAX`
// comparison, so `approve(X, 2^256-2)` evades it; and no allowance-increasing
// entry point other than `approve` is decoded at all.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/excessive_approval.rs`. F-ENG-002's
// PoC appends to the same file; the two modules do not collide and may both
// be applied.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_036
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_036 {
    use super::*;
    use crate::{
        contracts::bindings::erc20::approveCall,
        engine::{Operation, SafeTransaction},
    };
    use alloy::{
        primitives::{Address, address},
        sol_types::SolCall as _,
    };

    // `increaseAllowance` has no binding in `contracts/bindings.rs` — which
    // is the second half of this finding — so it is declared here.
    alloy::sol! {
        function increaseAllowance(address spender, uint256 addedValue);
    }

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    const USDC: Address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    /// A spender the Safe has interacted with before, so that after
    /// `ExcessiveApprovalChecker` abstains, `AddressPoisoningChecker` at
    /// position 10 finds an `ExactMatch` and answers `Secure` (F-ENG-033
    /// remediation option 2 is the other half of closing this).
    const SPENDER: Address = address!("0x1111111111111111111111111111111111111111");

    fn approve_tx(amount: U256) -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: USDC,
            value: U256::ZERO,
            data: approveCall {
                spender: SPENDER,
                amount,
            }
            .abi_encode()
            .into(),
            operation: Operation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    /// `U256::MAX - 1` = `2^256 - 2` = `0xffff…fffe`. One bit away from the
    /// only value the checker recognises.
    fn near_max() -> U256 {
        U256::MAX - U256::from(1u64)
    }

    /// (0) CONTROL — the exact-`MAX` case is denied. Must keep passing.
    /// The single-bit delta between this and test (1) is the whole finding.
    #[tokio::test]
    async fn poc_f_eng_036_control_exact_max_is_denied() {
        assert_eq!(
            ExcessiveApprovalChecker
                .check(&approve_tx(U256::MAX), &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
        );
    }

    /// (1) PINS TODAY'S BEHAVIOUR — a one-bit evasion.
    #[tokio::test]
    async fn poc_f_eng_036_abstains_one_bit_below_max_today() {
        assert_eq!(
            ExcessiveApprovalChecker
                .check(&approve_tx(near_max()), &CheckContext::default())
                .await,
            Verdict::Abstain,
            "unfixed behaviour: `amount == U256::MAX` is a bit-for-bit equality"
        );
    }

    /// (2) PINS TODAY'S BEHAVIOUR — `increaseAllowance` decodes to no effect
    /// at all, so there is nothing for the checker to examine.
    ///
    /// From a zero starting allowance, `increaseAllowance(spender, 2^256-1)`
    /// yields an allowance of `2^256-1`.
    #[tokio::test]
    async fn poc_f_eng_036_increase_allowance_produces_no_effect_today() {
        let transaction = SafeTransaction {
            data: increaseAllowanceCall {
                spender: SPENDER,
                addedValue: U256::MAX,
            }
            .abi_encode()
            .into(),
            ..approve_tx(U256::ZERO)
        };

        assert!(
            decode_target_effects(&transaction).is_empty(),
            "unfixed behaviour: `increaseAllowance` is absent from `decode_call`'s selector chain"
        );
        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Abstain,
        );
    }

    /// (3) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// Charter § 2.5: an approval is functionally unlimited if its amount
    /// "materially exceeds what is plausibly needed for the stated
    /// interaction", and — verbatim — "An approval can be functionally
    /// unlimited even if not technically max `uint256`." The max-`uint256`
    /// case is singled out only as the one needing no further analysis. The
    /// engine implements the shortcut and nothing else.
    ///
    /// Assertion is `!= Abstain` so that any of the remediation options
    /// (a `totalSupply` multiple, a fixed high threshold) satisfies it.
    #[tokio::test]
    async fn poc_f_eng_036_a_functionally_unlimited_approval_must_not_abstain() {
        assert_ne!(
            ExcessiveApprovalChecker
                .check(&approve_tx(near_max()), &CheckContext::default())
                .await,
            Verdict::Abstain,
            "§ 2.5: an approval can be functionally unlimited even if not technically max uint256"
        );
    }

    /// (4) THE SECOND HALF. **Expected to FAIL on unfixed code.**
    ///
    /// Satisfied only by remediation option 3 (add `increaseAllowance` — and
    /// arguably `permit` — to `contracts/bindings.rs` and to `decode_call`).
    /// Options 1 and 2 do not close it.
    #[tokio::test]
    async fn poc_f_eng_036_increase_allowance_must_decode_to_an_approval_effect() {
        let transaction = SafeTransaction {
            data: increaseAllowanceCall {
                spender: SPENDER,
                addedValue: U256::MAX,
            }
            .abi_encode()
            .into(),
            ..approve_tx(U256::ZERO)
        };

        assert!(
            decode_target_effects(&transaction)
                .iter()
                .any(|effect| matches!(effect.kind, EffectKind::Erc20Approval { .. })),
            "an allowance-increasing call must produce an approval effect"
        );
    }
}
