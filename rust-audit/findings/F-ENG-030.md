# F-ENG-030 `NestedSafeChecker` rates any `execTransaction`-shaped call `secure` while ignoring `value`, so a full native-currency drain is affirmed

| Field                | Value                                                                          |
| -------------------- | ------------------------------------------------------------------------------ |
| Status               | Verified                                                                             |
| Crate and module     | sentinel-engine, checkers/nested.rs                                             |
| Location             | crates/sentinel-engine/src/checkers/nested.rs:42-47 (related: crates/sentinel-engine/src/main.rs:57-73, crates/sentinel-engine/src/engine/mod.rs:62-69) |
| Severity             | Critical / Critical                                                              |
| Certainty            | 99% (RW-ENG, Phase 8; E1 — reproduced end-to-end against the running service on local Anvil; was 97%, V-ENG Phase 5) |
| Assumptions involved | A2, A3, A15                                                                     |
| Tags                 | input-validation, verdict-policy, charter                                       |

## Claim

`NestedSafeChecker` returns `Verdict::Secure` for **any** `Operation::Call` to an address other than the Safe
whose calldata merely starts with the `execTransaction` selector and ABI-decodes. It never inspects
`transaction.value`, `gas_price`, `gas_token`, `refund_receiver`, or the identity of `to`.

An attacker who can propose a Safe transaction (A2: the payload is fully attacker-controlled) therefore obtains a
`secure` verdict for a transaction that transfers the Safe's entire native balance to an address of their choosing.
The `to` address does not have to be a Safe, or even a contract: a plain `CALL` with a non-empty payload to an EOA
succeeds and transfers `value`. Because the engine's chain stops at the first non-abstaining verdict
(`engine/mod.rs:66-68`) and `NestedSafeChecker` sits at position 5 of 10 (`main.rs:62`), the affirmation also
suppresses `ExcessiveApprovalChecker`, `CowChecker`, `StakingChecker`, `RefundChecker` and
`AddressPoisoningChecker`.

Under the Charter this transaction is insecure: R-4.3 makes a transaction insecure when "it sends value to a
recipient address outside the expected target set", and §3.7 requires a transaction to satisfy **all** applicable
Article IV rules before it can be called secure. A `secure` verdict here is a malicious Safe transaction rated
`secure` (PROMPT.md §8, Critical), and the sentinel bonds an approving vote on it.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| - | ----- | ----------------- | -------- | -------------- |
| 1 | The affirmation predicate looks only at `operation`, `to != safe` and the calldata selector — `value` and every refund field are absent | E2 | crates/sentinel-engine/src/checkers/nested.rs:42-47 | Q1 |
| 2 | Matching that predicate yields `Secure`, not `Abstain` | E2 | crates/sentinel-engine/src/checkers/nested.rs:30-36 | Q2 |
| 3 | The engine stops at the first non-abstaining verdict, so a `Secure` here ends the chain | E2 | crates/sentinel-engine/src/engine/mod.rs:62-69 | Q3 |
| 4 | `NestedSafeChecker` is 5th, ahead of every remaining denier | E2 | crates/sentinel-engine/src/main.rs:57-63 | Q4 |
| 5 | `BaseChecker` (position 3) cannot stop it: any `Call` to an address other than the Safe passes | E2 | crates/sentinel-engine/src/checkers/base.rs:87-93 | Q5 |
| 6 | The Charter forbids sending value outside the expected target set, and requires all Article IV rules to hold before `secure` | I (Charter text, not repo code) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:588-593 and :437-439 | Q6 |
| 7 | The module doc's ordering rationale protects only against a blocklisted `to`, not against `value` | E2 | crates/sentinel-engine/src/checkers/nested.rs:4-11 | Q7 |

**Q1** `crates/sentinel-engine/src/checkers/nested.rs:42-47`

