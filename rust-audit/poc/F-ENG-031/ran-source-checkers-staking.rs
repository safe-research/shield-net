//! R-4.3/R-4.5 worked example: Safenet's own SAFE-token staking flow —
//! claiming staking rewards and/or staking (optionally re-staking freshly
//! claimed rewards) toward a validator.
//!
//! Each [`claim`](crate::contracts::bindings::staking::claimCall) against the
//! canonical [`REWARDS_DISTRIBUTOR`] is checked on its own: the contract pays
//! `account` — not necessarily `msg.sender` — so it's only accepted when
//! `account` is the Safe itself; otherwise the claimed rewards would be paid
//! to an unrelated address, the same value-misdirection concern
//! [`RuleId::R4_3ValueTarget`] already covers for a plain ERC-20
//! `transfer`/`transferFrom`.
//!
//! What's left after any `claim` calls are set aside is checked as a whole:
//!
//! - A single [`stake`](crate::contracts::bindings::staking::stakeCall)
//!   against [`STAKING`] is secure on its own — it spends an allowance
//!   granted by some earlier, separately-vetted transaction.
//!   `validator` isn't checked here — an unknown or deregistering validator
//!   only affects the staker's own future rewards, not fund safety (the
//!   staked amount stays attributed to the Safe itself in the contract's own
//!   accounting, and can always be withdrawn later).
//! - Exactly one `approve` paired with exactly one `stake`, with the
//!   `approve` running *first* (order matters: an `approve` that runs after
//!   the `stake` it's "paired" with never actually funds it — see
//!   [`check_pair`]), is secure as long as the approved amount doesn't
//!   exceed the staked amount. An approval that *does* exceed it is denied
//!   under [`RuleId::R4_5ExcessiveApproval`] — the approval was consumed by
//!   the right call, just for more than that call needed. An approval for
//!   *less* is fine; the extra must come from an existing allowance.
//!
//! A dangling `approve` — standalone, or paired with a `stake` in the wrong
//! order so nothing in this transaction actually spends it — is left to
//! [`Verdict::Abstain`] here rather than denied: unused-authorization checks
//! generic to any ERC-20 `approve` (not specific to Safenet staking) belong
//! in a separate, general-purpose approval check, not duplicated in this
//! protocol-specific one.
//!
//! Deliberately narrow beyond that: e.g. two `approve` calls in one batch
//! are *not* summed, since a Safe `approve` sets an allowance rather than
//! incrementing it (the second overwrites the first, so the batch's actual
//! net effect depends on execution order in a way that isn't safe to
//! recover from calldata alone). Anything other than the shapes above —
//! more than one `approve`, more than one `stake`, or any unrelated call —
//! is left to [`Verdict::Abstain`] rather than guessed at either way,
//! consistent with this codebase's other protocol-specific worked examples.

use super::Checker;
use crate::{
    contracts::{
        bindings::{
            erc20::approveCall,
            staking::{claimCall, stakeCall},
        },
        multi_send::sub_transactions,
    },
    engine::{CheckContext, Operation, RuleId, SafeTransaction, Verdict},
};
use alloy::{
    primitives::{Address, U256, address},
    sol_types::SolCall as _,
};

/// Safenet's canonical SAFE-token staking contract on Ethereum mainnet (see
/// `docs/configuration.md`'s `STAKER_ADDRESS` section for the Etherscan
/// link).
const STAKING: Address = address!("115E78f160e1E3eF163B05C84562Fa16fA338509");

/// The SAFE token's own canonical address on Ethereum mainnet.
const SAFE_TOKEN: Address = address!("5aFE3855358E112B5647B952709E6165e1c1eEEe");

/// Safe Foundation's canonical cumulative-claim staking-rewards distributor
/// on Ethereum mainnet (`token() == SAFE_TOKEN`, confirmed independently of
/// this codebase).
const REWARDS_DISTRIBUTOR: Address = address!("e5139fc0fb8eae81e30d8a85c22e88c6757120f2");

/// The only chain these canonical addresses are recognized on.
const SUPPORTED_CHAIN_ID: u64 = 1;

