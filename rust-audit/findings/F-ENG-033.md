# F-ENG-033 `AddressPoisoningChecker` affirms `secure` from event history on an attacker-chosen `to`, and never inspects `transaction.value`

| Field                | Value                                                                          |
| -------------------- | ------------------------------------------------------------------------------ |
| Status               | Verified                                                                             |
| Crate and module     | sentinel-engine, checkers/address_poisoning.rs                                  |
| Location             | crates/sentinel-engine/src/checkers/address_poisoning.rs:116-139, :192-222, :321-333 (related: main.rs:72, engine/mod.rs:62-69) |
| Severity             | Critical / Critical                                                              |
| Certainty            | 99% (RW-ENG, Phase 8; E1 — reproduced end-to-end against the running service on local Anvil; was 96%, V-ENG Phase 5) |
| Assumptions involved | A2, A3, A4, A15                                                                 |
| Tags                 | input-validation, verdict-policy, charter                                       |

## Claim

Two independent weaknesses in the same affirmation:

**(a) `value` is never read.** `decode_target` gates only on `operation` and on the calldata decoding as an
ERC-20 `transfer`/`transferFrom`/`approve`. The affirmation at `address_poisoning.rs:332` therefore says nothing
about `transaction.value`, which is paid to `transaction.to` on execution. A `Call` carrying
`transfer(<an address the Safe has paid before>, 1)` and `value = <the Safe's entire balance>` is rated `secure`.

**(b) The evidence pool is keyed by an address the proposer chooses.** `established_recipients` filters logs on
`address = transaction.to` — the "token" — and accepts any `Transfer`/`Approval` event whose `topics[1]` is the
Safe. Emitting such an event costs nothing and requires no allowance on a contract the attacker wrote: Solidity
lets any contract emit any event. An attacker deploys `T`, calls it once so a `Transfer(from = safe, to = X,
amount = 1)` log exists at a block at or before `context.block`, then proposes `{to: T, data: transfer(X, 1)}`.
The lookup returns `ExactMatch` and the engine answers `secure`. `T::transfer` can be `payable`, which combines
(b) with (a) into a native-currency drain.

The module doc already acknowledges a narrower version of (b) — forgery through `transferFrom` on a *real* token
(`address_poisoning.rs:26-39`) — but the allowance-free variant on an attacker-deployed `to` is stronger: it
needs no interaction with any genuine token, and `to` is never checked to be a token at all.

Charter: R-4.3 makes a transaction insecure when it sends value to a recipient outside the expected target set,
and §2.4 defines that set from "the user's established onchain pattern", not from logs a counterparty
manufactured. §3.7 forbids `secure` unless every applicable Article IV rule is satisfied. A malicious Safe
transaction rated `secure` is Critical under PROMPT.md §8.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| - | ----- | ----------------- | -------- | -------------- |
| 1 | `decode_target` gates on `operation` and calldata only — `value` is absent from the whole function | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:116-139 | Q1 |
| 2 | The evidence query is filtered on `transaction.to` as the token address, with only `topics[1] == safe` constraining it | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:192-199 | Q2 |
| 3 | The first log naming the candidate short-circuits to `ExactMatch` | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:214-222 | Q3 |
| 4 | `ExactMatch` becomes `Verdict::Secure`; the transaction's `value` is not consulted anywhere in `check` | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:321-333 | Q4 |
| 5 | The module docs concede a related forgery on real tokens, and confirm `to` is treated as "the exact `token` being called" without verification | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:6-9, :26-39 | Q5 |
| 6 | The Charter's expected target set is the user's established pattern, and R-4.3 covers value recipients | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:127-141, :592 | Q6 |

**Q1** `crates/sentinel-engine/src/checkers/address_poisoning.rs:116-130` (the `approve` arm at `:132-137`
and the closing `None` at `:138-139` add no further gate; the `approve` arm is quoted at Q4 of F-ENG-036)