```rust
fn is_nested_exec_transaction(tx: &SafeTransaction) -> bool {
    tx.operation == Operation::Call
        && tx.to != tx.safe
        && tx.data.starts_with(&safe::execTransactionCall::SELECTOR)
        && safe::execTransactionCall::abi_decode(&tx.data).is_ok
}
```

**Q2** `crates/sentinel-engine/src/checkers/nested.rs:30-36`

```rust
    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        if is_nested_exec_transaction(transaction) {
            Verdict::Secure
        } else {
            Verdict::Abstain
        }
    }
```

**Q3** `crates/sentinel-engine/src/engine/mod.rs:62-69`

```rust
        let mut verdict = Verdict::Abstain;
        for checker in &self.0 {
            verdict = checker.check(&transaction, &context).await;
            tracing::trace!(checker = checker.name(), ?verdict, "checker verdict");
            if verdict != Verdict::Abstain {
                break;
            }
        }
```

**Q4** `crates/sentinel-engine/src/main.rs:57-63`

```rust
    let engine = SentinelEngine::new(vec![
        Box::new(CancellationChecker),
        Box::new(EscapeHatchChecker),
        Box::new(BaseChecker),
        Box::new(BlocklistChecker::new(engine_config.blocklist)),
        Box::new(NestedSafeChecker),
        Box::new(ExcessiveApprovalChecker),
```

**Q5** `crates/sentinel-engine/src/checkers/base.rs:87-93`

```rust
fn check_calls(tx: &SafeTransaction) -> bool {
    if tx.operation != Operation::Call {
        return false;
    }
    if tx.safe != tx.to {
        return true;
    }
```

**Q6** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:588-593` and `:437-439`

```text
### R-4.3 — Value-target manipulation

#### Rule

