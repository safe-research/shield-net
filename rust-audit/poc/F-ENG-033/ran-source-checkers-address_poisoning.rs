//! R-4.3/R-4.4 address-poisoning check: whether an ERC-20
//! `transfer`/`transferFrom`/`approve` target has a prior genuine
//! interaction with the Safe, or closely resembles an address that does.
//! Needs onchain event history, so it runs with the engine's `Provider`.
//!
//! Scoped narrowly: a single (non-MultiSend, non-`DelegateCall`) ERC-20
//! call, only `safe`'s own outbound `Transfer`/`Approval` history on the
//! exact `token` being called, over a bounded, operator-configured block
//! range. Native value transfers and ERC-721/1155 are out of scope.
//!
//! A genuine (non-zero-value) prior event to the exact candidate returns
//! [`Verdict::Secure`]. Absent that, a candidate sharing a long enough run
//! of leading/trailing hex digits with a *different* established recipient
//! — the address-poisoning pattern the Charter's §2.4 Notes call out —
//! returns [`Verdict::Insecure`]. A novel candidate, with nothing to
//! compare it against, returns [`Verdict::Abstain`]: novelty alone isn't
//! grounds for denial. The evidence pool covers `Transfer` and `Approval`
//! together, so a lookalike of an address `safe` only ever paid still
//! denies a poisoned `approve`, and vice versa.
//!
//! Many RPC providers cap `eth_getLogs`' block range;
//! [`AddressPoisoningChecker::new`]'s `max_block_range` splits the lookback
//! into chunks to work around that (see its docs for a span-vs-count
//! gotcha worth reading before configuring it).
//!
//! Two known gaps share one root cause: a `transferFrom` event only proves
//! *some* previously-approved spender moved funds to that address, not
//! that `safe` itself chose it, and forging one needs no real allowance —
//! `transferFrom(safe, X, 0)` is always valid (any allowance covers a zero
//! amount, which is also why zero-value events are excluded from evidence
//! entirely) and even `transferFrom(safe, X, 1)` only costs a wei.
//! - It can manufacture a false [`Verdict::Secure`] for the forger's own
//!   target.
//! - Mined against a lookalike `R'` of `safe`'s real payee `R`, it can
//!   instead deny later *genuine* payments to `R` — a denial-of-service,
//!   not just a false approval.
//!
//! Fixing either needs per-log transaction forensics (whose `msg.sender`
//! actually moved the funds), not done today.
//!
//! Checks are evaluated as of [`CheckContext::block`] — the caller's own
//! declared current block — rather than this checker resolving "latest"
//! itself, which also lets a historical transaction replay against the
//! block it actually happened near. The caller must supply a block
//! *before* the transaction being checked, or the query would see that
//! transaction's own effects as if they were prior evidence.

use super::Checker;
use crate::{
    contracts::bindings::erc20::{Approval, Transfer, approveCall, transferCall, transferFromCall},
    engine::{CheckContext, Operation, RuleId, SafeTransaction, Verdict},
};
use alloy::{
    primitives::{Address, U256},
    providers::Provider as _,
    rpc::types::{Filter, Log},
    sol_types::{SolCall, SolEvent},
};
use safenet_core::provider::Provider;
use std::{collections::HashSet, num::NonZeroU64};

/// `address`'s 40 individual hex digits (nibbles), most significant first.
fn nibbles(address: Address) -> [u8; 40] {
    let mut out = [0u8; 40];
    for (i, byte) in address.as_slice().iter().enumerate() {
        out[2 * i] = byte >> 4;
        out[2 * i + 1] = byte & 0x0f;
    }
    out
}