```rust
fn decode_target(tx: &SafeTransaction) -> Option<(Address, TargetKind)> {
    // A `DelegateCall`'s `to` isn't necessarily even a token contract, so
    // events queried against it would be meaningless.
    if tx.operation != Operation::Call {
        return None;
    }
    if let Ok(call) = transferCall::abi_decode(&tx.data) {
        return (!call.amount.is_zero).then_some((call.to, TargetKind::Transfer));
    }
    if let Ok(call) = transferFromCall::abi_decode(&tx.data) {
        // Only meaningful when `safe` is the fund source: the evidence
        // compared against is `safe`'s own outbound history, not a third
        // party's funds `safe` merely has an allowance to move.
        return (call.from == tx.safe && !call.amount.is_zero)
            .then_some((call.to, TargetKind::Transfer));
```

**Q2** `crates/sentinel-engine/src/checkers/address_poisoning.rs:192-199`

```rust
        for (chunk_from, chunk_to) in block_chunks(from_block, current_block, self.max_block_range)
        {
            let filter = Filter::new()
                .address(token)
                .event_signature(vec![Transfer::SIGNATURE_HASH, Approval::SIGNATURE_HASH])
                .topic1(safe)
                .from_block(chunk_from)
                .to_block(chunk_to);
```

**Q3** `crates/sentinel-engine/src/checkers/address_poisoning.rs:214-222`

```rust
            for (target, amount) in logs.iter.filter_map(decode_target_and_amount) {
                if amount == U256::ZERO {
                    continue;
                }
                if target == candidate {
                    return Ok(RecipientLookup::ExactMatch);
                }
                recipients.insert(target);
            }
```

**Q4** `crates/sentinel-engine/src/checkers/address_poisoning.rs:321-333`

```rust
        match self
            .established_recipients(transaction.to, transaction.safe, candidate, context.block)
            .await
        {
            Ok(RecipientLookup::ExactMatch) => {
                tracing::debug!(
                    token = %transaction.to,
                    %candidate,
                    rule = kind.rule.code,
                    "address-poisoning: genuine prior interaction found"
                );
                Verdict::Secure
            }
```

**Q5** `crates/sentinel-engine/src/checkers/address_poisoning.rs:6-9` and `:26-36`

```rust
//! Scoped narrowly: a single (non-MultiSend, non-`DelegateCall`) ERC-20
//! call, only `safe`'s own outbound `Transfer`/`Approval` history on the
//! exact `token` being called, over a bounded, operator-configured block
//! range. Native value transfers and ERC-721/1155 are out of scope.
```

```rust
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
```

**Q6** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:127-141` and `:592`

```text
### § 2.4 Expected target set

#### Definition

- The expected target set is the set of economically or permission-relevant target addresses consistent with the user’s established onchain pattern.
```

```text
- A transaction is insecure if it sends value to a recipient address outside the expected target set.
```

## Trigger

**Trigger A — variant (a) only, no attacker contract needed.** Pick any ERC-20 the Safe has paid within
`address_poisoning_lookback_blocks` of `block`, and any address `R` that appears as the `to` of one of those
`Transfer` logs. Post:

```json
{
  "block": "<a block at or after that log>",
  "transaction": {
    "chainId": "<the engine provider's chain id>",
    "safe":  "<the Safe>",
    "to":    "<that ERC-20>",
    "value": "0x3635c9adc5dea00000",
    "data":  "<transfer(R, 1)>",
    "operation": 0,
    "safeTxGas": "0x0", "baseGas": "0x0", "gasPrice": "0x0",
    "gasToken": "0x0000000000000000000000000000000000000000",
    "refundReceiver": "0x0000000000000000000000000000000000000000",
    "nonce": "0x2a"
  }
}
```

Verdict: `secure`. Whether the 1000 ETH actually leaves the Safe depends on the token's `transfer` being
`payable`; for a canonical ERC-20 it is not and the transaction reverts, so this variant is best read as
"affirmation without evidence about `value`" and as the setup for Trigger B.

**Trigger B — (a) + (b), a real drain.** On the engine's chain, deploy

```solidity
contract T {
    event Transfer(address indexed from, address indexed to, uint256 amount);
    function seed(address safe, address x) external { emit Transfer(safe, x, 1); }
    function transfer(address, uint256) external payable returns (bool) { return true; }
}
```

Call `T.seed(SAFE, X)` once at block `b`. Then post the body above with `to = T`, `data = transfer(X, 1)`,
`value = <the Safe's balance>`, `block >= b` and `block - b <= address_poisoning_lookback_blocks`. Chain order:
Cancellation abstains, EscapeHatch abstains (wrong selector), Base abstains (`to != safe`, plain `Call`),
Blocklist abstains (`T` is fresh), Nested abstains, ExcessiveApproval abstains (a `ValueTransfer` effect is not
an approval — `excessive_approval.rs:20-31`), Cow abstains (no relayer approve), Staking abstains, Refund abstains
(`gas_price == 0`), **AddressPoisoning returns `Secure`**. The Safe's whole native balance goes to `T`.

