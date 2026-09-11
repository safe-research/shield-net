// PoC for F-ENG-031 — the gas-refund leg is never vetted on any transaction
// an affirming checker approves.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/staking.rs`. That file has no
// `#[cfg(test)]` module today, so nothing is overwritten.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_031
//
// `StakingChecker` is a pure function of `SafeTransaction` — no RPC, no HTTP,
// no `CheckContext` — so this whole PoC is deterministic and hermetic.
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_031 {
    use super::*;
    use alloy::{
        primitives::{B256, Bytes},
        sol_types::SolCall as _,
    };

    /// The victim Safe.
    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// The attacker's payout address.
    const ATTACKER: Address = address!("0x000000000000000000000000000000000000dEaD");
    /// USDC on mainnet — a token a real Safe plausibly holds. Used as
    /// `gasToken` for the ERC-20 refund leg, which Safe pays with **no
    /// `tx.gasprice` cap**.
    const USDC: Address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");

    /// `claim(account = safe, …)` against the canonical rewards distributor.
    /// Only `account` is inspected (`staking.rs:156-161`); the proof fields
    /// are never validated, so any well-formed encoding works.
    fn claim_calldata(account: Address) -> Bytes {
        Bytes::from(
            claimCall {
                account,
                cumulativeAmount: U256::ZERO,
                expectedMerkleRoot: B256::ZERO,
                merkleProof: Vec::<B256>::new(),
            }
            .abi_encode(),
        )
    }

    /// The honest transaction this attack hides behind: a mainnet reward
    /// claim paying the Safe itself, with no refund leg.
    fn honest_claim() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: REWARDS_DISTRIBUTOR,
            value: U256::ZERO,
            data: claim_calldata(SAFE),
            operation: Operation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    /// (0) CONTROL. Passes today and must keep passing after any fix: the
    /// honest claim, with every refund field zero, is the case the checker
    /// exists to affirm. The whole finding is the delta between this and the
    /// three tests below, which differ from it *only* in refund fields.
    #[tokio::test]
    async fn poc_f_eng_031_control_an_unrelayed_claim_is_affirmed() {
        assert_eq!(
            StakingChecker
                .check(&honest_claim(), &CheckContext::default())
                .await,
            Verdict::Secure,
        );
    }

    /// (0b) CONTROL — the affirmation is real, not accidental. A `claim`
    /// naming an account other than the Safe is denied `R-4.3`. The
    /// affirmation and the denial differ by one field, which is what makes
    /// this a good paired corpus vector.
    #[tokio::test]
    async fn poc_f_eng_031_control_a_claim_for_another_account_is_denied() {
        let misdirected = SafeTransaction {
            data: claim_calldata(ATTACKER),
            ..honest_claim()
        };

        assert_eq!(
            StakingChecker
                .check(&misdirected, &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_3ValueTarget,
            },
        );
    }

    /// Trigger A — the native-currency refund drain.
    ///
    /// `baseGas = 1e12` and `gasPrice = 1 gwei`. Safe's `handlePayment` pays
    /// `(gasUsed + baseGas) * gasPrice` to `refundReceiver`; at 1 gwei that
    /// is ~1000 ETH. `baseGas` is a proposer-chosen field added to the
    /// *measured* `gasUsed` — it never has to be spendable gas, so the block
    /// gas limit does not bound it. Only `safeTxGas` bounds real execution.
    fn native_refund_drain() -> SafeTransaction {
        SafeTransaction {
            base_gas: U256::from(1_000_000_000_000u64),
            gas_price: U256::from(1_000_000_000u64),
            gas_token: Address::ZERO,
            refund_receiver: ATTACKER,
            ..honest_claim()
        }
    }

    /// Trigger B — the ERC-20 refund drain. **This is the sub-case neither
    /// TODO in `refund.rs` covers.**
    ///
    /// With `gasToken` set to a real ERC-20 there is no `tx.gasprice` cap at
    /// all, so `gasPrice = 1e18` is accepted and the payout is
    /// `(gasUsed + baseGas) * 1e18` token units. The team's stated design is
    /// that this path *is* checked (`refund_transfer` builds a synthetic
    /// transfer for it) — it is not, for two independent reasons: the
    /// delegated check is dead (F-ENG-032), and `StakingChecker` at position
    /// 8 affirms before `RefundChecker` at position 9 ever runs.
    fn erc20_refund_drain() -> SafeTransaction {
        SafeTransaction {
            base_gas: U256::from(1_000_000_000_000u64),
            gas_price: U256::from(10u64).pow(U256::from(18u64)),
            gas_token: USDC,
            refund_receiver: ATTACKER,
            ..honest_claim()
        }
    }

    /// Trigger F — `refundReceiver == 0`, so Safe pays `tx.origin`: an
    /// address the engine cannot learn ahead of execution. `refund.rs:90-96`
    /// names this hole; nothing acts on it.
    fn tx_origin_refund_drain() -> SafeTransaction {
        SafeTransaction {
            base_gas: U256::from(1_000_000_000_000u64),
            gas_price: U256::from(1_000_000_000u64),
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            ..honest_claim()
        }
    }

    /// (1) PINS TODAY'S BEHAVIOUR. Passes on unfixed code: all three hostile
    /// refund legs are affirmed, identically to the control.
    #[tokio::test]
    async fn poc_f_eng_031_affirms_every_hostile_refund_leg_today() {
        for (name, transaction) in [
            ("native", native_refund_drain()),
            ("erc20", erc20_refund_drain()),
            ("tx.origin", tx_origin_refund_drain()),
        ] {
            assert_eq!(
                StakingChecker
                    .check(&transaction, &CheckContext::default())
                    .await,
                Verdict::Secure,
                "unfixed behaviour ({name}): no refund field is read anywhere in staking.rs"
            );
        }
    }

    /// (2) THE REGRESSION TEST. **Expected to FAIL on unfixed code**, on the
    /// first iteration (`native`).
    ///
    /// A transaction that differs from an affirmed one only in
    /// `gasPrice`/`baseGas`/`refundReceiver` must not inherit its
    /// affirmation. Charter §2.3 puts "gas and refund parameters" inside the
    /// transaction under review, R-4.3 governs the value they send, and §3.7
    /// forbids `secure` unless every applicable rule is satisfied.
    ///
    /// This is the assertion the finding's remediation option 4 ("require
    /// `gas_price.is_zero()` inside each affirming predicate") satisfies, and
    /// it is also satisfied by options 1–3.
    #[tokio::test]
    async fn poc_f_eng_031_a_hostile_refund_leg_must_not_be_affirmed() {
        for (name, transaction) in [
            ("native", native_refund_drain()),
            ("erc20", erc20_refund_drain()),
            ("tx.origin", tx_origin_refund_drain()),
        ] {
            assert_ne!(
                StakingChecker
                    .check(&transaction, &CheckContext::default())
                    .await,
                Verdict::Secure,
                "{name}: the refund pays value to an attacker-chosen address and was never vetted"
            );
        }
    }
}