- A transaction is insecure if it sends value to a recipient address outside the expected target set.
```

```text
- A transaction is secure only if it satisfies all applicable Article IV rules.
- If it fails any Article IV rule, it is insecure.
```

**Q7** `crates/sentinel-engine/src/checkers/nested.rs:4-11`

```rust
//! Article IV Part A already lets a Safe call any other contract freely —
//! only self-calls and delegatecalls are restricted (see
//! [`crate::checkers::BaseChecker`]). Calling another Safe's
//! `execTransaction` is just such a call: whatever the nested transaction
//! does is that Safe's own guard's concern (if it has one), not this
//! transaction's, so it's secure independent of the nested transaction's own
//! content. Runs after [`crate::checkers::BlocklistChecker`] so a nested call
//! to a known malicious `to` is still denied rather than short-circuited.
```

## Trigger

`POST /v1/security-check` with `x-request-id` and a body whose `transaction` is (all quantities hex strings,
`block` any recent block the sentinel has synced):

```json
{
  "block": "0x1500000",
  "transaction": {
    "chainId": "0x1",
    "safe":  "0x5aFE3855358E112B5647B952709E6165e1c1eEEe",
    "to":    "0x000000000000000000000000000000000000dEaD",
    "value": "0x3635c9adc5dea00000",
    "data":  "<execTransaction calldata, see below>",
    "operation": 0,
    "safeTxGas": "0x0",
    "baseGas": "0x0",
    "gasPrice": "0x0",
    "gasToken": "0x0000000000000000000000000000000000000000",
    "refundReceiver": "0x0000000000000000000000000000000000000000",
    "nonce": "0x2a"
  }
}
```

`data` is any ABI encoding of
`execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)`; the arguments are
never examined, so all-zero arguments with empty `data`/`signatures` suffice. `to` is an EOA the attacker
controls (`0x…dEaD` above stands in for it); `value` is `1000e18` wei, i.e. the Safe's whole balance in the
attacker's chosen amount.

Expected per the Charter: `{"verdict":"insecure","rule":"R-4.3"}`.
Actual per this code path: `{"verdict":"secure"}` — checker chain positions 1-4 all abstain
(Cancellation: fields are not all default; EscapeHatch: wrong selector; Base: `safe != to` so `check_calls`
returns `true`, hence `Abstain`; Blocklist: `to` is not configured), position 5 affirms and the chain breaks.

Test-vector shape for the `sentinel-test-vectors` corpus: one request/response pair as above, plus a variant with
`to` set to a contract with a payable fallback, plus a control with the same `value` and empty `data` (which must
stay `abstain`).

## Considered and rejected

- **`BaseChecker` denies it first.** No: `check_calls` returns `true` for every `Call` whose `to != safe`
  (`base.rs:91-93`, quoted above), and `check_settings_change` maps that to `Ok()` →
  `Verdict::Abstain` (`base.rs:60-68`, `base.rs:42-45`).
- **`BlocklistChecker` denies it first.** Only if the operator happens to have listed the attacker address
  (`blocklist.rs:24-32`); `blocklist` is an operator-supplied list and is allowed to be empty (`config.rs`
  `engine.blocklist`). The attacker picks a fresh address.
- **`decode` failure makes this unreachable.** `execTransactionCall::abi_decode` succeeds on any well-formed
  encoding, which the attacker constructs; the decoded values are then discarded (`nested.rs:42-47`).
- **The transaction cannot execute, so the verdict does not matter.** A plain `CALL` with calldata to an EOA
  succeeds on-chain and transfers `value`; to a contract with a payable fallback it also succeeds. Even where it
  reverted, the sentinel has already committed a `secure` vote, which is the slashable act.
- **This is the same defect as F-ENG-031 (refund leg).** No: this one drains `value` on the primary leg with
  `gasPrice == 0`; F-ENG-031 drains through the refund leg and applies to four different affirmers.
- **The Charter permits affirming a call to another contract.** Article IV Part A's R-4.1/R-4.2 are about
  settings changes and delegatecalls only; Part B's R-4.3/R-4.4 still apply to any value or authority the
  transaction moves, and §3.7 requires all of them to be satisfied.

## Remediation options

1. Require `value.is_zero && gas_price.is_zero` before affirming, mirroring `EscapeHatchChecker`
   (`escape_hatch.rs:42-45`). Cheapest change; still affirms without evidence about `to`.
2. Make the checker deny-only or abstain-only: a nested `execTransaction` is a *reason not to deny*, not evidence
   of security. Returning `Abstain` costs only the sentinel's vote on genuine nested-Safe flows, which today are
   already unverified.
3. Affirm only when `to` is verifiably a Safe at `context.block` (an RPC `getOwners`/singleton probe) **and** the
   value/refund legs are zero, moving the checker after the RPC-backed group.

Tests to add: a corpus vector for `execTransaction` calldata with non-zero `value` (must not be `secure`); one
with non-zero `gasPrice`/`refundReceiver`; one with `to` an EOA. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 88%. (Confirms lead ENG-H2; re-read all cited lines in this
  checkout at commit 2893917. Cannot execute — A9 is FALSE this run — so the class is E2, not E1.)

## Critic (C-ENG-B)

### 0. The chain-combination logic, derived independently first

Before opening this finding I re-derived the combinator from `main.rs` and `engine/mod.rs` without reading R9's
argument, because all three of R9's Criticals (F-ENG-030, F-ENG-031, F-ENG-033) rest on it and would collapse
together if it worked any other way. **It works exactly as R9 describes.**

`crates/sentinel-engine/src/engine/mod.rs:57-72`:

```rust
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
```

There is no recomputation, no aggregation, and no second pass: the loop `break`s on the first non-`Abstain`
verdict and that value is returned. The crate's own unit test pins the exact semantics R9 relies on —
`engine/mod.rs:104-120` builds `[Abstain, Secure, Insecure{R4_3}]` in the test named
`stops_at_the_first_non_abstaining_verdict` and asserts the result is `Verdict::Secure`. So a `Secure` really
does suppress every later denial, and one over-broad affirmer is sufficient. **Step verdict: supported.**

Registration order, `crates/sentinel-engine/src/main.rs:57-73`, is: 1 `CancellationChecker`, 2
`EscapeHatchChecker`, 3 `BaseChecker`, 4 `BlocklistChecker`, 5 `NestedSafeChecker`, 6
`ExcessiveApprovalChecker`, 7 `CowChecker`, 8 `StakingChecker`, 9 `RefundChecker`, 10
`AddressPoisoningChecker`. **Step verdict: supported.**

I also traced what the verdict costs downstream, which R9 established but which is load-bearing enough to
restate. `crates/sentinel/src/engine.rs:171-175` maps `Secure` → `CheckOutcome::Approved` and `Abstain` →
`CheckOutcome::Unknown`; `crates/sentinel/src/service.rs:173-179` then does:

```rust
        let (approve, reason) = match outcome {
            CheckOutcome::Approved => (true, String::new()),
            CheckOutcome::Denied(rule) => (false, rule.to_string),
            CheckOutcome::Unknown => {
                tracing::warn!(%request_id, "engine check failed; dropping request unanswered");
                return (state, Vec::new());
            }
        };
