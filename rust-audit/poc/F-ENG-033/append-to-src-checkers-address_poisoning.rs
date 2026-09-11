// PoC for F-ENG-033 — `AddressPoisoningChecker` affirms `secure` from event
// history on an attacker-chosen `to`, and never inspects `transaction.value`.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/address_poisoning.rs`. It is a sibling
// of the existing `#[cfg(test)] mod tests` (which covers only the pure
// helpers `nibbles`, `is_lookalike` and `block_chunks`, never `check`).
//
// RUN: cargo test -p sentinel-engine poc_f_eng_033
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_033 {
    use super::*;
    use alloy::{primitives::address, transports::mock::Asserter};

    /// The chain id `Provider::mocked` reports (`0x5afe` = 23294).
    const MOCK_CHAIN_ID: u64 = 0x5afe;
    const BLOCK: u64 = 22_020_096;
    const LOOKBACK: u64 = 50_000;

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// A real ERC-20 the Safe has paid before — used by variant (a).
    const USDC: Address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    /// A recipient that appears as the `to` of one of the Safe's own genuine
    /// `Transfer` logs on that token.
    const ESTABLISHED: Address = address!("0x1111111111111111111111111111111111111111");
    /// The attacker's own contract `T` — variant (b). Never probed: nothing
    /// checks that `to` has code, a `totalSupply`, or any deployment age.
    const ATTACKER_TOKEN: Address = address!("0x7777777777777777777777777777777777777777");
    /// The address `T` names in its self-emitted `Transfer` log, and the
    /// recipient of the `transfer` call in the proposal.
    const ATTACKER_PAYEE: Address = address!("0x8888888888888888888888888888888888888888");

    /// 1000 ETH — the Safe's whole native balance.
    fn drain_amount() -> U256 {
        U256::from(1_000u64) * U256::from(10u64).pow(U256::from(18u64))
    }

    fn checker(asserter: &Asserter) -> AddressPoisoningChecker {
        AddressPoisoningChecker::new(Provider::mocked(asserter), LOOKBACK, None)
    }

    /// A non-zero `Transfer(safe -> to, 1)` log attributed to `token`.
    ///
    /// For variant (b) this is the forgery: `T` emits it itself. Solidity
    /// lets any contract emit any event, the filter's `address` is exactly
    /// `transaction.to`, and `decode_target_and_amount`
    /// (`address_poisoning.rs:275-285`) checks only `topics[0]` — so a
    /// self-emitted log is indistinguishable from a genuine one. `amount = 1`
    /// is the cheapest value that clears the zero-amount exclusion at
    /// `address_poisoning.rs:215-217`.
    fn transfer_log(token: Address, to: Address, amount: u64) -> Log {
        Log {
            inner: alloy::primitives::Log {
                address: token,
                data: Transfer {
                    from: SAFE,
                    to,
                    amount: U256::from(amount),
                }
                .encode_log_data(),
            },
            block_number: Some(BLOCK - 1),
            log_index: Some(0),
            ..Default::default()
        }
    }

    /// Variant (a): a real token, a genuinely established recipient, a
    /// one-unit `transfer` — and the Safe's whole native balance riding
    /// along in `value`, which the checker never reads.
    fn value_bearing_transfer_to_an_established_recipient() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(MOCK_CHAIN_ID),
            safe: SAFE,
            to: USDC,
            value: drain_amount(),
            data: transferCall {
                to: ESTABLISHED,
                amount: U256::from(1u64),
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

    /// Variant (b): the evidence pool itself is attacker-supplied. `to` is
    /// the attacker's own contract `T`, whose `transfer` is `payable`, so
    /// `value` really moves.
    fn value_bearing_transfer_to_an_attacker_deployed_token() -> SafeTransaction {
        SafeTransaction {
            to: ATTACKER_TOKEN,
            data: transferCall {
                to: ATTACKER_PAYEE,
                amount: U256::from(1u64),
            }
            .abi_encode()
            .into(),
            ..value_bearing_transfer_to_an_established_recipient()
        }
    }

    /// (1a) PINS TODAY'S BEHAVIOUR — variant (a). Passes on unfixed code.
    #[tokio::test]
    async fn poc_f_eng_033_affirms_a_value_bearing_call_today() {
        let asserter = Asserter::new();
        asserter.push_success(&vec![transfer_log(USDC, ESTABLISHED, 1_000)]);

        assert_eq!(
            checker(&asserter)
                .check(
                    &value_bearing_transfer_to_an_established_recipient(),
                    &CheckContext { block: BLOCK },
                )
                .await,
            Verdict::Secure,
            "unfixed behaviour: `transaction.value` is not read anywhere in this checker"
        );
    }

    /// (1b) PINS TODAY'S BEHAVIOUR — variant (b). Passes on unfixed code.
    ///
    /// The only evidence is a log the attacker's own contract emitted, on a
    /// contract the engine never establishes to be a token at all.
    #[tokio::test]
    async fn poc_f_eng_033_affirms_from_self_emitted_history_today() {
        let asserter = Asserter::new();
        asserter.push_success(&vec![transfer_log(ATTACKER_TOKEN, ATTACKER_PAYEE, 1)]);

        assert_eq!(
            checker(&asserter)
                .check(
                    &value_bearing_transfer_to_an_attacker_deployed_token(),
                    &CheckContext { block: BLOCK },
                )
                .await,
            Verdict::Secure,
            "unfixed behaviour: the evidence pool is keyed by `transaction.to`, which the \
             proposer chooses"
        );
    }

    /// (2) CONTROL. Passes today and must keep passing: with the same
    /// history and `value = 0`, an exact-match recipient is a case the
    /// checker is entitled to have an opinion about. A fix that turns this
    /// into a denial has over-corrected.
    #[tokio::test]
    async fn poc_f_eng_033_control_a_zero_value_transfer_to_an_established_recipient() {
        let asserter = Asserter::new();
        asserter.push_success(&vec![transfer_log(USDC, ESTABLISHED, 1_000)]);

        let transaction = SafeTransaction {
            value: U256::ZERO,
            ..value_bearing_transfer_to_an_established_recipient()
        };

        assert_ne!(
            checker(&asserter)
                .check(&transaction, &CheckContext { block: BLOCK })
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_3ValueTarget,
            },
        );
    }

    /// (3) THE REGRESSION TEST — variant (a). **Expected to FAIL on unfixed
    /// code.**
    ///
    /// Charter R-4.3: value sent to a recipient outside the expected target
    /// set is insecure, and §2.4 derives that set from "the user's
    /// established onchain pattern". A one-unit ERC-20 transfer's history
    /// says nothing about 1000 ETH of native value riding on the same call.
    /// Remediation option 1 (`require value.is_zero()` in `decode_target`)
    /// and option 2 (make the checker deny-only) both make this pass.
    #[tokio::test]
    async fn poc_f_eng_033_a_value_bearing_call_must_not_be_affirmed() {
        let asserter = Asserter::new();
        asserter.push_success(&vec![transfer_log(USDC, ESTABLISHED, 1_000)]);

        assert_ne!(
            checker(&asserter)
                .check(
                    &value_bearing_transfer_to_an_established_recipient(),
                    &CheckContext { block: BLOCK },
                )
                .await,
            Verdict::Secure,
        );
    }

    /// (4) THE REGRESSION TEST — variant (b). **Expected to FAIL on unfixed
    /// code.**
    ///
    /// Note this one is *not* closed by option 1: set `value` to zero here
    /// and the affirmation still stands on forged evidence. Only options 2, 3
    /// or 4 (deny-only, standing for `to`, or per-log provenance) close it.
    #[tokio::test]
    async fn poc_f_eng_033_a_self_emitted_history_must_not_affirm() {
        let asserter = Asserter::new();
        asserter.push_success(&vec![transfer_log(ATTACKER_TOKEN, ATTACKER_PAYEE, 1)]);

        assert_ne!(
            checker(&asserter)
                .check(
                    &value_bearing_transfer_to_an_attacker_deployed_token(),
                    &CheckContext { block: BLOCK },
                )
                .await,
            Verdict::Secure,
        );
    }
}
