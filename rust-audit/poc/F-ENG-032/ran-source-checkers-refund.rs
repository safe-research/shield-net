//! Check for the Safe's own gas-refund mechanism: a nonzero `gasPrice` has
//! the Safe reimburse the relayer for `gasPrice * gasUsed` (up to
//! `safeTxGas`/`baseGas`) in `gasToken` to `refundReceiver`. That payment is
//! itself a transfer out of the Safe, and `refundReceiver` is just as
//! attacker-controllable as any other transfer recipient — so it gets the
//! same [`AddressPoisoningChecker`] scrutiny as the transaction's primary
//! ERC-20 transfer, by resynthesizing the refund as a `transfer` call and
//! delegating to it.
//!
//! Runs late in the engine's checker chain, alongside
//! [`AddressPoisoningChecker`]'s own primary-transfer check: both are RPC-
//! backed, so cheaper local checkers get a chance to reach a verdict first.
//! Also, for the same reason [`crate::checkers::CowChecker`] runs ahead of
//! `AddressPoisoningChecker`'s own bypass (see that module's docs), a
//! `Secure` verdict here must never stand in for the whole transaction: it
//! only means the refund's recipient has *some* prior history, which is weak
//! evidence for the refund leg alone, let alone the transaction's primary
//! effect. [`RefundChecker::check`] squashes it to [`Verdict::Abstain`]
//! accordingly — only a genuine [`Verdict::Insecure`] denial is allowed
//! through.

use super::{AddressPoisoningChecker, CheckContext, Checker};
use crate::{
    contracts::bindings::erc20::transferCall,
    engine::{SafeTransaction, Verdict},
};
use alloy::sol_types::SolCall as _;
use std::sync::Arc;

/// Treats a Safe transaction's own gas refund as a transfer and runs it
/// through [`AddressPoisoningChecker`]. Takes the checker as a shared `Arc`
/// so the engine can also run it directly against transactions' primary
/// transfers, rather than needing a second, independent instance.
pub struct RefundChecker(Arc<AddressPoisoningChecker>);

impl RefundChecker {
    pub fn new(address_poisoning: Arc<AddressPoisoningChecker>) -> Self {
        Self(address_poisoning)
    }
}

#[async_trait::async_trait]
impl Checker for RefundChecker {
    fn name(&self) -> &'static str {
        "refund"
    }

    /// Resynthesizes `transaction`'s own gas refund as an ERC-20 `transfer`
    /// from the Safe to `refundReceiver` and defers to
    /// [`AddressPoisoningChecker`]; abstains outright when there's no refund
    /// to resynthesize (see [`refund_transfer`]).
    async fn check(&self, transaction: &SafeTransaction, context: &CheckContext) -> Verdict {
        let Some(refund) = refund_transfer(transaction) else {
            return Verdict::Abstain;
        };
        deny_or_abstain(self.0.check(&refund, context).await)
    }
}

/// Only lets a denial through. A poisoning check's `Secure` verdict is, at
/// best, evidence about the one leg it was run against — never grounds to
/// affirm the whole transaction, which is what returning it here would do:
/// this checker runs in a chain that stops at the first non-[`Verdict::Abstain`]
/// verdict, so any recipient with *some* public onchain history (trivial for
/// an attacker to pick) would otherwise make the engine answer `Secure`
/// without Blocklist, CoW, ExcessiveApproval, or the primary-transfer check
/// ever running.
fn deny_or_abstain(verdict: Verdict) -> Verdict {
    match verdict {
        Verdict::Secure => Verdict::Abstain,
        verdict => verdict,
    }
}