```

So `Secure` makes the sentinel **bond and cast an approving vote**, while `Abstain` makes it cast none. This
matters for the honest framing of this finding (see §2 below) and it is why "`Secure` where the correct answer
was `Abstain`" is not a cosmetic difference.

### 1. Per-claim verdicts

| # | Verdict | Note |
| - | ------- | ---- |
| 1 | **Supported** | `nested.rs:42-47` is verbatim `tx.operation == Operation::Call && tx.to != tx.safe && tx.data.starts_with(&safe::execTransactionCall::SELECTOR) && safe::execTransactionCall::abi_decode(&tx.data).is_ok`. No `value`, `gas_price`, `gas_token` or `refund_receiver` term appears anywhere in the 47-line file. |
| 2 | **Supported** | `nested.rs:30-36` returns `Verdict::Secure` on that predicate, `Abstain` otherwise. |
| 3 | **Supported** | See §0. |
| 4 | **Supported** | `main.rs:62` is the 5th `Box::new(...)`. |
| 5 | **Supported** | `base.rs:87-93`: `if tx.safe != tx.to { return true; }` inside `check_calls`, and `BaseChecker::check` (`base.rs:41-46`) maps `Ok()` to `Abstain`. I add that `BaseChecker` can *never* affirm — it returns only `Abstain` or `Insecure` — so it cannot end the chain in the attacker's favour either. |

### 2. My own analysis, and one correction to the claim's framing

I reached the same mechanism independently. The predicate is purely structural: `abi_decode` validates only the
ABI shape of the calldata, so `to` need not be a Safe, need not implement `execTransaction`, and need not be a
contract at all. A `CALL` with non-empty calldata to an EOA succeeds and delivers `value`.

**Correction, which R9 should record but which does not weaken the finding.** R9's Claim says the affirmation
"also suppresses `ExcessiveApprovalChecker`, `CowChecker`, `StakingChecker`, `RefundChecker` and
`AddressPoisoningChecker`". That is literally true, but none of those five would have *denied* this particular
transaction: `decode_target_effects` yields only a `ValueTransfer` effect, which `ExcessiveApprovalChecker`
ignores (`excessive_approval.rs:21-25`, `_ => false`), and `AddressPoisoningChecker::decode_target`
(`address_poisoning.rs:116-139`) returns `None` because `execTransaction` calldata is not an ERC-20
`transfer`/`transferFrom`/`approve`. With `nested.rs` removed from the chain the engine would answer **`Abstain`,
not `Insecure`**. The defect is therefore precisely: *the engine converts "no opinion, cast no vote" into "an
affirmative, bonded attestation that a full native drain is secure."* Under PROMPT.md §8 that is still the
Critical bullet ("a malicious Safe transaction rated `secure`") verbatim, and under Charter §3.7:439 — "A
transaction is secure only if it satisfies all applicable Article IV rules" — an affirmation reached without
R-4.3 ever being evaluated cannot be a correct `secure`. Severity **Critical** confirmed.

### 3. Trigger — is it concrete enough for QA to encode?

Yes, and I checked it end to end against A2 (Safe transaction contents attacker-controlled). Minimal vector:

- `chainId` = any (this checker never reads it), `safe` = the victim Safe, `to` = attacker EOA (any address
  `!= safe`), `value` = the Safe's full native balance, `operation` = `0` (Call), `data` =
  `abi.encodeCall(execTransaction, (…))` with every argument arbitrary but ABI-well-formed, all gas/refund
  fields `0`, `nonce` = the Safe's next nonce.
- Expected today: `secure`. Expected after a fix: `insecure R-4.3`, or at minimum `abstain`.

The one detail QA must not lose: `data` has to *decode*, not merely carry the selector — `abi_decode` is called
at `nested.rs:46`, so a bare 4-byte selector with a truncated tail returns `Abstain` and the vector silently
tests nothing.

### 4. Finding verdict

**Confirmed.** Mechanism verified, trigger verified, chain semantics verified. Certainty **86%** — `E2` is the
ceiling on this run (no toolchain, corpus unavailable under A8, so the 90+ band is unreachable per the brief);
within the 70-89 band this sits high because every step is a direct code read with no dependency-behaviour
inference. Severity **Critical** (unchanged), with the framing corrected in §2.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection.** Every step traced in this checkout at `2893917`:
`nested.rs:42-47` names `operation`, `to`, `data` and nothing else — the strings `value`, `gas_price`,
`gas_token` and `refund_receiver` do not appear anywhere in the 47-line file; `:30-36` maps that predicate
to `Secure`; `main.rs:62` is the fifth `Box::new`; `engine/mod.rs:66-68` breaks on it. `BaseChecker` cannot
intervene — `base.rs:91-93` returns `true` for any `Call` with `to != safe`, and `BaseChecker::check` maps
`Ok()` to `Abstain`, so it can never affirm either. Not `E1`; the 90-100 band stays closed.

**Certainty unchanged at 86%. Severity unchanged at Critical.**

**PoC: `rust-audit/poc/F-ENG-030/`** — four tests appended to `nested.rs`, which has no test block today.
Two are expected to fail on unfixed code.

Two details the PoC pins that a re-implementation would otherwise lose:

1. **`data` must ABI-*decode*, not merely carry the selector.** `nested.rs:46` calls `abi_decode`, so a
   bare 4-byte selector with a truncated tail abstains and the vector silently tests nothing. The PoC has
   a dedicated control test for this; if that control fails, the main assertion proves nothing.
2. **The regression test asserts `!= Secure`, not `== Insecure`.** With `nested.rs` removed from the chain
   the engine answers `Abstain`, not `Insecure` — none of the suppressed checkers would have denied *this*
   transaction. Asserting `== Insecure` would prejudge remediation option 2 out of existence.

### Remediation check

- **Option 1 (`value.is_zero && gas_price.is_zero`) — sound but incomplete, as the finding says.** It
  closes both drains in this file and mirrors `escape_hatch.rs:53`, so it is cheap and consistent. It
  still affirms with zero evidence about `to`. Ship it as a stop-gap; do not present it as the fix.
- **Option 2 (make the checker abstain-only) — sound, and my recommendation.** A nested `execTransaction`
  is a reason *not to deny*, not evidence of security. The cost is stated correctly in the finding and is
  smaller than it looks: per `crates/sentinel/src/service.rs:173-179` an `Unknown` outcome makes the
  sentinel drop the request unanswered rather than vote, so the loss is participation on genuine
  nested-Safe flows, which are unverified today in any case. The crate already contains this argument, for
  a different checker, at `refund.rs:60-67`.
- **Option 3 (probe `to` for Safe-ness at `context.block`) — sound but it changes the checker's class,
  with a consequence the finding does not state.** It makes `nested.rs` RPC-backed, so it must move behind
  `main.rs:66`'s RPC group — which inverts its position relative to `BlocklistChecker` and `CowChecker`
  and must be re-checked against F-ENG-034/F-ENG-035 before it is taken. It also inherits
  abstain-on-RPC-failure and F-ENG-009's unbounded fan-out.
- **None of the three closes the class.** Under the current combinator, any other over-broad affirmer
  reproduces the same outcome; see F-ENG-044.
- **Test hook: none needed, and its absence is a policy artefact.** The checker is a pure function of
  `SafeTransaction` — no `Provider`, no HTTP, no `CheckContext` — yet the file has zero tests, because
  `AGENTS.md` routes checker verdicts to the `sentinel-test-vectors` corpus (unavailable, A8). For a
  checker this cheap to exercise in-process the corpus is the wrong oracle; recommend the rule be narrowed
  to verdicts that genuinely depend on chain state.
- **Where the fix belongs: the checker,** plus the combinator for the class. The `RuleId` mapping is
  correct — R-4.3 exists and means what the finding needs it to mean.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 86% -> **97%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a
binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source
file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under
`rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file
was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only
executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-030/append-to-src-checkers-nested.rs` appended to
`crates/sentinel-engine/src/checkers/nested.rs`, then `cargo test -p sentinel-engine poc_f_eng_030`. Compiled
on the first attempt (one `unused_imports` warning, no error). Full output:
`rust-audit/poc/F-ENG-030/run-output.txt`.

### Verbatim result

```
running 4 tests
test checkers::nested::poc_f_eng_030::poc_f_eng_030_a_truncated_encoding_abstains ... ok
test checkers::nested::poc_f_eng_030::poc_f_eng_030_affirms_a_full_native_drain_today ... ok
test checkers::nested::poc_f_eng_030::poc_f_eng_030_a_relayed_call_must_not_be_affirmed_either ... FAILED
test checkers::nested::poc_f_eng_030::poc_f_eng_030_a_value_bearing_call_must_not_be_affirmed ... FAILED

