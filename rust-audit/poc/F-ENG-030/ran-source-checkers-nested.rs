//! Recognition of nested Safe transactions: a Safe calling another
//! contract's `execTransaction`.
//!
//! Article IV Part A already lets a Safe call any other contract freely —
//! only self-calls and delegatecalls are restricted (see
//! [`crate::checkers::BaseChecker`]). Calling another Safe's
//! `execTransaction` is just such a call: whatever the nested transaction
//! does is that Safe's own guard's concern (if it has one), not this
//! transaction's, so it's secure independent of the nested transaction's own
//! content. Runs after [`crate::checkers::BlocklistChecker`] so a nested call
//! to a known malicious `to` is still denied rather than short-circuited.

use super::Checker;
use crate::{
    contracts::bindings::safe,
    engine::{CheckContext, Operation, SafeTransaction, Verdict},
};
use alloy::sol_types::SolCall as _;

/// Considers a call to another contract's `execTransaction` secure,
/// regardless of the nested transaction it carries.
pub struct NestedSafeChecker;

#[async_trait::async_trait]
impl Checker for NestedSafeChecker {
    fn name(&self) -> &'static str {
        "nested_safe"
    }

    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        if is_nested_exec_transaction(transaction) {
            Verdict::Secure
        } else {
            Verdict::Abstain
        }
    }
}

/// A `Call` (never a delegatecall) to a different address, carrying
/// `execTransaction` calldata for that address to decode and enforce on its
/// own terms.
fn is_nested_exec_transaction(tx: &SafeTransaction) -> bool {
    tx.operation == Operation::Call
        && tx.to != tx.safe
        && tx.data.starts_with(&safe::execTransactionCall::SELECTOR)
        && safe::execTransactionCall::abi_decode(&tx.data).is_ok()
}
// PoC for F-ENG-030 — `NestedSafeChecker` rates any `execTransaction`-shaped
// call `secure` while ignoring `value`.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/nested.rs`. That file has no
// `#[cfg(test)]` module today, so nothing is overwritten.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_030
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_030 {
    use super::*;
    use alloy::{
        primitives::{Address, Bytes, U256, address},
        sol_types::SolCall as _,
    };

    /// The victim Safe.
    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// The attacker's own address. A plain EOA: it is never probed, so it
    /// need not be a Safe, need not implement `execTransaction`, and need not
    /// be a contract at all.
    const ATTACKER: Address = address!("0x000000000000000000000000000000000000dEaD");
    /// 1000 ETH (`0x3635c9adc5dea00000` wei) — stands in for "the Safe's
    /// entire native balance".
    fn drain_amount() -> U256 {
        U256::from(1_000u64) * U256::from(10u64).pow(U256::from(18u64))
    }

    /// ABI-well-formed `execTransaction` calldata whose arguments are all
    /// inert. `is_nested_exec_transaction` calls `abi_decode` (`nested.rs:46`)
    /// and then discards every decoded value, so the arguments are free — but
    /// the encoding must be *complete*: a bare 4-byte selector with a
    /// truncated tail makes `abi_decode` fail and the checker abstains, and
    /// the vector would then silently test nothing.
    fn exec_transaction_calldata() -> Bytes {
        Bytes::from(
            safe::execTransactionCall {
                to: Address::ZERO,
                value: U256::ZERO,
                data: Bytes::new(),
                operation: 0u8,
                safeTxGas: U256::ZERO,
                baseGas: U256::ZERO,
                gasPrice: U256::ZERO,
                gasToken: Address::ZERO,
                refundReceiver: Address::ZERO,
                signatures: Bytes::new(),
            }
            .abi_encode(),
        )
    }

    /// The attacker's transaction: a plain `CALL` to their own EOA carrying
    /// the Safe's whole balance, dressed as a nested-Safe execution.
    fn native_drain() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: ATTACKER,
            value: drain_amount(),
            data: exec_transaction_calldata(),
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
    async fn poc_f_eng_030_affirms_a_full_native_drain_today() {
        assert_eq!(
            NestedSafeChecker
                .check(&native_drain(), &CheckContext::default())
                .await,
            Verdict::Secure,
            "unfixed behaviour: `value` is not part of the affirmation predicate"
        );
    }

    /// (2) CONTROL — the fixture is well-formed.
    ///
    /// Proves the affirmation really came from the nested-exec predicate and
    /// not from a decode failure: truncating the calldata to the bare
    /// selector must abstain. If this test fails, test (1)'s `Secure` is
    /// meaningless and the fixture must be repaired before anything else is
    /// believed.
    #[tokio::test]
    async fn poc_f_eng_030_a_truncated_encoding_abstains() {
        let truncated = SafeTransaction {
            data: Bytes::from(safe::execTransactionCall::SELECTOR.to_vec()),
            ..native_drain()
        };

        assert_eq!(
            NestedSafeChecker
                .check(&truncated, &CheckContext::default())
                .await,
            Verdict::Abstain,
        );
    }

    /// (3) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// Charter R-4.3: "A transaction is insecure if it sends value to a
    /// recipient address outside the expected target set." This transaction
    /// sends the Safe's whole balance to an address the engine knows nothing
    /// about. Any fix — denying under R-4.3, or (option 2) abstaining because
    /// a nested `execTransaction` is a reason not to deny rather than
    /// evidence of security — makes this pass.
    ///
    /// Failure output on unfixed code:
    ///   assertion failed: verdict != Verdict::Secure
    #[tokio::test]
    async fn poc_f_eng_030_a_value_bearing_call_must_not_be_affirmed() {
        let verdict = NestedSafeChecker
            .check(&native_drain(), &CheckContext::default())
            .await;

        assert_ne!(
            verdict,
            Verdict::Secure,
            "R-4.3: value sent to an unvetted recipient cannot be affirmed on the strength of \
             the calldata's selector alone"
        );
    }

    /// (4) THE REFUND LEG, same checker. **Expected to FAIL on unfixed code.**
    ///
    /// This is F-ENG-031's slice of `nested.rs`: zero `value`, but a refund
    /// that pays `(gasUsed + 1e12) * 1 gwei` to the attacker. Kept here as
    /// well as in F-ENG-031's PoC because a fix that only adds
    /// `value.is_zero()` (remediation option 1's first half) passes test (3)
    /// and still fails this one.
    #[tokio::test]
    async fn poc_f_eng_030_a_relayed_call_must_not_be_affirmed_either() {
        let relayed = SafeTransaction {
            value: U256::ZERO,
            base_gas: U256::from(1_000_000_000_000u64),
            gas_price: U256::from(1_000_000_000u64),
            refund_receiver: ATTACKER,
            ..native_drain()
        };

        assert_ne!(
            NestedSafeChecker
                .check(&relayed, &CheckContext::default())
                .await,
            Verdict::Secure,
            "the gas-refund leg is a value transfer to an attacker-chosen address; \
             `EscapeHatchChecker` already refuses to affirm a relayed call (escape_hatch.rs:53)"
        );
    }
}
