# F-ENG-006 `decode_target_effects` recurses through MultiSend with no depth limit; the only thing keeping attacker-chosen depth away from it is undocumented, untested checker ordering

| Field                | Value                                                                                     |
| -------------------- | ----------------------------------------------------------------------------------------- |
| Status               | QA-done                                                                                         |
| Crate and module     | sentinel-engine, `contracts/target_effects.rs`                                              |
| Location             | `crates/sentinel-engine/src/contracts/target_effects.rs:44-52` (related: `crates/sentinel-engine/src/checkers/excessive_approval.rs:19-20`; `crates/sentinel-engine/src/checkers/base.rs:205-213`, `151-154`, `87-90`; `crates/sentinel-engine/src/main.rs:57-73`; `crates/sentinel-engine/src/contracts/multi_send.rs:142-151`) |
| Severity             | Low / Low |
| Certainty            | 80% |
| Assumptions involved | A2, A6                                                                                      |
| Tags                 | dos                                                                                         |

## Claim

`decode_target_effects` recurses into itself for every sub-transaction of a MultiSend batch, with no depth
parameter, no depth counter and no bound of any kind. Depth is a function of the attacker's calldata: under
A2 the transaction contents are fully attacker-controlled, and each additional level of nesting costs roughly
150 bytes of `data` (an 85-byte packed entry wrapping a `multiSend(bytes)` ABI envelope), so a request body at
axum's default limit encodes on the order of several thousand levels.

**It is not reachable today, and I verified that rather than assuming it.** The function has exactly one
caller in the whole workspace — `ExcessiveApprovalChecker`, checker #6 — and `BaseChecker`, checker #3,
denies every batch containing a nested MultiSend before the chain gets there: `decode_multi_send_call`
recurses only into `DelegateCall` sub-transactions, and a `DelegateCall` sub-transaction to a MultiSend
address fails both arms of `check_multi_send`'s test (`check_calls` rejects non-`Call` operations;
`check_delegate_calls` has MultiSend in none of its three address lists), so `all(...)` is false and the
verdict is `Insecure R-4.2`, which breaks the chain. The maximum recursion depth actually reachable through
the shipped chain is therefore **2**, not unbounded.

So the finding is about the shield, not the recursion. That shield is a five-way coincidence between
`main.rs`'s checker order, `check_multi_send`'s `all(...)`, `check_delegate_calls`' address lists,
`decode_multi_send_call`'s `DelegateCall` requirement and the chain's break-on-first-non-abstain — and **it
is written down nowhere and tested nowhere**. `target_effects.rs:44-45` describes the recursion without
mentioning depth. `excessive_approval.rs` carries no note that it is safe only because another checker ran
first. `main.rs:66-70`, the one comment in the crate that discusses ordering as a correctness property,
reasons only about `RefundChecker`. The recursion test at `target_effects.rs:419-453` exercises a single
level. Any of the following re-exposes it, with nothing to catch the change: moving `ExcessiveApprovalChecker`
ahead of `BaseChecker`; adding a MultiSend address to `check_delegate_calls`; relaxing `check_multi_send` to
`any(...)` or to a per-sub-call verdict (which the TODO at `base.rs:198-204` explicitly contemplates); or a
second caller of `decode_target_effects` from an earlier checker.