---- ..._a_value_bearing_call_must_not_be_affirmed stdout ----
assertion `left != right` failed: R-4.3: value sent to an unvetted recipient cannot be affirmed on the \
strength of the calldata's selector alone
  left: Secure
 right: Secure

---- ..._a_relayed_call_must_not_be_affirmed_either stdout ----
assertion `left != right` failed: the gas-refund leg is a value transfer to an attacker-chosen address; \
`EscapeHatchChecker` already refuses to affirm a relayed call (escape_hatch.rs:53)
  left: Secure
 right: Secure

test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 97 filtered out
```

Both expected-to-fail tests failed for the claimed reason.

### What this establishes

- `NestedSafeChecker::check` returns `Verdict::Secure` for a plain `Operation::Call` to an attacker EOA
  (`0x…dEaD`, never probed, need not be a contract at all) carrying `value = 1000 ETH`, purely because the
  calldata is a well-formed `execTransaction` encoding whose every argument is zero. **The claim's central
  assertion — a full native drain rated `secure` — is now executed fact, not inference.**
- The control (test 2) passed: truncating the same calldata to the bare 4-byte selector makes
  `abi_decode` fail and the checker abstain. So the `Secure` in test (1) really came from the nested-exec
  predicate, and the fixture is not accidentally testing nothing.
- The refund leg (test 4) is affirmed too, at `value = 0`: `baseGas = 1e12`, `gasPrice = 1 gwei`,
  `refundReceiver = attacker`. A fix that only adds `value.is_zero` closes test (3) and leaves this open.

Residual uncertainty: the Charter mapping to R-4.3 (A7/A15) and the A2 premise that the transaction payload is
attacker-chosen. The code behaviour is settled.

## Real-world validation (Phase 8, RW-ENG)

### Scenario

A real deployment, not a harness: Anvil 1.8.1 on `127.0.0.1:8545` (chain id 31337); the Safe 1.5.0
singleton and `SafeProxyFactory` from `contracts/lib/safe-smart-account` deployed with `forge script`;
a real Safe proxy at `0x643d887734c637f108B095dc3EE0e06F79bC320C` (one owner — Anvil account 0 —
threshold 1) funded with **1000 ETH**. The attacker is Anvil account 3,
`0x90F79bf6EB2c4f870365E785982E1f101E93b906`, a **plain EOA with no code**.

The engine was the real binary (`target/debug/sentinel-engine`), started as a service from a copy of
the shipped `crates/sentinel-engine/sentinel-engine.sample.toml` with only `rpc`, `bind_address` and
`log_filter` rewritten. Effective config, logged before start:

```
rpc = "http://127.0.0.1:8545"
bind_address = "127.0.0.1:5473"
[engine]
blocklist = []
address_poisoning_lookback_blocks = 50000
```

The engine confirmed the local chain at startup: `{"method":"eth_chainId"...}` →
`Ok({"jsonrpc":"2.0","id":0,"result":"0x7a69"})` (0x7a69 = 31337). No live endpoint was contacted at
any point.

The request is the one a production sentinel makes — `POST /v1/security-check` — carrying a
proposer-supplied transaction (A2). A3 is untouched: the payload was submitted to the loopback API
exactly as the co-deployed sentinel would submit it.

### Verbatim outcome

Request:

```json
{"block":"0x9","transaction":{"chainId":"0x7a69",
 "safe":"0x643d887734c637f108B095dc3EE0e06F79bC320C",
 "to":"0x90F79bf6EB2c4f870365E785982E1f101E93b906",
 "value":"0x3635c9adc5dea00000","data":"0x6a761202...","operation":0,
 "safeTxGas":"0x0","baseGas":"0x0","gasPrice":"0x0",
 "gasToken":"0x0000000000000000000000000000000000000000",
 "refundReceiver":"0x0000000000000000000000000000000000000000","nonce":"0x0"}}