/// Whether `candidate` reads as an eyeballed match for `established`
/// without being the same address — shares a long enough run of leading
/// and trailing hex digits to be mistaken for it in a wallet UI's
/// truncated `0x1234…5678` display, while differing in the middle. A
/// coincidental match this long (4 leading, 4 trailing) between unrelated
/// addresses is astronomically unlikely (roughly 1 in 16^8).
fn is_lookalike(established: Address, candidate: Address) -> bool {
    if established == candidate {
        return false;
    }
    let (a, b) = (nibbles(established), nibbles(candidate));
    let prefix = a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count();
    let suffix = a
        .iter()
        .rev()
        .zip(b.iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    prefix >= 4 && suffix >= 4
}

/// Which of the two poisoning-relevant ERC-20 call shapes `tx.data` decoded
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TargetKind {
    /// `transfer`/`transferFrom` — maps to [`RuleId::R4_3ValueTarget`].
    Transfer,
    /// `approve` — maps to [`RuleId::R4_4AuthorizationTarget`].
    Approval,
}

impl TargetKind {
    fn rule(self) -> RuleId {
        match self {
            Self::Transfer => RuleId::R4_3ValueTarget,
            Self::Approval => RuleId::R4_4AuthorizationTarget,
        }
    }
}

/// Decodes `tx.data` as an ERC-20 `transfer`/`transferFrom`/`approve` call,
/// returning the recipient/spender address and which kind it is. `None`
/// for anything out of scope for this check — including a MultiSend batch
/// (recursing through batched sub-calls is deferred; see module docs).
fn decode_target(tx: &SafeTransaction) -> Option<(Address, TargetKind)> {
    // A `DelegateCall`'s `to` isn't necessarily even a token contract, so
    // events queried against it would be meaningless.
    if tx.operation != Operation::Call {
        return None;
    }
    if let Ok(call) = transferCall::abi_decode(&tx.data) {
        return (!call.amount.is_zero()).then_some((call.to, TargetKind::Transfer));
    }
    if let Ok(call) = transferFromCall::abi_decode(&tx.data) {
        // Only meaningful when `safe` is the fund source: the evidence
        // compared against is `safe`'s own outbound history, not a third
        // party's funds `safe` merely has an allowance to move.
        return (call.from == tx.safe && !call.amount.is_zero())
            .then_some((call.to, TargetKind::Transfer));
    }
    if let Ok(call) = approveCall::abi_decode(&tx.data) {
        // `approve(spender, 0)` is the standard way to *revoke* an
        // allowance — including to a poisoned lookalike — and must never
        // itself be denied.
        return (!call.amount.is_zero()).then_some((call.spender, TargetKind::Approval));
    }
    None
}

/// Runs the R-4.3/R-4.4 address-poisoning check (see module docs for its
/// current scope and limitations).
pub struct AddressPoisoningChecker {
    provider: Provider,
    lookback_blocks: u64,
    max_block_range: Option<NonZeroU64>,
}

impl AddressPoisoningChecker {
    /// `max_block_range` is the widest `toBlock - fromBlock` *span* the
    /// configured provider allows per `eth_getLogs` call — **not** a block
    /// count (100..110 has a span of 10 but covers 11 block numbers; if a
    /// provider caps an inclusive block count `N`, pass `N - 1`). Matches
    /// how e.g. Infura's own error reports it ("range 50000 exceeds limit
    /// of 10000"). `None` issues the whole lookback window as one call.
    pub fn new(
        provider: Provider,
        lookback_blocks: u64,
        max_block_range: Option<NonZeroU64>,
    ) -> Self {
        Self {
            provider,
            lookback_blocks,
            max_block_range,
        }
    }

    /// Looks up whether `safe` has a genuine (non-zero-value) prior
    /// `Transfer` *or* `Approval` to `candidate` on `token`, within
    /// `lookback_blocks` of `current_block`. Returns as soon as `candidate`
    /// itself turns up (an exact match settles the verdict on its own);
    /// otherwise every other established address seen is returned to
    /// compare `candidate` against for a lookalike. Chunked per
    /// `max_block_range`, one `eth_getLogs` call each, sequentially so a
    /// capped provider isn't hit with parallel requests for what is really
    /// a single lookup.
    ///
    /// A chunk failing stops the scan rather than retrying into an
    /// already-erroring provider, but doesn't discard evidence already
    /// gathered — see [`RecipientLookup::NoExactMatch`] for why an
    /// incomplete scan still can't be used to deny.
    async fn established_recipients(
        &self,
        token: Address,
        safe: Address,
        candidate: Address,
        current_block: u64,
    ) -> Result<RecipientLookup, alloy::transports::TransportError> {
        let from_block = current_block.saturating_sub(self.lookback_blocks);
        let mut recipients = HashSet::new();
        let mut complete = true;
        for (chunk_from, chunk_to) in block_chunks(from_block, current_block, self.max_block_range)
        {
            let filter = Filter::new()
                .address(token)
                .event_signature(vec![Transfer::SIGNATURE_HASH, Approval::SIGNATURE_HASH])
                .topic1(safe)
                .from_block(chunk_from)
                .to_block(chunk_to);
            let logs = match self.provider.get_logs(&filter).await {
                Ok(logs) => logs,
                Err(err) if !recipients.is_empty() => {
                    tracing::warn!(
                        %err,
                        chunk_from,
                        chunk_to,
                        "address-poisoning: chunk lookup failed, using the partial (incomplete) scan gathered so far"
                    );
                    complete = false;
                    break;
                }
                Err(err) => return Err(err),
            };
            for (target, amount) in logs.iter().filter_map(decode_target_and_amount) {
                if amount == U256::ZERO {
                    continue;
                }
                if target == candidate {
                    return Ok(RecipientLookup::ExactMatch);
                }
                recipients.insert(target);
            }
        }
        Ok(RecipientLookup::NoExactMatch {
            recipients,
            complete,
        })
    }
}

/// The result of [`AddressPoisoningChecker::established_recipients`].
enum RecipientLookup {
    /// The candidate itself already has a genuine prior interaction.
    ExactMatch,
    /// No exact match among the addresses actually scanned, to compare the
    /// candidate against for a lookalike. `complete` is `false` when a
    /// chunk failure cut the scan short: the unscanned remainder could
    /// have held the candidate's own genuine event (i.e. `ExactMatch`), so
    /// a lookalike found here still isn't grounds for denial unless
    /// `complete` is `true`.
    NoExactMatch {
        recipients: HashSet<Address>,
        complete: bool,
    },
}

/// Splits the inclusive block range `[from, to]` into consecutive `(from,
/// to)` chunks no wider than `max_range` (a `toBlock - fromBlock` span, not
/// a block count) each. `None` returns the whole range as a single chunk.
fn block_chunks(
    from: u64,
    to: u64,
    max_range: Option<NonZeroU64>,
) -> impl Iterator<Item = (u64, u64)> {
    let max_range = max_range.unwrap_or(NonZeroU64::MAX).get();
    let mut start = from;
    let mut done = false;
    std::iter::from_fn(move || {
        if done {
            return None;
        }
        let end = start.saturating_add(max_range).min(to);
        let chunk = (start, end);
        done = end >= to;
        start = end.saturating_add(1);
        Some(chunk)
    })
}

/// Decodes a `Transfer`/`Approval` log's non-`safe` indexed address and its
/// (non-indexed) amount, using the log's own topic0 to tell the two event
/// shapes apart (the query matches both — see
/// [`AddressPoisoningChecker::established_recipients`]). `None` if the log
/// doesn't decode as either shape.
fn decode_target_and_amount(log: &Log) -> Option<(Address, U256)> {
    match log.inner.data.topics().first() {
        Some(&Transfer::SIGNATURE_HASH) => Transfer::decode_log_data(&log.inner.data)
            .ok()
            .map(|event| (event.to, event.amount)),
        Some(&Approval::SIGNATURE_HASH) => Approval::decode_log_data(&log.inner.data)
            .ok()
            .map(|event| (event.spender, event.amount)),
        _ => None,
    }
}

#[async_trait::async_trait]
impl Checker for AddressPoisoningChecker {
    fn name(&self) -> &'static str {
        "address_poisoning"
    }

    /// Decodes `transaction.data`'s ERC-20 target (if any) and compares it
    /// against the Safe's own recent genuine outbound history on that token.
    /// An exact match returns [`Verdict::Secure`]; a lookalike of a
    /// *different* established recipient returns [`Verdict::Insecure`]; a
    /// candidate with nothing in that history to compare against — along
    /// with no ERC-20 target to check, a `chain_id` mismatch between the
    /// transaction and the configured provider, or the lookup itself
    /// failing — returns [`Verdict::Abstain`], deferring to whatever checker
    /// runs next.
    ///
    /// TODO(follow-up): a first-time-looking recipient with no established
    /// address to compare against still only ever abstains — richer
    /// recipient-quality signals (the candidate's own fund/transaction
    /// history, whether it's an EOA or a contract, and if so its deployment
    /// age) are needed before that case can be safely denied too.
    async fn check(&self, transaction: &SafeTransaction, context: &CheckContext) -> Verdict {
        let Some((candidate, kind)) = decode_target(transaction) else {
            return Verdict::Abstain;
        };
        if transaction.chain_id != U256::from(self.provider.chain_id()) {
            tracing::warn!(
                tx_chain_id = %transaction.chain_id,
                provider_chain_id = self.provider.chain_id(),
                "address-poisoning check: transaction chain id does not match the configured provider"
            );
            return Verdict::Abstain;
        }

        match self
            .established_recipients(transaction.to, transaction.safe, candidate, context.block)
            .await
        {
            Ok(RecipientLookup::ExactMatch) => {
                tracing::debug!(
                    token = %transaction.to,
                    %candidate,
                    rule = kind.rule().code(),
                    "address-poisoning: genuine prior interaction found"
                );
                Verdict::Secure
            }
            Ok(RecipientLookup::NoExactMatch {
                recipients,
                complete,
            }) => {
                let Some(established) = recipients.iter().find(|&&r| is_lookalike(r, candidate))
                else {
                    tracing::debug!(
                        token = %transaction.to,
                        %candidate,
                        rule = kind.rule().code(),
                        "address-poisoning: no established recipient to compare against"
                    );
                    return Verdict::Abstain;
                };
                if !complete {
                    // See `RecipientLookup::NoExactMatch` — can't deny on
                    // an incomplete scan.
                    tracing::warn!(
                        token = %transaction.to,
                        %candidate,
                        %established,
                        rule = kind.rule().code(),
                        "address-poisoning: candidate resembles an established recipient, but the scan was incomplete; abstaining rather than denying on partial evidence"
                    );
                    return Verdict::Abstain;
                }
                tracing::debug!(
                    token = %transaction.to,
                    %candidate,
                    %established,
                    rule = kind.rule().code(),
                    "address-poisoning: candidate is a lookalike of an established recipient"
                );
                Verdict::Insecure { rule: kind.rule() }
            }
            Err(err) => {
                tracing::warn!(
                    %err,
                    token = %transaction.to,
                    %candidate,
                    rule = kind.rule().code(),
                    "address-poisoning history lookup failed"
                );
                Verdict::Abstain
            }
        }
    }
}

