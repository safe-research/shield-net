# F-ENG-040 Every MultiSend denial is reported as R-4.2, even when the failing sub-call is a settings-change violation

| Field                | Value                                                                          |
| -------------------- | ------------------------------------------------------------------------------ |
| Status               | QA-done                                                                             |
| Crate and module     | sentinel-engine, checkers/base.rs                                               |
| Location             | crates/sentinel-engine/src/checkers/base.rs:195-213 (related: base.rs:52-56, :74-83) |
| Severity             | Low / Informational                                                                   |
| Certainty            | 85% (Critic C-ENG-B; E2 ceiling, read-only run)                                          |
| Assumptions involved | A3, A12, A15                                                                    |
| Tags                 | known, verdict-policy, charter                                                  |

## Claim

`check_multi_send` returns a bare `bool`, so `check_delegatecall_integrity` maps any failure to
`RuleId::R4_2DelegatecallIntegrity`. A batch denied because one sub-call is a disallowed **self-call** — an
R-4.1 settings-change violation — is reported as an R-4.2 delegatecall-integrity violation. The code carries a
TODO saying exactly this (`base.rs:198-204`), and it is item `checkers/base.rs:198` in the codebase map's known
list, so it is filed tagged `known` at reduced priority.

Consequences: the sentinel's vote reason string is the rule code (`crates/sentinel/src/service.rs:175`,
`rule.to_string`), and that string is cryptographically bound into the commitment and emitted on-chain. Charter
§2.14 says the protocol does not validate whether the reason correctly applies the Charter and that the reason
does not affect vote counting, so there is no direct protocol consequence — but §3.7 asks the Council to
"identify the applicable Charter rule", and precedent under Article V is organised by rule. A systematically
mis-attributed rule code degrades the evidence trail for every batched denial and will mislead any later
analysis that groups denials by rule.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| - | ----- | ----------------- | -------- | -------------- |
| 1 | The TODO records the defect, and `check_multi_send` returns `bool` | E2 | crates/sentinel-engine/src/checkers/base.rs:195-213 | Q1 |
| 2 | The caller turns that `bool` into `R4_2DelegatecallIntegrity` unconditionally | E2 | crates/sentinel-engine/src/checkers/base.rs:74-83 | Q2 |
| 3 | The rule code becomes the sentinel's on-chain vote reason | E2 | crates/sentinel/src/service.rs:173-176 | Q3 |
| 4 | The Charter treats the reason as unvalidated but expects the applicable rule to be identified | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:320-326, :443 | Q4 |

**Q1** `crates/sentinel-engine/src/checkers/base.rs:198-204` and `:205-213`

```rust
/// TODO: a denial here always maps to `RuleId::R4_2DelegatecallIntegrity` at
/// the `check_transaction` level, even when the actual failing sub-tx is a
/// settings-change violation (`RuleId::R4_1SettingsChange`) rather than a
/// delegatecall-integrity one. Correctly attributing the rule would mean
/// this function returning the failing sub-tx's own rule instead of a flat
/// `bool`, which is a bigger change than a MultiSend-only fix warrants
/// right now.
```

```rust
fn check_multi_send(tx: &SafeTransaction) -> bool {
    let Some((sub_txs, allows_delegate_calls)) = decode_multi_send_call(tx) else {
        return false;
    };

    sub_txs.iter.all(|sub_tx| {
        check_calls(sub_tx) || (allows_delegate_calls && check_delegate_calls(sub_tx))
    })
}
```

**Q2** `crates/sentinel-engine/src/checkers/base.rs:74-83`

```rust
fn check_delegatecall_integrity(tx: &SafeTransaction) -> Option<Result<, RuleId>> {
    if tx.operation != Operation::DelegateCall {
        return None;
    }
    Some(if check_delegate_calls(tx) || check_multi_send(tx) {
        Ok()
    } else {
        Err(RuleId::R4_2DelegatecallIntegrity)
    })
}
```

**Q3** `crates/sentinel/src/service.rs:173-176`

```rust
        let (approve, reason) = match outcome {
            CheckOutcome::Approved => (true, String::new()),
            CheckOutcome::Denied(rule) => (false, rule.to_string),
```

