// PoC for F-ENG-034 — `EscapeHatchChecker` affirms the announcement shape for
// ANY `to` and runs ahead of the blocklist, so an R-4.6 target is rated
// `secure`.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/escape_hatch.rs`. That file has no
// `#[cfg(test)]` module today — it has no test block at all — so nothing is
// overwritten.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_034
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_034 {
    use super::*;
    use crate::{
        checkers::BlocklistChecker,
        engine::{RuleId, SentinelEngine},
    };
    use alloy::{
        primitives::{Address, Bytes, U256, address},
        sol_types::SolCall as _,
    };

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// The operator's configured R-4.6 target — their only expression of
    /// "this address is known malicious".
    const BLOCKLISTED: Address = address!("0x1111111111111111111111111111111111111111");

    /// The bare `announceTransaction` selector plus one arbitrary trailing
    /// byte. `is_escape_hatch_call` matches with `starts_with`
    /// (`escape_hatch.rs:56-60`) and never ABI-decodes, so this is accepted
    /// even though it is **not a valid encoding of anything**. That is the
    /// sharper half of the finding: the affirmation carries no information
    /// about the call it affirms.
    fn malformed_announcement_calldata() -> Bytes {
        let mut data = safenet_guard::announceTransactionCall::SELECTOR.to_vec();
        data.push(0x00);
        Bytes::from(data)
    }

    fn announcement_to_a_blocklisted_address() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: BLOCKLISTED,
            value: U256::ZERO,
            data: malformed_announcement_calldata(),
            operation: Operation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    /// (1) PINS TODAY'S BEHAVIOUR — the checker alone.
    #[tokio::test]
    async fn poc_f_eng_034_affirms_any_to_today() {
        assert_eq!(
            EscapeHatchChecker
                .check(&announcement_to_a_blocklisted_address(), &CheckContext::default())
                .await,
            Verdict::Secure,
            "unfixed behaviour: no constraint on `to`, and no ABI decode of the argument"
        );
    }

    /// (2) PINS TODAY'S BEHAVIOUR — the ordering, in `main.rs`'s own order.
    ///
    /// `EscapeHatchChecker` is 2nd and `BlocklistChecker` is 4th
    /// (`main.rs:59`, `:61`), so the affirmation ends the chain before R-4.6
    /// is ever evaluated.
    #[tokio::test]
    async fn poc_f_eng_034_the_blocklist_never_runs_today() {
        let engine = SentinelEngine::new(vec![
            Box::new(EscapeHatchChecker),
            Box::new(BlocklistChecker::new([BLOCKLISTED])),
        ]);

        assert_eq!(
            engine
                .security_check(
                    announcement_to_a_blocklisted_address(),
                    CheckContext::default()
                )
                .await,
            Verdict::Secure,
        );
    }

    /// (3) CONTROL — R-4.6 is implemented and the fixture violates it.
    ///
    /// The same transaction with a different first four bytes of `data`
    /// reaches the blocklist and is denied. The pair (2)/(3) isolates the
    /// bypass to the selector alone.
    #[tokio::test]
    async fn poc_f_eng_034_control_a_different_selector_is_denied() {
        let engine = SentinelEngine::new(vec![
            Box::new(EscapeHatchChecker),
            Box::new(BlocklistChecker::new([BLOCKLISTED])),
        ]);
        let not_an_announcement = SafeTransaction {
            data: Bytes::from(vec![0xde, 0xad, 0xbe, 0xef, 0x00]),
            ..announcement_to_a_blocklisted_address()
        };

        assert_eq!(
            engine
                .security_check(not_an_announcement, CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_6KnownMaliciousTarget,
            },
        );
    }

    /// (4) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// Charter § 2.18 auto-allows calls to *the Safenet Guard's*
    /// `announceTransaction`/`cancelAnnouncement`, and `SafenetGuard._isAutoAllowed`
    /// enforces `to == address(this)`. A call to some *other* address that
    /// merely starts with the same selector is not in that set — it is
    /// exactly the traffic that does reach the sentinel and does need a
    /// verdict.
    ///
    /// Satisfied by remediation option 1 (reorder), 2 (constrain `to`) or
    /// 3 (abstain).
    #[tokio::test]
    async fn poc_f_eng_034_an_r4_6_target_must_not_be_affirmed() {
        let engine = SentinelEngine::new(vec![
            Box::new(EscapeHatchChecker),
            Box::new(BlocklistChecker::new([BLOCKLISTED])),
        ]);

        assert_eq!(
            engine
                .security_check(
                    announcement_to_a_blocklisted_address(),
                    CheckContext::default()
                )
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_6KnownMaliciousTarget,
            },
        );
    }

    /// (5) THE ARGUMENT-VALIDATION HALF. **Expected to FAIL on unfixed code.**
    ///
    /// Independent of the blocklist: an affirmation should not be reachable
    /// with calldata that does not decode. Satisfied only by remediation
    /// option 4. Kept separate because options 1-3 do **not** close it.
    #[tokio::test]
    async fn poc_f_eng_034_a_malformed_announcement_must_not_be_affirmed() {
        let unlisted = SafeTransaction {
            to: address!("0x2222222222222222222222222222222222222222"),
            ..announcement_to_a_blocklisted_address()
        };

        assert_ne!(
            EscapeHatchChecker
                .check(&unlisted, &CheckContext::default())
                .await,
            Verdict::Secure,
            "the calldata is a bare selector plus a junk byte and decodes as nothing"
        );
    }
}