/// The built-in Safenet staking check (see module docs).
pub struct StakingChecker;

#[async_trait::async_trait]
impl Checker for StakingChecker {
    fn name(&self) -> &'static str {
        "staking"
    }

    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        if transaction.chain_id != U256::from(SUPPORTED_CHAIN_ID) {
            return Verdict::Abstain;
        }

        let calls = sub_transactions(transaction);

        let mut remaining = Vec::with_capacity(calls.len());
        let mut claimed = false;
        for call in &calls {
            match claim_account(call) {
                Some(account) if account != transaction.safe => {
                    return Verdict::Insecure {
                        rule: RuleId::R4_3ValueTarget,
                    };
                }
                Some(_) => claimed = true,
                None => remaining.push(call),
            }
        }

        match remaining.as_slice() {
            [] if claimed => Verdict::Secure,
            [] => Verdict::Abstain,
            [call] => check_lone_call(call),
            [first, second] => check_pair(first, second),
            _ => Verdict::Abstain,
        }
    }
}

/// A single non-`claim` call left over after set-aside `claim`s: secure if
/// it's a `stake` (spending some earlier, separately-vetted allowance).
/// A dangling, unused `approve` on [`STAKING`] is left to
/// [`Verdict::Abstain`] — see the module docs for why this check doesn't
/// deny it.
fn check_lone_call(call: &SafeTransaction) -> Verdict {
    if stake_amount(call).is_some() {
        return Verdict::Secure;
    }
    Verdict::Abstain
}

/// Checks an exactly-two-call remainder, in the order the batch itself runs
/// them — order matters here, unlike a same-transaction amount comparison:
/// `[approve, stake]` funds the `stake` with an allowance set moments
/// earlier in the same transaction, but `[stake, approve]` runs the `stake`
/// against whatever allowance already existed *before* this transaction,
/// leaving the `approve` a dangling, un-consumed authorization — left to
/// [`Verdict::Abstain`] for the same reason as [`check_lone_call`]'s
/// standalone `approve` case, regardless of the amount approved.
fn check_pair(first: &SafeTransaction, second: &SafeTransaction) -> Verdict {
    if let (Some(approved), Some(staked)) = (staking_approval_amount(first), stake_amount(second)) {
        return if approved > staked {
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            }
        } else {
            Verdict::Secure
        };
    }
    Verdict::Abstain
}

/// The claimed-for account, if `tx` is a `claim` call against
/// [`REWARDS_DISTRIBUTOR`]. Only a plain, valueless `CALL` is recognized — a
/// `DELEGATECALL` executes the target's code in the Safe's own storage
/// context rather than a real call to it, and `claim` is never payable.
fn claim_account(tx: &SafeTransaction) -> Option<Address> {
    if tx.operation != Operation::Call || !tx.value.is_zero() || tx.to != REWARDS_DISTRIBUTOR {
        return None;
    }
    Some(claimCall::abi_decode(&tx.data).ok()?.account)
}

/// The approved amount, if `tx` is an ERC-20 `approve` on [`SAFE_TOKEN`]
/// naming [`STAKING`] as spender. See [`claim_account`] for why
/// `DELEGATECALL` and nonzero `tx.value` are excluded even to a legitimate
/// address.
fn staking_approval_amount(tx: &SafeTransaction) -> Option<U256> {
    if tx.operation != Operation::Call || !tx.value.is_zero() || tx.to != SAFE_TOKEN {
        return None;
    }
    let call = approveCall::abi_decode(&tx.data).ok()?;
    (call.spender == STAKING).then_some(call.amount)
}

/// The staked amount, if `tx` is a `stake` call against [`STAKING`]. See
/// [`claim_account`] for why `DELEGATECALL` and nonzero `tx.value` are
/// excluded even to a legitimate address.
fn stake_amount(tx: &SafeTransaction) -> Option<U256> {
    if tx.operation != Operation::Call || !tx.value.is_zero() || tx.to != STAKING {
        return None;
    }
    Some(stakeCall::abi_decode(&tx.data).ok()?.amount)
}
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