**Q4** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:320-326` and `:443`

```text
- Every revealed Sentinel vote carries a `reason` string. The exact string is cryptographically bound to the Sentinel's prior commitment and emitted onchain.
- The protocol accepts an empty or arbitrary string and does not validate whether the reason correctly applies this Charter. The reason does not affect vote counting or create another verdict.
```

```text
- Where the protocol-supported ruling format permits, the Council should identify the applicable Charter rule and version in the ruling explanation or associated metadata.
```

## Trigger

A MultiSend delegatecall to a canonical deployment (e.g. `0x40A2aCCbd92BCA938b02010E17A5b8929b49130D`) with one
packed `Call` sub-transaction whose `to` equals the Safe and whose calldata is a non-allow-listed self-call
selector, e.g. `0xdeadbeef`:

```text
operation = 0x00
to        = <the Safe's own address>
value     = 0x00…00
dataLength= 0x00…04
data      = deadbeef
```

wrapped as `multiSend(bytes)` and posted with `operation: 1`, `to: 0x40A2aCC…130D`. Response:
`{"verdict":"insecure","rule":"R-4.2"}`. The failing sub-call is a settings-change violation, so the Charter's
applicable rule is R-4.1 (`base.rs:277-295` returns exactly that for the same calldata unbatched).

Corpus shape: this vector plus the unbatched control, asserting the two currently disagree on the rule code.

## Considered and rejected

- **The verdict itself is wrong.** It is not — the transaction is correctly denied; only the cited rule is wrong.
- **The mis-attribution is protocol-relevant.** Charter §2.14 (Q4) explicitly says the reason is not validated
  and does not affect vote counting, so there is no slashing or counting consequence. This is why the severity is
  Low despite the claim being certain.
- **This is not in the known list.** It is: `crates/sentinel-engine/src/checkers/base.rs:198` appears in
  `rust-audit/codebase-map.md` §4, so A12 applies and it is filed at reduced priority rather than omitted.

## Remediation options

1. Change `check_multi_send` to return `Result<, RuleId>` (or `Option<RuleId>`) by threading each sub-call
   through `check_transaction` instead of the `check_calls || check_delegate_calls` pair, and propagate the first
   failing sub-call's rule. This is the fix the TODO describes.
2. If the wider refactor is unwanted, special-case the common shape: when every sub-call failure is a self-call
   failure, report R-4.1.

Tests to add: the batched-vs-unbatched pair above, asserting the same rule code from both. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 95% (fact) / Low severity. Known item per A12.

## Critic (C-ENG-B)

### 1. Per-claim verdicts — all Supported

`check_multi_send` returns a bare `bool` (`base.rs:205-213`) and its only caller,
`check_delegatecall_integrity` (`base.rs:74-83`), maps any `false` to
`Err(RuleId::R4_2DelegatecallIntegrity)` unconditionally. The TODO at `base.rs:198-204` states the defect in the
authors' own words, so the `known` tag under A12 is correct and correctly attributed to the codebase-map entry
`checkers/base.rs:198`. `crates/sentinel/src/service.rs:173-176` does turn the rule into the vote reason
(`CheckOutcome::Denied(rule) => (false, rule.to_string)`). Charter `:443` — "the Council should identify the
applicable Charter rule and version in the ruling explanation or associated metadata" — is quoted accurately.

### 2. Severity — corrected from Low to Informational

R9 rates this Low. Under PROMPT.md §8 the Informational row is explicitly "hardening, documentation, test gaps,
and `known` items", and this is a `known` item whose entire impact is on the *label* attached to a denial that
still happens: the transaction is denied either way, the vote is a denial either way, and R9 correctly cites
Charter §2.14 for the fact that the reason string is not validated and does not affect vote counting. Nothing
about the security outcome changes. **Informational** with the `known` tag is the honest placement, and A12's
"reduced priority" instruction points the same way. This is not a criticism of the finding — it is well
evidenced and worth recording; it is a severity correction only.

### 3. Finding verdict

**Confirmed.** Certainty **85%** (mechanism and consequence both directly code-traced; the only inferential step
is the Charter's expectation about rule attribution, which is `I` and correctly labelled). Severity
**Informational** (corrected from Low).

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the
finding is a mis-attributed rule *code* on a transaction that is correctly denied either way, so it changes
no verdict and no vote count — the cheapest demonstration is the batched/unbatched pair the finding already
specifies, which belongs in `base.rs`'s own suite next to the fix.

**Certainty unchanged at 85%. Severity unchanged (Low reviewer / Informational final).**

**One Charter point that supports the Informational rating and is worth citing in the report.** §2.14:
"Every revealed Sentinel vote carries a `reason` string… The protocol accepts an empty or arbitrary string
and does not validate whether the reason correctly applies this Charter. **The reason does not affect vote
counting or create another verdict.**" So a wrong rule code is not a protocol-level defect. It is still a
real defect at the arbitration layer, where §5.2's reasoned-ruling requirements and §5.1's precedent
mechanism both operate on *which rule* was cited — a Council reading a batched settings-change denial
labelled R-4.2 is being told the wrong thing about why the transaction failed.

### Remediation check

- **Option 1 (thread each sub-call through `check_transaction` and propagate the first failing sub-call's
  rule) — sound, and it is the fix the code's own TODO describes.** Returning `Result<, RuleId>` from
  `check_multi_send` rather than the `check_calls || check_delegate_calls` boolean pair also removes the
  place where the two predicates are combined with `||`, which is a small structural improvement in its own
  right. Take this.
- **Option 2 (special-case the common shape: report R-4.1 when every sub-call failure is a self-call
  failure) — unsound as a substitute, and I would not ship it.** It replaces one wrong answer with a
  narrower wrong answer: a batch mixing a self-call violation and a delegatecall violation still reports
  whichever the special case picks, and the next reader has to reconstruct the heuristic from the code. It
  also entrenches the boolean pair that option 1 removes. If option 1 is genuinely unaffordable, prefer
  emitting a distinct log line naming the failing sub-call and its rule while leaving the emitted code
  alone — that gets the diagnostic value without putting a guess on the wire.
- **Interaction with F-ENG-039.** If option 1 there (align `check_multi_send` with the Charter by refusing
  self-call sub-transactions outright) is taken, the batched settings-change case becomes a denial for a
  *different* reason, and the rule code question changes shape. Fix F-ENG-040 first — it is small and
  independent — so that whatever F-ENG-039 decides, the code reported is the code that failed.
- **Test hook: exists.** `BaseChecker` is pure; `base.rs:216-764`. The assertion the finding names — the
  same rule code from the batched and unbatched forms of one violation — is the right regression test and
  is worth keeping permanently, because it is the property that broke.
- **Where the fix belongs: the checker.** The `RuleId` mapping itself is fine; this is about which existing
  variant is selected, not about what the variants mean.