Corpus shape: Trigger A can be encoded directly against the mainnet RPC the integration runner already uses
(`scripts/run_sentinel_engine_integration_test.sh`) by picking a historical Safe/token/recipient triple; Trigger B
needs an Anvil fixture with `T` deployed and one `seed` call, then a single request/response pair. A control with
`value: "0x0"` must return the same `secure` verdict today, which is the assertion a fix would flip.

## Considered and rejected

- **`decode_target` rejects the fake token.** It only decodes calldata; nothing checks that `to` has ERC-20
  code, a `totalSupply`, or any deployment age (`address_poisoning.rs:116-139`). No `eth_call` is made anywhere
  in the crate.
- **The forged log is filtered out.** `decode_target_and_amount` only checks `topics[0]` against
  `Transfer`/`Approval` and decodes (`address_poisoning.rs:275-285`); the emitting contract is exactly the
  filter's `address`, i.e. `transaction.to`, so a self-emitted log is indistinguishable from a genuine one.
- **Zero-amount forgeries are excluded, so forgery is blocked.** Only `amount == 0` is skipped
  (`address_poisoning.rs:215-217`); `amount = 1` is accepted, and costs one unit of a token the attacker invented.
- **This is already the documented `transferFrom` gap.** The documented gap assumes a *real* token and an
  existing allowance (`address_poisoning.rs:26-31`); variant (b) needs neither, and variant (a) — `value` being
  unread — is not mentioned anywhere in the module.
- **A malicious RPC is required.** It is not: the logs are genuinely on-chain, emitted by a contract anyone may
  deploy. A4 (malicious RPC out of scope) does not apply.
- **`ExcessiveApprovalChecker` catches the `value` transfer first.** It decodes a `ValueTransfer` effect but only
  denies `Erc20Approval{U256::MAX}` / `OperatorApproval{true}` (`excessive_approval.rs:20-31`), so it abstains.
- **The `block` field lets the sentinel exclude the seeded log.** `block` is the sentinel's synced head and the
  window is `[block - lookback, block]` (`address_poisoning.rs:189`); the attacker seeds `T` before proposing, so
  the log is inside the window by construction.

## Remediation options

1. Require `tx.value.is_zero` in `decode_target` (or before affirming in `check`). Closes (a) outright; a
   payable ERC-20 call is not a shape this checker claims to understand.
2. Downgrade `ExactMatch` from `Secure` to `Abstain` — make the checker deny-only. Prior history is good evidence
   that a recipient is *not* poisoned; it is not evidence that the rest of the transaction is safe, and the
   short-circuit at `engine/mod.rs:66-68` makes an affirmation stand for the whole transaction. This is exactly
   the argument `refund.rs:60-67` already makes for the refund leg.
