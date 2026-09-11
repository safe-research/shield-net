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