The failure mode if it is re-exposed is worse than the usual "panic is a 500 is a missing vote". A stack
overflow in a tokio worker is not an unwinding panic — it aborts the process, so one request takes the whole
engine down and every concurrent check with it. That is why a latent defect with no current trigger is still
worth writing down.

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| - | ----- | ----- | -------- | -------------- |
| 1 | The recursion has no depth limit and no depth parameter; the doc comment does not mention depth. | E2 | `crates/sentinel-engine/src/contracts/target_effects.rs:44-52` | <pre>/// Decodes the target effects of a Safe transaction, recursing through<br>/// MultiSend so each batched sub-call is decoded individually.<br>pub fn decode_target_effects(tx: &SafeTransaction) -> Vec<TargetEffect> {<br>    if let Some((sub_txs, _)) = decode_multi_send_call(tx) {<br>        return sub_txs.iter.flat_map(decode_target_effects).collect;<br>    }<br><br>    decode_call(tx)<br>}</pre> |
| 2 | It has exactly one non-test caller in the workspace, `ExcessiveApprovalChecker`. | E2 | `crates/sentinel-engine/src/checkers/excessive_approval.rs:19-20` (established by `grep -rn "decode_target_effects" crates/ --include=*.rs`, whose only non-test hits are `target_effects.rs:46-48` and this line) | <pre>    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {<br>        for effect in decode_target_effects(transaction) {</pre> |
| 3 | That checker is #6, three positions after `BaseChecker`, and the chain breaks at the first non-abstain. | E2 | `crates/sentinel-engine/src/main.rs:57-65` | <pre>    let engine = SentinelEngine::new(vec![<br>        Box::new(CancellationChecker),<br>        Box::new(EscapeHatchChecker),<br>        Box::new(BaseChecker),<br>        Box::new(BlocklistChecker::new(engine_config.blocklist)),<br>        Box::new(NestedSafeChecker),<br>        Box::new(ExcessiveApprovalChecker),</pre> |
| 4 | The chain stops at the first non-`Abstain` verdict, so a `BaseChecker` denial prevents checker #6 from running at all. | E2 | `crates/sentinel-engine/src/engine/mod.rs:62-69` | <pre>        let mut verdict = Verdict::Abstain;<br>        for checker in &self.0 {<br>            verdict = checker.check(&transaction, &context).await;<br>            tracing::trace!(checker = checker.name, ?verdict, "checker verdict");<br>            if verdict != Verdict::Abstain {<br>                break;<br>            }<br>        }</pre> |
| 5 | `BaseChecker` requires *every* sub-transaction to pass, and a nested MultiSend delegatecall passes neither arm. | E2 | `crates/sentinel-engine/src/checkers/base.rs:205-213` | <pre>fn check_multi_send(tx: &SafeTransaction) -> bool {<br>    let Some((sub_txs, allows_delegate_calls)) = decode_multi_send_call(tx) else {<br>        return false;<br>    };<br><br>    sub_txs.iter.all(\|sub_tx\| {<br>        check_calls(sub_tx) \|\| (allows_delegate_calls && check_delegate_calls(sub_tx))<br>    })<br>}</pre> |
| 6 | `check_calls` rejects any sub-transaction that is not a `Call`, so a nested MultiSend delegatecall fails the first arm. | E2 | `crates/sentinel-engine/src/checkers/base.rs:87-90` | <pre>fn check_calls(tx: &SafeTransaction) -> bool {<br>    if tx.operation != Operation::Call {<br>        return false;<br>    }</pre> |
| 7 | Recursion requires `DelegateCall` at every level, which is exactly what the shield rejects — a `Call` to a MultiSend address is not recursed into. | E2 | `crates/sentinel-engine/src/contracts/multi_send.rs:142-151` | <pre>pub fn decode_multi_send_call(tx: &SafeTransaction) -> Option<(Vec<SafeTransaction>, bool)> {<br>    if tx.operation != Operation::DelegateCall {<br>        tracing::trace!(<br>            to = %tx.to,<br>            operation = ?tx.operation,<br>            "not a MultiSend batch: top-level call is not a delegatecall, so any sub-calls \<br>             would run with the MultiSend contract itself as their sender, not the Safe"<br>        );<br>        return None;<br>    }</pre> |
| 8 | `check_delegate_calls` returns false for a MultiSend address, so the second arm fails too: the function ends at `false` after three address-list tests, none of which contains a MultiSend deployment. | E2 | `crates/sentinel-engine/src/checkers/base.rs:181-192` | <pre>    const CREATE_CALL_CONTRACTS: &[Address] = &[<br>        address!("7cbB62EaA69F79e6873cD1ecB2392971036cFAa4"), // 1.3.0 - canonical<br>        address!("B19D6FFc2182150F8Eb585b79D4ABcd7C5640A9d"), // 1.3.0 - eip155<br>        address!("9b35Af71d77eaf8d7e40252370304687390A1A52"), // 1.4.1<br>        address!("2Ef5ECfbea521449E4De05EDB1ce63B75eDA90B4"), // 1.5.0<br>    ];<br>    if CREATE_CALL_CONTRACTS.contains(&tx.to) {<br>        return tx.data.starts_with(&safe::performCreateCall::SELECTOR)<br>            \|\| tx.data.starts_with(&safe::performCreate2Call::SELECTOR);<br>    }<br><br>    false</pre> |
| 9 | The one existing recursion test covers a single level of nesting, so no test would fail if the shield were removed. | E2 | `crates/sentinel-engine/src/contracts/target_effects.rs:419-429` | <pre>    #[test]<br>    fn recurses_through_multi_send {<br>        let approve_data = erc20::approveCall {<br>            spender: RECIPIENT,<br>            amount: U256::MAX,<br>        }<br>        .abi_encode;<br>        let data = multisend(&[<br>            pack(Operation::Call, RECIPIENT, U256::from(2u64), &[]),<br>            pack(Operation::Call, TOKEN, U256::ZERO, &approve_data),<br>        ]);</pre> |
| 10 | A body at axum's default 2 MiB limit encodes several thousand nesting levels, and a tokio worker's default stack is 2 MiB, so the depth is of the same order as the stack budget. | I | Both numbers are documented library defaults for axum 0.8.9 and tokio 1.x whose sources are not on this machine (A6); nothing was executed, so the per-frame stack cost — and therefore whether an overflow is actually reached — is unmeasured. | *(no verbatim quote available)* |

## Trigger

**None identified in the shipped checker chain**, and I state that positively rather than as an absence of
effort: I traced every caller and re-derived the shield (basis rows 2-8). The reachable depth is 2.

The trigger *if the shield is removed* is a body with `operation: 1`, `to` = a MultiSend deployment that
allows delegate calls (e.g. `0x218543288004CD07832472D464648173c77D7eB7`, `multi_send.rs:29-31`), and `data`
= `multiSend(bytes)` whose single packed entry is `{operation: 1, to: <the same MultiSend address>,
value: 0, data: <the same structure, one level shallower>}`, repeated to the depth the body limit allows.
Each level is one `decode_target_effects` frame.

The four concrete changes that remove the shield, each of which is a plausible ordinary edit:
`ExcessiveApprovalChecker` moved before `BaseChecker` in `main.rs:57-73`; a MultiSend address added to any
list in `check_delegate_calls`; `check_multi_send`'s `all(...)` relaxed while implementing the per-sub-call
rule attribution the TODO at `base.rs:198-204` describes; or a second caller of `decode_target_effects` added
to a checker that runs before #3.

## Considered and rejected

- **"The recursion is reachable today, so this is High."** Rejected — this is the claim I most wanted to be
  true and it is not. I checked each link: `decode_multi_send_call` needs `DelegateCall` (row 7), so a `Call`
  to a MultiSend address — which `check_calls` *does* allow at `base.rs:91-93`, since `to != safe` returns
  `true` — is never recursed into; and a `DelegateCall` sub-entry is denied by `check_multi_send` (rows 5, 6,
  8) before checker #6 runs (rows 3, 4). Depth 2 is the ceiling. Anyone re-checking this finding should start
  here.
- **"Some other decoder recurses too."** Rejected. `decode_multi_send` (`multi_send.rs:92-133`) is a flat
  `while` loop over a checked cursor with no recursion at all; nested batches come back as opaque
  `DelegateCall` sub-transactions. `sub_transactions` (`multi_send.rs:168-172`) is one level by construction.
  `decode_target_effects` is the only recursive decoder in the crate.
- **"Unbounded memory is the real risk, not stack depth."** Rejected: allocation is linear in the input.
  `decode_multi_send` copies one `Bytes` per entry (`multi_send.rs:109`) from a slice the cursor has already
  bounds-checked, so total allocation is O(body). A declared `dataLength` larger than the remaining input
  allocates nothing — `Cursor::read` returns `None` from `split_at_checked` before the copy
  (`multi_send.rs:181-185`).
- **"`serde_json`'s recursion limit already caps nesting."** Rejected as irrelevant here: the nesting is
  inside a single hex string in the `data` field, not in the JSON structure, so serde never sees it.
- **"A stack overflow would be caught as a panic and returned as a 500."** Rejected, and this is why the
  latent severity is high even though the current severity is Low: a Rust stack overflow aborts the process
  rather than unwinding, so it is not catchable by `catch_unwind` or by a `CatchPanicLayer` (which is not
  enabled anyway — the workspace takes `tower-http` with only `trace`, `Cargo.toml:24`). One request would
  kill the engine and every concurrent check.
- **"Row 10 is unverified, so the finding is speculative."** The *defect* — an unbounded recursion over
  attacker-controlled depth — is established by row 1 alone and needs no library facts. Row 10 only sizes the
  consequence, and I have marked it `I`. Even if the reachable depth turned out to be an order of magnitude
  below the stack budget, an unbounded recursion in an attacker-facing decoder with an undocumented,
  untested shield is worth a depth cap.

## Remediation options

1. **Add an explicit depth cap and return no effects past it.** Give `decode_target_effects` a private
   `depth` parameter (public wrapper unchanged) and stop at a small constant — 2 or 3 covers every legitimate
   Safe batch and is generous against real MultiSend usage. Cheapest, entirely local to
   `target_effects.rs`, and it makes the function safe independent of anything the checkers do. Tradeoff: the
   effects of a batch nested deeper than the cap go unenumerated, so `ExcessiveApprovalChecker` would miss an
   approval hidden below it — acceptable given that `BaseChecker` denies such a batch outright today, but the
   cap should be *at least* as deep as whatever `check_multi_send` is ever relaxed to accept.
2. **Convert the recursion to an explicit work queue with a bounded budget.** Replace the `flat_map` recursion
   with a `Vec` worklist and a maximum number of sub-transactions processed. Removes the stack entirely as a
   failure mode and bounds total work rather than only depth. Tradeoff: a little more code for a function that
   is currently four lines.
3. **Write the shield down and pin it.** Independent of 1 and 2: add a comment at
   `excessive_approval.rs:19-20` and at `target_effects.rs:44-45` naming `BaseChecker`'s precedence as the
   reason unbounded depth cannot arrive, and extend `main.rs:66-70`'s ordering comment — the crate's only
   statement that ordering is a correctness property — to cover this dependency too. Do not rely on this
   alone; a comment is not a bound.

Tests to add: a `target_effects.rs` case decoding a batch nested to the cap and one past it, asserting
termination and the expected effect list; and an integration-level case asserting that the shipped checker
order denies a nested batch at `BaseChecker` (there is currently no test over the real `main.rs` chain at
all — `engine/mod.rs:79-90` uses stubs).

## Trail

- Reviewer R8: drafted, self-estimate 85%. Confirms ENG-H11's latent form and, more usefully,
  *refutes* its reachability with the specific citations (rows 5-8) rather than repeating the prior analysis's
  assertion. Row 10 is class `I` (A6). Severity Low reflects that nothing triggers it today; the reason to
  file it anyway is that the shield is undocumented, untested, and four one-line edits away from gone, and
  the failure mode is a process abort rather than a 500. Files touched by the shield
  (`checkers/base.rs`, `checkers/excessive_approval.rs`) are R9's scope.

## Critic (C-ENG-A)

The parent brief asked me to verify the depth-2 bound independently, since an accidental invariant is
exactly what a later refactor removes silently. **I re-derived it from scratch, without reading R8's
reasoning first, and I get the same answer.** No claim here is `H`; the one row that rests on library
defaults (row 10, axum's 2 MiB body limit and tokio's 2 MiB worker stack) is correctly marked `I`.

### Independent derivation of the bound

Recursion in `decode_target_effects` (`target_effects.rs:46-52`) requires `decode_multi_send_call` to return
`Some`, which requires all three of: `tx.operation == Operation::DelegateCall`
(`multi_send.rs:143-151`), `tx.to` in `DEPLOYMENTS` (`multi_send.rs:152-155`, via `known_deployment`), and
`tx.data` decoding as `multiSend(bytes)` (`multi_send.rs:156-159`). So each extra level costs one
`DelegateCall` sub-entry pointing at a MultiSend address.

`BaseChecker` is checker #3 (`main.rs:60`) and `ExcessiveApprovalChecker`, the sole non-test caller of
`decode_target_effects`, is #6 (`main.rs:63`); `engine/mod.rs:63-69` breaks the loop at the first
non-`Abstain`. For a top-level MultiSend delegatecall, `check_multi_send` (`base.rs:205-213`) requires
**every** sub-tx to satisfy `check_calls(sub_tx) || (allows_delegate_calls && check_delegate_calls(sub_tx))`.
A nested-MultiSend sub-tx fails both: `check_calls` returns `false` at `base.rs:88-90` for a non-`Call`, and
`check_delegate_calls` (`base.rs:151-192`) ends at `false` because a MultiSend address is in none of
`MIGRATION_CONTRACTS`, `SIGN_MESSAGE_LIBS` or `CREATE_CALL_CONTRACTS`. So `all(..)` is false, the verdict is
`Insecure { R4_2DelegatecallIntegrity }`, and checker #6 never runs.

I also closed the two paths R8 did not spell out. **Checkers #1 and #2 do not recurse:**
`cancellation.rs:15-28` compares against a constructed default and `escape_hatch.rs:36-46` matches selectors
— neither calls `decode_target_effects` or `sub_transactions`, and a workspace-wide grep for
`decode_target_effects` returns one non-test caller. **A top-level `Call` to a MultiSend address does not
recurse either**, because `decode_multi_send_call` requires `DelegateCall`; it takes `decode_call` and
stops. So the only surviving shape is: top-level MultiSend delegatecall (frame 1) whose sub-txs are all
plain `Call`s (frame 2, no further recursion). **Maximum reachable depth is 2.** Confirmed.

### The claim is about the shield, and the shield is real and undocumented

I checked each of the four re-exposure routes R8 names and all four are live edits:

- `main.rs:57-73` is a plain `vec![..]` with no ordering assertion; moving `ExcessiveApprovalChecker` above
  `BaseChecker` is a one-line change. The only comment in the file that reasons about ordering as
  correctness (`main.rs:66-70`) discusses `RefundChecker` and says nothing about this.
- `base.rs:198-204` is an existing TODO contemplating exactly the change ("this function returning the
  failing sub-tx's own rule instead of a flat `bool`") that would relax `all(..)`.
- `target_effects.rs:44-45` documents the recursion without mentioning depth; `excessive_approval.rs` has no
  note that it is safe only because checker #3 ran.
- The recursion test at `target_effects.rs:419-453` exercises a single level, so no test fails if the shield
  goes.

### Severity and certainty

**Low / Low — I confirm the reviewer's band**, and record that Informational is also defensible so the
Documentation agent does not have to re-litigate it. The deciding considerations: (i) there is genuinely no
trigger today, which is the brief's own test for Low-or-Informational; but (ii) this is a missing bound in
code, not a documentation gap, and (iii) the failure mode if re-exposed is not the usual "panic → 500 →
missing vote" — a stack overflow trips the guard page and `abort`s the process, taking every concurrent
check with it, and `tower-http`'s `catch-panic` feature is not enabled anyway (root `Cargo.toml:24`), so
nothing in the engine would convert even an ordinary panic into a response. Low is right; the fix is a depth
counter and an assertion in `main.rs` that `BaseChecker` precedes `ExcessiveApprovalChecker`.

**Confirmed — 80%.** The verdict attaches to the claim as written, which is a *latency* claim ("unbounded
recursion whose only shield is undocumented, untested checker ordering"), and both its mechanism and its
reachability analysis are verified. It is emphatically **not** an 80% claim that the recursion is
exploitable — the Trigger section says "none identified in the shipped checker chain", correctly and
positively. I want that distinction preserved verbatim in the report, because a reader who sees "Confirmed,
80%" against a recursion finding will otherwise assume a live DoS.

R8 refuting its own reachability, in writing, with citations, is the behaviour this audit should reward. It
is also why I spent the effort to re-derive the bound rather than accept it: an invariant nobody wrote down
is the one a refactor removes.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the
observable is a stack overflow, which aborts the process rather than failing an assertion, so a PoC would
be a `#[should_panic]`-shaped test that cannot actually be written portably (stack size is
platform- and thread-dependent). The finding's own value is in the *shield*, not the crash.

**Certainty unchanged at 80%. Severity unchanged at Low**, and I agree with Low: the recursion is real but
`BaseChecker` at position 3 denies a deeply-nested batch before `ExcessiveApprovalChecker` at position 6
ever decodes it, so the depth an attacker can actually deliver is bounded by an *ordering property*, not by
the function.

### Remediation check

- **Option 1 (an explicit depth cap) — sound, cheapest, and entirely local.** One caution the finding
  states and which must not be lost in review: the cap must be **at least as deep as whatever
  `check_multi_send` is ever relaxed to accept**, or the two limits silently disagree and effects go
  unenumerated inside batches `BaseChecker` allows. Encode that relationship in a comment on both sides,
  or better, derive both from one constant.
- **Option 2 (an explicit work queue with a bounded budget) — sound and strictly stronger.** It bounds
  total work rather than only depth, which matters because a *wide* batch (many sub-calls at depth 1) is
  not addressed by option 1 at all and is just as cheap for an attacker to construct. Given that
  `decode_target_effects` is about to become more load-bearing if F-ENG-035 option 1 is taken (which routes
  the blocklist through it), I would take option 2 rather than option 1.
- **Option 3 (write the shield down and pin it) — sound as a complement, dangerous as a substitute**, and
  the finding says so. Worth emphasising why: `main.rs:66-70`'s ordering comment is currently **the
  crate's only written statement that checker order is a correctness property**, and F-ENG-034 is a
  concrete case where that reasoning was applied to one checker and not to another. A comment records the
  dependency; it does not enforce it.
- **Interaction with F-ENG-035.** If option 1 there (check every address the transaction reaches) is taken
  by routing `BlocklistChecker` through `decode_target_effects`, this finding's shield becomes
  load-bearing for a checker at **position 4** rather than position 6 — still behind `BaseChecker`, so
  still sound, but with less margin. Fix F-ENG-006 in the same change.
- **Test hook: exists and is trivial** — `decode_target_effects` is a pure function of `SafeTransaction`
  and `target_effects.rs:135` already has a test module. The finding's second suggestion (an
  integration-level test over the real `main.rs` chain) has **no hook today**: `engine/mod.rs:79-90` uses
  stubs and nothing anywhere constructs the shipped checker order. This audit's
  `rust-audit/poc/F-ENG-044/` builds that chain (minus the two RPC-backed checkers) and is the closest
  thing to a starting point.
- **Where the fix belongs: the effect decoder** (`contracts/target_effects.rs`). Not the checkers, not the
  combinator, not the `RuleId` mapping.
