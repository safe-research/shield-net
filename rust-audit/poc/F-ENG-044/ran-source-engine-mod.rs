//! Transaction-verification logic for the sentinel engine service.
//!
//! This module is independent of the HTTP transport in [`crate::api`]. The
//! API passes decoded Safe transactions to [`SentinelEngine`], which owns the
//! configured checker chain.

mod rule;
mod transaction;

pub use self::{
    rule::RuleId,
    transaction::{Operation, SafeTransaction},
};
use crate::checkers::Checker;
use serde::{Deserialize, Serialize};

/// The transaction-verification engine shared by API handlers.
pub struct SentinelEngine(Vec<Box<dyn Checker>>);

/// Per-request context threaded to every [`Checker`] alongside the
/// transaction being assessed, carrying caller-supplied hints that aren't
/// part of the transaction itself.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CheckContext {
    /// The block number the caller (the sentinel) considers current — the
    /// most recent block it had synced past when it submitted this check,
    /// from the request's required `block` field. A check that reads
    /// RPC-derived state should evaluate against this rather than resolving
    /// "latest" itself, so it shares the same view of the chain the caller
    /// had rather than racing ahead of (or behind) it. A check is free to
    /// ignore this if it has no RPC-derived state to anchor.
    pub block: u64,
}

/// The engine's assessment of a proposed transaction.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase", tag = "verdict")]
pub enum Verdict {
    /// All configured checks consider the transaction secure.
    Secure,
    /// A check found that the transaction violates a Charter rule.
    Insecure {
        /// The rule violated by the transaction.
        rule: RuleId,
    },
    /// The engine abstains because it cannot give a trustworthy answer.
    Abstain,
}

impl SentinelEngine {
    /// Creates an engine that runs `checkers` in order.
    pub fn new(checkers: Vec<Box<dyn Checker>>) -> Self {
        Self(checkers)
    }

    /// Assesses a proposed Safe transaction using the configured checks.
    pub async fn security_check(
        &self,
        transaction: SafeTransaction,
        context: CheckContext,
    ) -> Verdict {
        let mut verdict = Verdict::Abstain;
        for checker in &self.0 {
            verdict = checker.check(&transaction, &context).await;
            tracing::trace!(checker = checker.name(), ?verdict, "checker verdict");
            if verdict != Verdict::Abstain {
                break;
            }
        }
        tracing::trace!(?verdict, "security check verdict");
        verdict
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubChecker(Verdict);

    #[async_trait::async_trait]
    impl Checker for StubChecker {
        fn name(&self) -> &'static str {
            "stub"
        }

        async fn check(&self, _: &SafeTransaction, _: &CheckContext) -> Verdict {
            self.0
        }
    }

    #[tokio::test]
    async fn abstains_when_every_checker_abstains() {
        let engine = SentinelEngine::new(vec![Box::new(StubChecker(Verdict::Abstain))]);

        assert_eq!(
            engine
                .security_check(SafeTransaction::default(), CheckContext::default())
                .await,
            Verdict::Abstain
        );
    }