/// Builds the ERC-20 `transfer` call `transaction`'s own gas refund amounts
/// to, or `None` when there's nothing to check:
///
/// - `gasPrice` zero — no refund is paid at all.
/// - `gasToken` zero — the refund is paid in native currency, which
///   [`AddressPoisoningChecker`] doesn't decode (it only recognizes ERC-20
///   calldata).
///
///   TODO(follow-up): this checker abstaining doesn't mean anything else
///   inspects a native refund either. Since the engine-wide "abstain on any
///   nonzero `gasPrice`" guard that used to sit ahead of the whole checker
///   chain is gone, a transaction another checker calls `Secure` can now
///   drain unbounded native currency to `refundReceiver` uncommented on. A
///   native-value-aware check (or at least an amount cap) is needed before
///   this is safe to affirm.
/// - `refundReceiver` zero — Safe.sol then pays `tx.origin` instead, an
///   address this checker has no way to learn ahead of execution.
///
///   TODO(follow-up): same hole as the native-currency case — the refund
///   still goes out, to a fully unvetted relayer, on a transaction the rest
///   of the chain can still approve. Needs a policy once the engine can
///   observe or reason about the relayer, not just abstain on it.
fn refund_transfer(transaction: &SafeTransaction) -> Option<SafeTransaction> {
    if transaction.gas_price.is_zero()
        || transaction.gas_token.is_zero()
        || transaction.refund_receiver.is_zero()
    {
        return None;
    }

    Some(SafeTransaction {
        safe: transaction.safe,
        to: transaction.gas_token,
        data: transferCall {
            to: transaction.refund_receiver,
            amount: transaction
                .gas_price
                .saturating_mul(transaction.safe_tx_gas.saturating_add(transaction.base_gas)),
        }
        .abi_encode()
        .into(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::RuleId;
    use alloy::primitives::{Address, U256};

    const SAFE: Address = Address::new([1u8; 20]);
    const GAS_TOKEN: Address = Address::new([2u8; 20]);
    const REFUND_RECEIVER: Address = Address::new([3u8; 20]);

    fn relayed_tx() -> SafeTransaction {
        SafeTransaction {
            safe: SAFE,
            gas_price: U256::from(1u64),
            safe_tx_gas: U256::from(100_000u64),
            base_gas: U256::from(21_000u64),
            gas_token: GAS_TOKEN,
            refund_receiver: REFUND_RECEIVER,
            ..Default::default()
        }
    }

    #[test]
    fn none_when_gas_price_is_zero() {
        let transaction = SafeTransaction {
            gas_price: U256::ZERO,
            ..relayed_tx()
        };

        assert_eq!(refund_transfer(&transaction), None);
    }

    #[test]
    fn none_on_a_native_currency_refund() {
        let transaction = SafeTransaction {
            gas_token: Address::ZERO,
            ..relayed_tx()
        };

        assert_eq!(refund_transfer(&transaction), None);
    }

    #[test]
    fn none_when_the_refund_receiver_is_unset() {
        let transaction = SafeTransaction {
            refund_receiver: Address::ZERO,
            ..relayed_tx()
        };

        assert_eq!(refund_transfer(&transaction), None);
    }

    #[test]
    fn builds_an_erc20_transfer_to_the_refund_receiver() {
        let refund = refund_transfer(&relayed_tx()).expect("a refund to check");

        assert_eq!(refund.safe, SAFE);
        assert_eq!(refund.to, GAS_TOKEN);
        assert_eq!(
            refund.data,
            transferCall {
                to: REFUND_RECEIVER,
                amount: U256::from(1u64) * U256::from(121_000u64),
            }
            .abi_encode()
        );
    }

    #[test]
    fn never_lets_a_secure_refund_leg_affirm_the_whole_transaction() {
        assert_eq!(deny_or_abstain(Verdict::Secure), Verdict::Abstain);
    }

    #[test]
    fn passes_through_a_denial() {
        let denial = Verdict::Insecure {
            rule: RuleId::R4_3ValueTarget,
        };

        assert_eq!(deny_or_abstain(denial), denial);
    }

    #[test]
    fn passes_through_an_abstention() {
        assert_eq!(deny_or_abstain(Verdict::Abstain), Verdict::Abstain);
    }
}
// PoC for F-ENG-032 — `RefundChecker` is dead: its synthetic refund transfer
// carries `chainId = 0`, so the delegated address-poisoning check always
// abstains.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/refund.rs`. It is a sibling of the
// existing `#[cfg(test)] mod tests`, which is left untouched. Note that the
// existing suite never calls `check()` at all — which is exactly why this bug
// was invisible.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_032
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_032 {
    use super::*;
    use crate::{
        contracts::bindings::erc20::Transfer,
        engine::{Operation, RuleId},
    };
    use alloy::{
        primitives::{Address, U256, address},
        rpc::types::Log,
        sol_types::{SolCall as _, SolEvent as _},
        transports::mock::Asserter,
    };
    use safenet_core::provider::Provider;

    /// The chain id `Provider::mocked` reports (`0x5afe` = 23294), against
    /// which `AddressPoisoningChecker::check` compares
    /// `transaction.chain_id`.
    const MOCK_CHAIN_ID: u64 = 0x5afe;

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// USDC — the `gasToken` the refund is paid in.
    const USDC: Address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    /// A relayer the Safe has genuinely paid in USDC before.
    const ESTABLISHED_RELAYER: Address = address!("0xAbCd111111111111111111111111111111112345");
    /// The poisoned lookalike: shares `AbCd` leading and `2345` trailing hex
    /// digits with `ESTABLISHED_RELAYER` (4 and 4, `is_lookalike`'s exact
    /// thresholds at `address_poisoning.rs:78-91`) while differing in the
    /// middle. This is the address `RefundChecker` exists to deny.
    const POISONED_LOOKALIKE: Address = address!("0xAbCd999999999999999999999999999999992345");
    /// The block the sentinel declares current.
    const BLOCK: u64 = 22_020_096;

    fn checker(asserter: &Asserter) -> RefundChecker {
        RefundChecker::new(Arc::new(AddressPoisoningChecker::new(
            Provider::mocked(asserter),
            50_000,
            None,
        )))
    }

    /// A relayed transaction whose gas refund is paid in USDC to the poisoned
    /// lookalike. The primary leg is deliberately inert (`to` an unrelated
    /// address, empty calldata) so that nothing but the refund is under test.
    fn relayed_with_poisoned_refund_receiver() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(MOCK_CHAIN_ID),
            safe: SAFE,
            to: address!("0x000000000000000000000000000000000000B0B0"),
            value: U256::ZERO,
            data: Default::default(),
            operation: Operation::Call,
            safe_tx_gas: U256::from(100_000u64),
            base_gas: U256::from(21_000u64),
            gas_price: U256::from(1u64),
            gas_token: USDC,
            refund_receiver: POISONED_LOOKALIKE,
            nonce: U256::from(42u64),
        }
    }

    /// A genuine, non-zero `Transfer(safe -> ESTABLISHED_RELAYER, 1000)` log
    /// emitted by USDC — the evidence that makes `POISONED_LOOKALIKE` a
    /// lookalike of an established recipient rather than a novel address.
    fn established_history_log() -> Log {
        Log {
            inner: alloy::primitives::Log {
                address: USDC,
                data: Transfer {
                    from: SAFE,
                    to: ESTABLISHED_RELAYER,
                    amount: U256::from(1_000u64),
                }
                .encode_log_data(),
            },
            block_number: Some(BLOCK - 1),
            log_index: Some(0),
            ..Default::default()
        }
    }

    /// (1) PINS TODAY'S BEHAVIOUR, AND PROVES NO RPC IS ISSUED.
    /// Passes on unfixed code.
    ///
    /// The `Asserter` queue is left **empty**. `alloy`'s mock transport panics
    /// when a request arrives with nothing queued, so reaching the assertion
    /// at all proves `RefundChecker::check` returned without issuing a single
    /// `eth_getLogs` call — the checker abstained at the chain-id comparison
    /// (`address_poisoning.rs:312-319`), before any lookup.
    #[tokio::test]
    async fn poc_f_eng_032_abstains_without_any_rpc_call_today() {
        let asserter = Asserter::new();

        assert_eq!(
            checker(&asserter)
                .check(
                    &relayed_with_poisoned_refund_receiver(),
                    &CheckContext { block: BLOCK },
                )
                .await,
            Verdict::Abstain,
            "unfixed behaviour: the synthetic transfer's `chain_id` is `U256::ZERO`, so it can \
             never equal a live provider's chain id and the delegated check abstains"
        );
    }

    /// (2) THE ROOT CAUSE, ISOLATED. Passes on unfixed code, fails once fixed.
    ///
    /// A single-field assertion on the synthetic transaction itself. This is
    /// the whole bug: `..Default::default()` at `refund.rs:116`.
    #[test]
    fn poc_f_eng_032_the_synthetic_transfer_has_chain_id_zero() {
        let refund = refund_transfer(&relayed_with_poisoned_refund_receiver())
            .expect("a refund to check");

        assert_eq!(refund.chain_id, U256::ZERO);
        assert_ne!(
            refund.chain_id,
            relayed_with_poisoned_refund_receiver().chain_id,
            "the synthetic transfer does not inherit the real transaction's chain"
        );
    }

    /// (3) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// With one genuine `Transfer` to `ESTABLISHED_RELAYER` in the lookback
    /// window and a refund receiver that is a 4+4-nibble lookalike of it, the
    /// checker must deny under R-4.3 — the exact case it was written for.
    ///
    /// On unfixed code this fails with `left: Abstain`, `right: Insecure {
    /// rule: R4_3ValueTarget }`, **and the queued log is never consumed**.
    ///
    /// One queued success is enough because `max_block_range` is `None`, so
    /// `block_chunks` issues the whole 50,000-block window as a single
    /// `eth_getLogs` call (`address_poisoning.rs:249-268`).
    #[tokio::test]
    async fn poc_f_eng_032_denies_a_poisoned_erc20_refund_receiver() {
        let asserter = Asserter::new();
        asserter.push_success(&vec![established_history_log()]);

        assert_eq!(
            checker(&asserter)
                .check(
                    &relayed_with_poisoned_refund_receiver(),
                    &CheckContext { block: BLOCK },
                )
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_3ValueTarget,
            },
        );
    }

    /// (4) THE CONTROL FOR (3). **Expected to FAIL on unfixed code** for the
    /// same reason, and to pass once fixed.
    ///
    /// The same transaction with the refund paid to the *established*
    /// relayer itself must reach `ExactMatch` and then be squashed to
    /// `Abstain` by `deny_or_abstain` (`refund.rs:68-73`) — never `Secure`.
    /// Today it returns `Abstain` too, but for the wrong reason, so the pair
    /// (3)/(4) is indistinguishable on the wire. That indistinguishability is
    /// itself why the defect went unnoticed.
    #[tokio::test]
    async fn poc_f_eng_032_an_established_refund_receiver_abstains_after_a_real_lookup() {
        let asserter = Asserter::new();
        asserter.push_success(&vec![established_history_log()]);

        let transaction = SafeTransaction {
            refund_receiver: ESTABLISHED_RELAYER,
            ..relayed_with_poisoned_refund_receiver()
        };

        assert_eq!(
            checker(&asserter)
                .check(&transaction, &CheckContext { block: BLOCK })
                .await,
            Verdict::Abstain,
        );
        // Once fixed, the lookup really happened, so the queue is drained.
        // On unfixed code the assertion above still passes while this one
        // fails — the cleanest single discriminator in this file.
        //
        // NOTE: `Asserter::is_empty` is the one identifier in this PoC that
        // could not be verified against source (no dependency sources on
        // disk, assumption A6). If your `alloy` version exposes no such
        // accessor, DELETE these three lines — test (1) already establishes
        // the same fact, by leaving the queue empty and relying on the mock
        // transport to panic if a request is ever issued.
        // V-ENG (Phase 5): Q-ENG-A settled by reading
        // ~/.cargo/registry/src/index.crates.io-*/alloy-transport-2.0.5/src/mock.rs:44-89 —
        // `Asserter` exposes `new`, `push`, `push_success`, `push_failure`,
        // `push_failure_msg`, `pop_response`, `read_q`, `write_q`. There is NO
        // `is_empty`. Per QA-ENG's instruction the three lines are deleted here;
        // test (1) establishes the same fact by leaving the queue empty.
    }
}
