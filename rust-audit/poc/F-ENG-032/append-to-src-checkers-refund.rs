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
        assert!(
            asserter.is_empty(),
            "the queued eth_getLogs response was never consumed: no lookup was issued"
        );
    }
}