```

`value` is `0x3635c9adc5dea00000` = 1000 ETH, the Safe's entire balance. `data` is a well-formed
`execTransaction` call (selector `0x6a761202`). `to` is an EOA.

Response, verbatim:

```
{"verdict":"secure"}
```

The engine's own checker trace for that request:

```
cancellation -> Abstain
escape_hatch -> Abstain
base -> Abstain
blocklist -> Abstain
nested_safe -> Secure
(final) -> Secure
```

The affirmation comes from `nested_safe`, the fifth checker, exactly as the claim states, and the
remaining five checkers are never invoked.

The affirmed transaction was then executed against the real Safe, signed with an approved-hash
signature from the owner:

```
status               1 (success)
transactionHash      0x203f697ce3044004203ca6fca1a88eb0991518246114a29f8ddbf7806af47c9b
safe ETH before:     1000000000000000000000
safe ETH after:      0
attacker ETH before: 100000000000000000000000
attacker ETH after:  101000000000000000000000
```

The Safe's entire native balance moved to the attacker EOA on a transaction the live service rated
`secure`.

### Verdict

**Reproduced end-to-end** — on the first attempt, with no contrivance. The payload is the plainest
possible instance of the claim: a well-formed `execTransaction` calldata blob to a codeless EOA with
the Safe's whole balance attached. Nothing about the scenario is unlikely in production; the calldata
does not have to be *executable* by the recipient, only decodable by the checker, and `to` does not
have to be a Safe or even a contract.

Certainty **97% → 99%** (was E1 in-process; now E1 against a running service and a real Safe, with
the drain observed on chain). Severity **Critical / Critical**, unchanged.