    #[tokio::test]
    async fn stops_at_the_first_non_abstaining_verdict() {
        let engine = SentinelEngine::new(vec![
            Box::new(StubChecker(Verdict::Abstain)),
            Box::new(StubChecker(Verdict::Secure)),
            Box::new(StubChecker(Verdict::Insecure {
                rule: RuleId::R4_3ValueTarget,
            })),
        ]);

        assert_eq!(
            engine
                .security_check(SafeTransaction::default(), CheckContext::default())
                .await,
            Verdict::Secure
        );
    }
}
// PoC for F-ENG-044 — the first-non-abstain-wins combinator cannot implement
// Charter §3.7.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/engine/mod.rs`. It is a sibling of the existing
// `#[cfg(test)] mod tests`, not a replacement for it; nothing in the crate is
// modified. Revert by deleting the block.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_044
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_044 {
    use super::*;
    use crate::{
        checkers::{
            BaseChecker, BlocklistChecker, CancellationChecker, CowChecker, EscapeHatchChecker,
            ExcessiveApprovalChecker, NestedSafeChecker, StakingChecker,
        },
        contracts::bindings::safenet_guard,
    };
    use alloy::{
        primitives::{Address, Bytes, U256, address},
        sol_types::SolCall as _,
    };

    /// The victim Safe.
    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// An address the operator has explicitly configured as malicious
    /// (Charter R-4.6). The attacker sets it as `to`.
    const BLOCKLISTED: Address = address!("0x1111111111111111111111111111111111111111");

    struct StubChecker(&'static str, Verdict);

    #[async_trait::async_trait]
    impl Checker for StubChecker {
        fn name(&self) -> &'static str {
            self.0
        }

        async fn check(&self, _: &SafeTransaction, _: &CheckContext) -> Verdict {
            self.1
        }
    }

    /// The attacker's transaction, written out in full.
    ///
    /// `data` is the bare 4-byte `announceTransaction` selector plus one
    /// arbitrary trailing byte: `EscapeHatchChecker` matches on the selector
    /// prefix only and never ABI-decodes the argument
    /// (`escape_hatch.rs:56-60`), so this need not be a valid encoding — and
    /// making it invalid is the point, because it proves the affirmation
    /// carries no information about the call.
    fn attacker_transaction() -> SafeTransaction {
        let mut data = safenet_guard::announceTransactionCall::SELECTOR.to_vec();
        data.push(0x00);
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: BLOCKLISTED,
            value: U256::ZERO,
            data: Bytes::from(data),
            operation: Operation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    /// `main.rs:57-73`'s registration order, minus the two RPC-backed
    /// checkers (`RefundChecker`, `AddressPoisoningChecker`) that need a live
    /// `Provider`. Both of those run *after* every checker below, so their
    /// absence cannot change the verdict for this fixture: the chain has
    /// already broken by then.
    fn production_chain_without_rpc_checkers() -> Vec<Box<dyn Checker>> {
        vec![
            Box::new(CancellationChecker),
            Box::new(EscapeHatchChecker),
            Box::new(BaseChecker),
            Box::new(BlocklistChecker::new([BLOCKLISTED])),
            Box::new(NestedSafeChecker),
            Box::new(ExcessiveApprovalChecker),
            Box::new(CowChecker::new()),
            Box::new(StakingChecker),
        ]
    }

    /// (1) PINS TODAY'S BEHAVIOUR. Passes on unfixed code; must be deleted or
    /// inverted by whoever fixes F-ENG-044.
    ///
    /// The production chain answers `secure` for a transaction whose `to` the
    /// operator configured as malicious, because `EscapeHatchChecker`
    /// (position 2) affirms before `BlocklistChecker` (position 4) runs.
    #[tokio::test]
    async fn poc_f_eng_044_production_chain_returns_secure_today() {
        let engine = SentinelEngine::new(production_chain_without_rpc_checkers());

        assert_eq!(
            engine
                .security_check(attacker_transaction(), CheckContext { block: 22_020_096 })
                .await,
            Verdict::Secure,
            "unfixed behaviour: an affirmation at position 2 ends the chain"
        );
    }

    /// (2) THE LATER CHECKER WOULD HAVE DENIED IT. Passes today; it exists
    /// only to prove that test (3)'s expectation is reachable and that the
    /// rule really is implemented — the engine simply never runs it.
    #[tokio::test]
    async fn poc_f_eng_044_the_suppressed_checker_denies_the_same_transaction() {
        let denier = BlocklistChecker::new([BLOCKLISTED]);

        assert_eq!(
            denier
                .check(&attacker_transaction(), &CheckContext { block: 22_020_096 })
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_6KnownMaliciousTarget,
            },
        );
    }

    /// (3) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// Charter §3.7: "A transaction is secure only if it satisfies all
    /// applicable Article IV rules." R-4.6 is an applicable rule here, it is
    /// implemented (test 2), and it is violated — so the only Charter-correct
    /// verdict is `insecure R-4.6`.
    ///
    /// Failure output on unfixed code:
    ///   left:  Secure
    ///   right: Insecure { rule: R4_6KnownMaliciousTarget }
    #[tokio::test]
    async fn poc_f_eng_044_a_denial_must_win_over_an_earlier_affirmation() {
        let engine = SentinelEngine::new(production_chain_without_rpc_checkers());

        assert_eq!(
            engine
                .security_check(attacker_transaction(), CheckContext { block: 22_020_096 })
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_6KnownMaliciousTarget,
            },
            "Charter §3.7 requires the conjunction of all applicable Article IV rules; \
             engine/mod.rs:62-69 returns the first non-abstaining verdict instead"
        );
    }

    /// (4) THE COMBINATOR IN ISOLATION. **Expected to FAIL on unfixed code.**
    ///
    /// No transaction content, no checker logic, no chain state: two stubs.
    /// This is the same shape as the crate's own
    /// `stops_at_the_first_non_abstaining_verdict` (`engine/mod.rs:104-120`),
    /// with the Charter-correct expectation instead of the implemented one.
    /// Whatever fix is chosen for F-ENG-044 must make this pass, and any
    /// future over-broad affirmer is caught by it for free.
    #[tokio::test]
    async fn poc_f_eng_044_ordering_must_not_decide_the_verdict() {
        let denial = Verdict::Insecure {
            rule: RuleId::R4_6KnownMaliciousTarget,
        };

        for order in [[Verdict::Secure, denial], [denial, Verdict::Secure]] {
            let engine = SentinelEngine::new(vec![
                Box::new(StubChecker("first", order[0])),
                Box::new(StubChecker("second", order[1])),
            ]);

            assert_eq!(
                engine
                    .security_check(SafeTransaction::default(), CheckContext::default())
                    .await,
                denial,
                "a chain containing a denial must return that denial regardless of position"
            );
        }
    }
}