// Only the chain-independent pure helpers below are unit-tested here —
// AGENTS.md's "no unit tests for checkers" rule is about a checker's
// verdicts, which the sentinel-test-vectors corpus covers; it doesn't cover
// generic, deterministic logic like this that has nothing to do with any
// one Charter rule.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nibbles_decomposes_most_significant_first() {
        let address = Address::new([0x12; 20]);
        assert_eq!(nibbles(address).as_slice(), [1u8, 2].repeat(20).as_slice());
    }

    #[test]
    fn is_lookalike_requires_both_thresholds_and_a_different_address() {
        // Shares all but the middle two bytes.
        let established = Address::new([
            0xaa, 0xaa, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x22, 0x22,
        ]);
        let lookalike = Address::new([
            0xaa, 0xaa, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x33, 0x22, 0x22,
        ]);
        assert!(is_lookalike(established, lookalike));

        // The identical address is never its own lookalike.
        assert!(!is_lookalike(established, established));

        // One nibble short of the leading threshold (3 shared, not 4).
        let short_prefix = Address::new([
            0xaa, 0xab, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x22, 0x22,
        ]);
        assert!(!is_lookalike(established, short_prefix));

        // One nibble short of the trailing threshold (3 shared, not 4).
        let short_suffix = Address::new([
            0xaa, 0xaa, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
            0x11, 0x11, 0x11, 0x11, 0x32, 0x22,
        ]);
        assert!(!is_lookalike(established, short_suffix));

        // A matching prefix alone, with an unrelated suffix, isn't enough.
        let mut prefix_only_bytes = [0x99u8; 20];
        prefix_only_bytes[0] = 0xaa;
        prefix_only_bytes[1] = 0xaa;
        let prefix_only = Address::new(prefix_only_bytes);
        assert!(!is_lookalike(established, prefix_only));
    }

    /// Shorthand for a non-zero `max_range` argument in tests.
    fn nz(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }

    #[test]
    fn block_chunks_covers_the_whole_range_without_gaps_or_overlap() {
        let chunks = |from, to, max_range| block_chunks(from, to, max_range).collect::<Vec<_>>();
        assert_eq!(chunks(0, 10, Some(nz(5))), vec![(0, 5), (6, 10)]);
        assert_eq!(chunks(0, 10, Some(nz(4))), vec![(0, 4), (5, 9), (10, 10)]);
        assert_eq!(chunks(0, 10, Some(nz(10))), vec![(0, 10)]);
        assert_eq!(chunks(5, 5, Some(nz(1))), vec![(5, 5)]);
    }

    #[test]
    fn block_chunks_is_a_single_chunk_when_unset() {
        assert_eq!(
            block_chunks(0, 100_000, None).collect::<Vec<_>>(),
            vec![(0, 100_000)]
        );
    }
}
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