3. Qualify the evidence source: require `transaction.to` to have independent standing (a code-size and
   deployment-age probe, a token allow-list, or corroboration from a second token's history) before its logs
   count. Costs an RPC round trip.
4. Weight by log provenance: only count `Transfer` logs whose originating transaction was sent by the Safe
   (per-log forensics, which the module docs already name as the real fix at `address_poisoning.rs:38-39`).

Tests to add: corpus vectors for Trigger A and Trigger B; a unit test over a mocked provider asserting `Abstain`
for a non-zero `value` with an otherwise-matching history. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 85%. (Confirms lead ENG-H3 and separates it into the
  `value`-blindness half, which is pure code and needs no attacker contract, and the forged-evidence half. All
  cited lines re-read in this checkout at commit 2893917; not executed — A9 is FALSE this run.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts

Chain premise re-derived independently in F-ENG-030 §0 and confirmed. `AddressPoisoningChecker` is 10th and last
(`main.rs:72`), so nothing follows it — an affirmation here is final by construction, and there is no later
checker whose denial is being suppressed. That makes this the *purest* of R9's three Criticals: the defect is
entirely in the affirmation itself, not in the ordering.

**(a) `value` is never read — Supported.** `decode_target` (`address_poisoning.rs:116-139`) gates on
`tx.operation != Operation::Call` and then on the calldata decoding as `transfer`/`transferFrom`/`approve` with
a non-zero amount. `value` appears nowhere in the file's non-test code. The affirmation at `:325-333` returns
`Verdict::Secure` on `RecipientLookup::ExactMatch` without any further test.

**(b) The evidence pool is keyed by an attacker-chosen address — Supported.**
`established_recipients` (`:182-228`) builds its filter as `Filter::new.address(token)…topic1(safe)` where
`token` is the caller's `transaction.to` (`:322`: `self.established_recipients(transaction.to,
transaction.safe, candidate, context.block)`). Nothing anywhere checks that `transaction.to` is a token, is a
contract, or has any relationship to the Safe. The evidence test itself is `if target == candidate { return
Ok(RecipientLookup::ExactMatch); }` (`:218-220`) over logs decoded by topic0 alone (`:275-285`). Events are
unauthenticated in the EVM: any contract may emit `Transfer(address,address,uint256)` with arbitrary topics.
So the attacker supplies both the log source and the log contents.

### 2. My own analysis, reached before reading R9's

I arrived at the same two weaknesses and at the same combination. The strongest form is worth stating plainly
because it is stronger than the module's own acknowledged gap: the doc at `address_poisoning.rs:26-39` worries
about forging evidence *on a real token* via `transferFrom(safe, X, 0|1)`, which at least requires a real token
and a real allowance. The attacker does not need either. They deploy `T`, emit one
`Transfer(from=safe, to=X, amount=1)` from it, and every later query against `T` returns whatever `T`'s author
chose. The check's entire evidence base is attacker-authored.

Combining with (a): `T.transfer` is declared `payable`, the proposal carries `to = T`, `data = transfer(X, 1)`,
`value = <the Safe's whole native balance>`, and the verdict is `secure`. Charter §3.7:439 — "A transaction is
secure only if it satisfies all applicable Article IV rules" — and §2.4's definition of the target set as the
user's established onchain pattern (not logs a counterparty manufactured) both make this `insecure`. PROMPT.md
§8: a malicious Safe transaction rated `secure` is **Critical**. Severity confirmed.

As in F-ENG-030, the honest counterfactual is that without this defect the engine would answer `Abstain`, not
`Insecure` — but per `crates/sentinel/src/service.rs:173-179` that is the difference between the sentinel
casting no vote and the sentinel bonding an approving one, so the correction does not soften the finding.

### 3. Trigger — concrete enough, with two details QA must preserve

Constructible under A2 and A4. The two details that will silently void the vector if lost:

1. **The forged log must sit at or before `context.block`.** `established_recipients` scans
   `[context.block - lookback_blocks, context.block]` (`:189`, `:192`) and the module doc at `:41-46` requires
   the caller to pass a block *before* the transaction under review. So the corpus vector needs the emitting
   transaction mined at least one block earlier, and the request's `block` field set at or after it.
2. **The forged event's amount must be non-zero.** `:215-217` skips zero-amount logs (`if amount == U256::ZERO
   { continue; }`), and `decode_target` likewise requires a non-zero amount in the proposal itself (`:123`,
   `:129`, `:136`).

Also note `transaction.chain_id` must equal the configured provider's chain id (`:312`), unlike F-ENG-030's
vector which is chain-agnostic.

Because this vector needs a live chain with an attacker-deployed contract, it is the least corpus-friendly of
R9's three Criticals. QA should expect to need an Anvil fixture, not a static vector — worth flagging to
whoever schedules Phase 3.

### 4. Finding verdict

**Confirmed.** Certainty **84%** — slightly below F-ENG-030 because the trigger's "any contract may emit any
event" step is EVM semantics rather than a line in this checkout (class `I`, though it is not seriously
disputable), and because the vector depends on chain state rather than calldata alone. Severity **Critical**
(unchanged).

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection**, both variants. (a) `decode_target`
(`address_poisoning.rs:116-139`) gates on `operation` and the calldata decoding only; `check`
(`:308-333`) reads `transaction.chain_id`, `transaction.to`, `transaction.safe` and `context.block` — the
string `transaction.value` appears nowhere in the file, so the affirmation at `:332` says nothing about it.
(b) the evidence filter is `Filter::new.address(token)` where `token` is `transaction.to`
(`:193-198`), and `decode_target_and_amount` (`:275-285`) tests `topics[0]` against the two signature
hashes and nothing else, so a log emitted by the same contract the filter names is indistinguishable from
a genuine one; no `eth_call` is made anywhere in the crate, so `to` is never established to be a token.
Not `E1`; the 90-100 band stays closed.

**Certainty unchanged at 84%. Severity unchanged at Critical.**

**PoC: `rust-audit/poc/F-ENG-033/`** — five tests appended to `address_poisoning.rs`, over
`Provider::mocked` + an `Asserter`. Two are expected to fail on unfixed code, and one is a control that
must keep passing.

**The PoC replaces the finding's Anvil requirement with a queued log.** Trigger B as written calls for
deploying contract `T` on a chain; in-process, the *only* thing `T` contributes is one
`Transfer(safe → X, 1)` log, which the mocked provider supplies directly. Keep the Anvil version for the
external corpus; use the in-process one for CI. The README carries the Solidity for `T` either way, and
notes that `T::transfer` must be **`payable`** for variant (b) to be a real drain rather than a wrong
verdict on a reverting transaction.

**One thing the PoC is deliberate about:** test 5 (variant b) is kept separate from test 4 (variant a)
because **remediation option 1 closes only test 4**. Set `value` to zero in variant (b) and the
affirmation still stands on forged evidence. A reader who sees option 1 land and the suite go green on
test 4 must not close the finding.

### Remediation check

- **Option 1 (`require tx.value.is_zero` in `decode_target`) — sound for variant (a), closes nothing
  else.** Necessary, not sufficient. See above.
- **Option 2 (downgrade `ExactMatch` from `Secure` to `Abstain`, i.e. deny-only) — sound, closes both
  variants, and my recommendation.** Prior history is evidence that a recipient is not a poisoned
  lookalike; it is not evidence about the rest of the transaction. **This exact argument is already written
  in this codebase, about this same delegate, at `refund.rs:60-67`** — applied to one caller and not the
  other. Cost: the sentinel abstains on transfers to established recipients, which per
  `crates/sentinel/src/service.rs:173-179` drops the request unanswered rather than voting wrongly.
- **Option 3 (require `transaction.to` to have independent standing) — sound but expensive and
  incomplete.** A code-size / deployment-age probe costs an `eth_call` per request on the checker that is
  already the RPC hot spot (F-ENG-009: the lookback fan-out is unbounded and unvalidated), and a patient
  attacker can age a contract. A token allow-list is cheaper and stricter at the cost of per-chain
  maintenance — and it is what `CowChecker` and `StakingChecker` already do for their own addresses, so it
  is consistent with the crate's shape.
- **Option 4 (per-log provenance) — sound, and the real fix**, as the module's own docs say at
  `address_poisoning.rs:38-39`. It closes variant (b) and the already-documented `transferFrom` forgery at
  once, and it is also the durable fix for F-ENG-042 path (a). Cost: an extra RPC round trip per candidate
  log, multiplying the fan-out by the log count — **unacceptable until F-ENG-009's missing bound exists.**
  Sequence it after that.
- **Recommended combination: option 2 now, option 4 later.** Option 2 is a one-line change closing both
  variants at the cost only of affirmations the engine is not entitled to make; option 4 restores the
  affirmation safely once there is an RPC budget to pay for it.
- **Test hook: exists (`Provider::mocked`), and the file states the policy that excluded it.**
  `address_poisoning.rs:383-386` says in a comment that only the pure helpers are unit-tested because
  "`AGENTS.md`'s 'no unit tests for checkers' rule is about a checker's verdicts, which the
  `sentinel-test-vectors` corpus covers". That corpus is unavailable (A8) **and** is the wrong oracle for
  variant (b), which needs a live chain with an attacker contract deployed to express as a vector, versus
  one queued log in-process. Recommend narrowing the rule to "verdicts that depend on real chain history".
- **Where the fix belongs: the checker.** The `RuleId` mapping is correct — R-4.3 is the right rule and
  `TargetKind::rule` selects it correctly. The combinator (F-ENG-044) is what makes an affirmation here
  decide the whole transaction and remains a separate fix.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 84% -> **96%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a
binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source
file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under
`rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file
was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only
executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-033/append-to-src-checkers-address_poisoning.rs` appended to
`crates/sentinel-engine/src/checkers/address_poisoning.rs`, then `cargo test -p sentinel-engine poc_f_eng_033`.
Compiled on the first attempt. The RPC is `Provider::mocked` (chain id `0x5afe`) driven by an `alloy`
`Asserter` — no network. Full output: `rust-audit/poc/F-ENG-033/run-output.txt`.

### Verbatim result

```
running 5 tests
test checkers::address_poisoning::poc_f_eng_033::poc_f_eng_033_affirms_from_self_emitted_history_today ... ok
test checkers::address_poisoning::poc_f_eng_033::poc_f_eng_033_affirms_a_value_bearing_call_today ... ok
test checkers::address_poisoning::poc_f_eng_033::poc_f_eng_033_a_value_bearing_call_must_not_be_affirmed ... FAILED
test checkers::address_poisoning::poc_f_eng_033::poc_f_eng_033_a_self_emitted_history_must_not_affirm ... FAILED
test checkers::address_poisoning::poc_f_eng_033::poc_f_eng_033_control_a_zero_value_transfer_to_an_established_recipient ... ok

---- ..._a_value_bearing_call_must_not_be_affirmed stdout ----
assertion `left != right` failed
  left: Secure
 right: Secure

---- ..._a_self_emitted_history_must_not_affirm stdout ----
assertion `left != right` failed
  left: Secure
 right: Secure

test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 97 filtered out
```

Both expected-to-fail tests failed for the claimed reason. **Both halves of the claim are independently
executed:**

**(a) `value` is never read** — variant (a): `to = USDC`, calldata `transfer(<established recipient>, 1)`, and
`value = 1000 ETH`. One genuine `Transfer(safe -> established, 1000)` log is served. Verdict: `Secure`. The
affirmation is licensed by a one-unit ERC-20 transfer's history and says nothing about the 1000 ETH riding on
the same call.

**(b) the evidence pool is attacker-chosen** — variant (b): `to` is the attacker's own contract `T`
(`0x7777…`), the only log served is one `T` emitted about itself, and `T` is never probed for code, for a
`totalSupply`, or for deployment age. The filter's `address` is exactly `transaction.to`, and
`decode_target_and_amount` inspects only `topics[0]`, so a self-emitted log is indistinguishable from a genuine
one. Verdict: `Secure`. **Setting `value` to zero here would not help** — this half is not closed by remediation
option 1, only by options 2/3/4.

The control (test 2) passed: at `value = 0`, an exact-match recipient does not become a denial, so a fix must
not over-correct into denying that case.

Residual uncertainty: the Charter §2.4/R-4.3 mapping (A7/A15), and A4/A2 for the attacker's ability to deploy
`T` and to choose `to`. The checker's behaviour is settled.

## Real-world validation (Phase 8, RW-ENG)

### Scenario

This finding is the only engine finding that needs **real chain history**, so it was tested against
genuine `Transfer` logs on a real chain read through a real `eth_getLogs`.

Live deployment: Anvil 1.8.1 on `127.0.0.1:8545` (chain 31337); a real Safe 1.5.0 proxy at
`0x643d887734c637f108B095dc3EE0e06F79bC320C` funded with **1000 ETH**; the real `sentinel-engine`
binary on `127.0.0.1:5473`, configured from a copy of the shipped sample with
`rpc = "http://127.0.0.1:8545"` and the sample's own `address_poisoning_lookback_blocks = 50000`.
Startup log confirms the local chain: `result":"0x7a69"`.

The attacker deployed `PoisonToken` at `0x5FC8d32690cc91D4c39d9d3abcBD16989F875707` — **not a token**:
a contract with an `ERC-20`-shaped `transfer(address,uint256)` that is `payable` and forwards the
native currency it receives to the attacker, plus a `forgeHistory(from, to, amount)` that emits
`Transfer(from, to, amount)` for any `from` the caller names. Emitting an event costs nothing, needs
no allowance, and needs no interaction with the Safe.

### Verbatim outcome

Step 1 — the attacker EOA (`0x90F79bf6EB2c4f870365E785982E1f101E93b906`), signing with its **own**
key, manufactures the evidence in one transaction:

```
cast send $POISON_TOKEN "forgeHistory(address,address,uint256)" $SAFE $ATTACKER 1
status               1 (success)
blockNumber          13
```

The log genuinely exists on chain (`cast logs --address $POISON_TOKEN` returns it at block 13).

Step 2 — the attacker proposes `transfer(attacker, 1)` on their own contract, with the Safe's whole
balance attached as `value`:

```json
{"block":"0xd","transaction":{"chainId":"0x7a69",
 "safe":"0x643d887734c637f108B095dc3EE0e06F79bC320C",
 "to":"0x5FC8d32690cc91D4c39d9d3abcBD16989F875707",
 "value":"0x3635c9adc5dea00000","data":"0xa9059cbb…","operation":0,
 "safeTxGas":"0x0","baseGas":"0x0","gasPrice":"0x0", …}}
```

Response, verbatim:

```
{"verdict":"secure"}
```

The engine really did query the chain — this is a live `eth_getLogs`, not a stub:

```json
{"method":"eth_getLogs","params":[{"fromBlock":"0x0","toBlock":"0xd",
 "address":"0x5fc8d32690cc91d4c39d9d3abcbd16989f875707",
 "topics":[["0xddf252ad…23b3ef","0x8c5be1e5…c3b925"],
 "0x000000000000000000000000643d887734c637f108b095dc3ee0e06f79bc320c"]}],"id":2,"jsonrpc":"2.0"}
```

and reached its verdict on the forged log, in its own words:

```
{"message": "address-poisoning: genuine prior interaction found",
 "token": "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707",
 "candidate": "0x90F79bf6EB2c4f870365E785982E1f101E93b906", "rule": "R-4.3"}
{"message": "checker verdict", "checker": "address_poisoning", "verdict": "Secure"}
```

Step 3 — the affirmed transaction executed against the real Safe:

```
status               1 (success)
transactionHash      0xa284d0d7a13999b3c73a27dba3165615e63961c3379ab576f2b1bd439c04247a
safe ETH before:     1000000000000000000000
safe ETH after:      0
attacker ETH before: 100999999994953737302786
attacker ETH after:  101999999994953737302786
```

Both halves of the claim are confirmed together: `value` was never read (a) and the evidence pool was
keyed by an address the attacker chose (b), and the `payable` `transfer` combined them into a
1000 ETH drain.

### The F-XC-005 tension, resolved

F-XC-005 claims the shipped sample's 50,000-block single-call lookback makes this checker **abstain on
every request** against a capped provider — which, if true everywhere, would mean it can never wrongly
affirm. Both were tested against the same chain, back to back.

The chain was mined to block 15,014 so that the lookback window exceeds a realistic provider cap. A
local proxy on `127.0.0.1:8546` was placed in front of Anvil, rejecting any `eth_getLogs` whose
`toBlock - fromBlock` span exceeds 10,000 with the same error text a capped provider returns, and
forwarding everything else unchanged. (Local only — the proxy's upstream is `127.0.0.1:8545`.)

**A. Shipped sample values, capped provider** (`lookback 50000`, `max_block_range` left commented
out, exactly as shipped):

```
VERDICT: {"verdict":"abstain"}
proxy:   CAPPED eth_getLogs span=15014 > 10000
engine:  {"message": "address-poisoning history lookup failed",
          "err": "… range 15014 exceeds limit of 10000",
          "token": "0x5FC8…5707", "candidate": "0x90F7…b906", "rule": "R-4.3"}
```

**B. The sample's own documented remedy applied** (`address_poisoning_max_block_range = 10000`),
same capped provider, same payload:

```
VERDICT: {"verdict":"secure"}
proxy:   ALLOWED eth_getLogs 0x0..0x2710
engine:  {"message": "address-poisoning: genuine prior interaction found", … "rule": "R-4.3"}
```

So the two findings do not contradict each other, and neither is reduced by the other:

1. **F-XC-005 is real and reproduces**, but it is a *configuration* defect, not a property of the
   checker. It suppresses this finding only in the exact deployment where the operator takes the
   sample verbatim *and* the provider caps `eth_getLogs` below 50,000.
2. **The suppression disappears the moment the operator does the right thing.** Setting
   `address_poisoning_max_block_range` — which the sample file itself instructs the operator to do,
   and which the checker's Rust docs explain in detail — restores the false `secure` immediately. So
   does any provider without a range cap, which includes the self-hosted node a serious sentinel
   operator runs (and which this engine's unbounded, uncached per-request `eth_getLogs` effectively
   requires anyway).
3. **F-XC-005's "safe" state is not safe.** An always-abstaining address-poisoning checker is not a
   checker that refuses to be fooled; it is the only lookalike **denial** in the engine turned off.
   The deployment trades a false `secure` for a check that never fires — and F-ENG-030, F-ENG-031 and
   F-ENG-044 all still affirm drains in that same deployment, because they never reach the RPC.

The correct reading is therefore: F-XC-005 masks F-ENG-033 in one misconfigured corner of the
configuration space, and F-ENG-033 is what the operator gets the instant they leave it. Neither
finding's severity should move on account of the other.

### Verdict

**Reproduced end-to-end** against real chain history and a real `eth_getLogs`, on the first attempt,
including the on-chain drain. The forged evidence cost the attacker one ordinary transaction from
their own EOA.

Certainty **96% → 99%**. Severity **Critical / Critical**, unchanged — see the tension resolution
above for why F-XC-005 does not lower it.
