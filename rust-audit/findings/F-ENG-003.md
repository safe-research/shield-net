# F-ENG-003 `RuleId::R4_2DelegatecallIntegrity` restates a storage-effect rule as a target allow-list, and the allow-list admits migrations that change Safe storage the Charter does not except

| Field                | Value                                                                                     |
| -------------------- | ----------------------------------------------------------------------------------------- |
| Status               | QA-done                                                                                         |
| Crate and module     | sentinel-engine, `engine/rule.rs` (behavioural half in `checkers/base.rs`, R9's scope)      |
| Location             | `crates/sentinel-engine/src/engine/rule.rs:20-23` (related: `crates/sentinel-engine/src/checkers/base.rs:151-192`, `71-83`, `205-213`; `crates/sentinel-engine/src/contracts/bindings.rs:14-18`) |
| Severity             | Medium / Medium |
| Certainty            | 75% |
| Assumptions involved | A7, A15, A6                                                                                 |
| Tags                 | input-validation, charter-mismatch                                                          |

## Claim

Charter R-4.2 is a rule about *effects*: "A transaction is insecure if it performs a delegatecall that changes
any storage slot of the Safe", with exactly one exception — the `signedMessages` mapping. Article I restates
the same shape in the Charter's own scope statement: "block delegatecalls that modify Safe storage, except
where expressly allowed".

`RuleId::R4_2DelegatecallIntegrity`'s doc comment replaces that with a rule about *targets*: "a delegatecall
must target a known Safe migration, signing-library, `CreateCall`, or MultiSend contract, calling one of that
contract's allow-listed functions." The storage criterion — the entire content of the rule — is gone, and so
is the one exception the Charter grants.

`check_delegate_calls` implements the doc comment literally: three hardcoded address lists, each paired with a
selector prefix test. Only one of the three corresponds to anything the Charter permits. `SIGN_MESSAGE_LIBS`
+ `signMessage` is precisely the `signedMessages` exception and is correct. But `MIGRATION_CONTRACTS` +
`migrateSingleton` / `migrateWithFallbackHandler` / `migrateL2Singleton` / `migrateL2WithFallbackHandler`
is a delegatecall whose declared purpose is to change the Safe's singleton implementation address and
fallback handler — two Safe settings (`Charter:508`, `:510`), and therefore two Safe storage slots. The
Charter's exception list does not contain them, so under R-4.2 such a transaction is insecure and the engine
answers `abstain`. The crate's own `R4_1SettingsChange` doc comment concedes the point: it calls a singleton
migration a settings change (`rule.rs:16-19`), and § 2.10 lists the singleton implementation address as a
Safe setting — yet `check_settings_change` returns `None` for anything that is not a `Call`
(`base.rs:60-63`), so the delegatecall migration path is never examined under R-4.1 either. It falls between
the two rules.

The deeper problem is structural, and it is why this is worth a finding rather than a comment fix: nowhere in
the engine is there any notion of "does this delegatecall write Safe storage?". The model is
address-plus-selector throughout, so R-4.2 as the Charter states it is not merely mis-implemented, it is
inexpressible. Adding a contract to the allow-list is currently a decision no code can check against the rule
the enum claims to encode. That is the thing to fix, whatever is decided about migrations specifically.

Practical impact is contained — the allow-listed migration contracts are canonical Safe deployments whose
target singletons are fixed, so an attacker cannot steer a migration anywhere of their choosing through this
path. What is lost is the denial: a delegatecall that replaces a Safe's implementation, the single
highest-leverage operation available on a Safe, gets no vote from the reference engine where a Part A
deterministic rule calls for `insecure R-4.2`. Batched migrations inherit the same treatment, since
`check_multi_send` re-enters `check_delegate_calls` for sub-calls of a deployment that allows them
(`base.rs:210-212`).

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| - | ----- | ----- | -------- | -------------- |
| 1 | The doc comment states R-4.2 as a target-and-selector allow-list, with no storage criterion and no mention of the `signedMessages` exception. | E2 | `crates/sentinel-engine/src/engine/rule.rs:20-23` | <pre>    /// Article IV Part A, R-4.2: a delegatecall must target a known Safe<br>    /// migration, signing-library, `CreateCall`, or MultiSend contract,<br>    /// calling one of that contract's allow-listed functions.<br>    R4_2DelegatecallIntegrity,</pre> |
| 2 | The Charter's R-4.2 turns on whether the delegatecall changes a Safe storage slot, and grants exactly one exception. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:546-559` | <pre>### R-4.2 — Delegatecall integrity<br><br>#### Rule<br><br>- A transaction is insecure if it performs a delegatecall that changes any storage slot of the Safe.<br><br>#### Exceptions<br><br>- Storage slots related to onchain message signing, meaning only data stored in the signedMessages mapping type and accessible via the Solidity-generated getter method, as interpreted under § 2.2 Solidity.<br><br>#### Council applies by checking<br><br>- whether the transaction performs a delegatecall;<br>- whether the transaction modifies any storage slots of the Safe that are not included in the exceptions.</pre> |
| 3 | The Charter's own scope statement frames delegatecall integrity the same way — by storage modification, not by target. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:23` | <pre>- Delegatecall integrity: block delegatecalls that modify Safe storage, except where expressly allowed under the delegatecall integrity rules in Article IV.</pre> |
| 4 | The checker is an address-plus-selector allow-list with no storage reasoning; migration contracts are admitted alongside the sign-message libraries. | E2 | `crates/sentinel-engine/src/checkers/base.rs:151-161` | <pre>fn check_delegate_calls(tx: &SafeTransaction) -> bool {<br>    if tx.operation != Operation::DelegateCall {<br>        return false;<br>    }<br><br>    const MIGRATION_CONTRACTS: &[Address] = &[<br>        address!("6439e7ABD8Bb915A5263094784C5CF561c4172AC"),<br>        address!("526643F69b81B008F46d95CD5ced5eC0edFFDaC6"),<br>    ];<br>    if MIGRATION_CONTRACTS.contains(&tx.to) {<br>        return tx.data.starts_with(&safe::migrateSingletonCall::SELECTOR)</pre> |
| 5 | The four allow-listed migration entry points are declared, by name, to migrate the singleton and the fallback handler. | E2 | `crates/sentinel-engine/src/contracts/bindings.rs:14-18` | <pre>        function migrateSingleton;<br>        function migrateWithFallbackHandler;<br>        function migrateL2Singleton;<br>        function migrateL2WithFallbackHandler;<br>        function signMessage(bytes message);</pre> |
| 6 | The singleton implementation address and the fallback handler are Safe settings, i.e. Safe configuration state. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:253-267` | <pre>### § 2.10 Settings<br><br>#### Definition<br><br>- Settings are Safe configuration parameters.<br><br>#### Includes<br><br>- singleton implementation address;<br>- enabled modules;<br>- fallback handler;<br>- guard;<br>- module guard;<br>- owner list;<br>- signing threshold.</pre> |
| 7 | The crate itself already classifies a singleton migration as a settings change, yet the settings-change check declines to look at delegatecalls, so no rule examines it. | E2 | `crates/sentinel-engine/src/checkers/base.rs:58-69` | <pre>/// Article IV Part A settings-change guarantee. `None` if `tx` isn't a<br>/// self-call at all — not this rule's concern.<br>fn check_settings_change(tx: &SafeTransaction) -> Option<Result<, RuleId>> {<br>    if tx.operation != Operation::Call {<br>        return None;<br>    }<br>    Some(if check_calls(tx) {<br>        Ok()<br>    } else {<br>        Err(RuleId::R4_1SettingsChange)<br>    })<br>}</pre> |
| 8 | Safe's migration contracts write the singleton slot (and, for the `WithFallbackHandler` variants, the fallback-handler slot) in the Safe's own storage via delegatecall. | I | Safe's `SafeMigration`/`SafeToL2Migration` sources are **not in this checkout** (`contracts/src` carries only Safenet's own Solidity) and no registry or network is available. The claim rests on the function names in basis row 5, on the Charter's own classification of both as Safe settings (row 6), and on the fact that a migration performed by delegatecall has no other way to take effect. | *(no verbatim quote available — see the honesty note in the Trail)* |

## Trigger

`POST /v1/security-check` with `operation: 1` (DelegateCall), `to: 0x6439e7ABD8Bb915A5263094784C5CF561c4172AC`
(`base.rs:157`), `value: "0x0"`, and `data` = the 4-byte `migrateWithFallbackHandler` selector. Checker #1
abstains (not an all-zero self-call), #2 abstains (wrong selector), and #3 reaches
`check_delegatecall_integrity` -> `check_delegate_calls`, which returns `true` at `base.rs:160-169` and
therefore `Ok()` -> `Verdict::Abstain` (`base.rs:74-83`, `41-45`). No later checker inspects migration
calldata, so the engine answers `{"verdict":"abstain"}` for a delegatecall that replaces the Safe's
implementation and fallback handler.

The batched variant: `operation: 1`, `to` = `0x218543288004CD07832472D464648173c77D7eB7`
(`multi_send.rs:29`, `allows_delegate_calls: true`), `data` = `multiSend(bytes)` wrapping one packed entry
`{operation: 1, to: 0x6439e7…72AC, value: 0, data: migrateWithFallbackHandler}` — allowed by
`base.rs:210-212`, same `abstain`.

## Considered and rejected

- **"The allow-list *is* the Charter's 'expressly allowed' clause."** Rejected on the Charter's text. R-4.2's
  `#### Exceptions` block has exactly one bullet and it is the `signedMessages` mapping
  (`Charter:552-554`); there is no clause delegating the exception set to an implementation or to an operator
  list. `Charter:565-568` goes the other way, listing what passing R-4.2 does *not* buy you.
- **"`CreateCall` is the same defect."** Rejected — I checked it separately and it is defensible.
  `performCreate`/`performCreate2` (`bindings.rs:32-33`, allow-listed at `base.rs:181-190`) deploy a contract
  from the Safe's address; the account nonce is not a storage slot, so no Safe storage slot changes and R-4.2
  is satisfied without needing an exception. That the code reaches the right answer by the wrong route is the
  point of the structural half of this finding, not a separate defect.
- **"MultiSend is the same defect."** Rejected. A delegatecall into MultiSend writes no Safe storage on its
  own, and `check_multi_send` (`base.rs:205-213`) does re-examine every sub-call. The R-4.2 gap enters only
  through the sub-calls it forwards to `check_delegate_calls`, which is the same migration issue, not a new
  one.
- **"The sign-message library allowance is also unjustified."** Rejected — it is exactly right.
  `SIGN_MESSAGE_LIBS` + `signMessage` (`base.rs:171-179`, `bindings.rs:18`) maps one-to-one onto
  `Charter:554`'s `signedMessages` exception. Its correctness is evidence that R-4.2's real shape was
  understood when that branch was written, which makes the migration branch look like drift rather than a
  considered reading.
- **"This is a false positive because migrations are safe in practice."** Partly conceded, and it is why the
  Claim says so plainly and why this is Medium and not higher: the two migration addresses are canonical and
  their destination singletons are fixed, so no attacker choice enters. The finding is the missed *denial*
  and the inexpressible rule, not a fund-loss path.
- **"Basis row 8 could be wrong and sink the finding."** It could be wrong in its *mechanism* and the finding
  still stands, because rows 5 + 6 + 7 establish the mismatch from repository and Charter text alone: the
  crate's own `rule.rs:16-19` calls a singleton migration a settings change, and § 2.10 makes the singleton
  address Safe configuration state. Row 8 only supplies the slot-level detail, and I have marked it `I`
  rather than dress it up.

## Remediation options

1. **Correct the doc comment and record the divergence.** State R-4.2 as the storage-effect rule it is, name
   the `signedMessages` exception, and say explicitly which entries in `check_delegate_calls` are Charter
   exceptions (sign-message libs), which are storage-neutral and therefore pass on their merits (`CreateCall`,
   MultiSend), and which are a deliberate divergence pending a decision (migrations). This makes every future
   allow-list addition a decision someone has to justify against a stated rule.
2. **Deny migration delegatecalls under R-4.2 and R-4.1.** Remove `MIGRATION_CONTRACTS` from
   `check_delegate_calls`, and additionally have `check_settings_change` stop returning `None` for
   delegatecalls so that a singleton change is caught by the rule that actually names it. Tradeoff: Safes
   upgrading through Safenet would need `Charter:542`'s "protocol-defined settings-change path outside
   Safenet's standard transaction-security flow", which may not exist yet — a product decision, not a code
   one.
3. **Give the model a storage dimension.** Annotate each allow-list entry with the Safe storage it is known to
   write (`None`, `SignedMessages`, `Singleton`, `FallbackHandler`) and let `check_delegatecall_integrity`
   decide from that annotation rather than from membership. This is the change that makes R-4.2 expressible
   and makes options 1 and 2 mechanical rather than editorial.

Tests to add: a `base.rs` case per allow-listed migration selector pinning the intended verdict; a case for
the batched migration in the Trigger; and, under option 3, a case asserting that an entry annotated as
writing non-excepted storage is denied regardless of its address.

## Trail

- Reviewer R8: drafted, self-estimate 70%. Rows 1-7 are direct reads of this checkout and of
  `safenet-charter@44a1e53`. Row 8 is class `I` and I have said so rather than inflate it: Safe's migration
  contract source is not on this machine and I ran nothing. The self-estimate is below F-ENG-001's mainly
  because a reviewer could reasonably argue the project has already decided migrations are acceptable and
  simply never wrote that decision down — in which case this collapses to a documentation finding, which is
  option 1. Low is a defensible final severity on that reading. `checkers/base.rs` is R9's file.

## Critic (C-ENG-A)

I read R-4.2 and `check_delegate_calls` before reading the argument. **No claim in this finding is `H`.**
R8's own class marking on the slot-level row (`I`, not `E2`) is the correct call and I have not upgraded it.

### Per-claim verdicts

**Supported — the Charter half is exact.** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:548-554`:

```
#### Rule

- A transaction is insecure if it performs a delegatecall that changes any storage slot of the Safe.

#### Exceptions

- Storage slots related to onchain message signing, meaning only data stored in the signedMessages mapping type and accessible via the Solidity-generated getter method, as interpreted under § 2.2 Solidity.
```

R-4.2 is stated purely as an **effect** predicate over storage, with exactly one exception, and
`Charter:556-559` ("Council applies by checking ... whether the transaction modifies any storage slots of
the Safe that are not included in the exceptions") repeats the same shape. `rule.rs:20-23` states it as a
target-plus-selector allow-list. The two are not restatements of each other, and the doc comment carries no
"MVP note" caveat where its siblings do (`rule.rs:24-27`, `:58-68`).

**Supported.** `base.rs:151-192` is three hardcoded address lists each paired with selector prefix tests,
exactly as described. `SIGN_MESSAGE_LIBS` + `signMessage` (`:171-179`) maps one-to-one onto the
`signedMessages` exception — R8 is right that this branch is correct, and right that its correctness makes
the migration branch look like drift rather than a considered reading.

**Supported, with the reviewer's own caveat retained.** That `migrateSingleton` /
`migrateWithFallbackHandler` write Safe storage is class `I` here: Safe's contracts are not in this
checkout (`contracts/src` holds Safenet's own sources only), so the slot-level fact cannot be `E2` this
run. What *is* `E2` is that § 2.10 (`Charter:259-267`) lists "singleton implementation address" and
"fallback handler" as Safe **Settings**, i.e. Safe configuration state, and that `rule.rs:16-19` — the
crate's own text — calls a singleton migration a settings change. The finding does not depend on the
slot-level row, and R8 says so in "Considered and rejected". I agree.

**Supported, and this is the sharpest part of the finding.** The migration path falls between the two
rules: `check_settings_change` returns `None` for anything that is not a `Call` (`base.rs:60-63`), so a
delegatecall migration is never examined under R-4.1; and `check_delegate_calls` allows it under R-4.2. I
re-derived `check_transaction`'s dispatch (`base.rs:52-56`) to confirm there is no third path.

### On the structural claim

R8's central point — "nowhere in the engine is there any notion of *does this delegatecall write Safe
storage?*, so R-4.2 as the Charter states it is inexpressible" — is correct and, in my view, the most
valuable sentence in this finding. It is what makes the allow-list unmaintainable: adding an address is a
decision no code and no test can check against the rule the enum claims to encode. It also generalises to
the *narrow* direction, which R8 recorded as an observation but did not file — see **F-ENG-010**, which I
have drafted from that observation.

### Severity and certainty

**Medium / Medium.** The reviewer's own concession is right: the two migration addresses are canonical and
their destination singletons are fixed, so no attacker choice enters through this path. The harm is the
missed R-4.2 denial on the highest-leverage operation a Safe has, plus the inexpressibility. Same
`abstain ≠ secure` reasoning as F-ENG-001 (`crates/sentinel/src/service.rs:176-179`), so not Critical.

**Confirmed — 75%.** Ten points below F-ENG-001 for one reason only: the Charter/doc mismatch is fully
`E2` and verified, but the *behavioural* impact statement ("a delegatecall that replaces a Safe's
implementation gets no vote") rests on a class `I` premise about where Safe stores its singleton. The
premise is close to definitional and I did not find any reading that rescues the code, but it is not `E2` in
this checkout and I will not certify it as such.

### Relationship to F-ENG-039 (R9) and F-ENG-040 (R9)

**F-ENG-003 is the R-4.2 half of the same defect F-ENG-039 documents from the implementation side** — see
the same discussion in `F-ENG-001`'s Critic section. **Canonical: F-ENG-039** for the behaviour and its
severity; F-ENG-003 is retained as the rule-vocabulary and structural half and must not be counted twice in
the report's totals. **F-ENG-040** (MultiSend denials all reported as R-4.2) is a distinct defect that shares
this file's `base.rs:198-204` TODO as its subject; no overlap in impact.

## Cross-reference (Critic C-ENG-B, covering R9)

Not a critique of this file — C-ENG-A owns that. This is the overlap note §6 of the Critic brief requires,
recorded in both places so the two do not contradict each other.

**F-ENG-039 (R9) makes the same behavioural claim as this file**, from the checker side: `check_delegate_calls`
(`base.rs:151-193`) admits the four `migrate*` entry points on two migration contracts, which write the
singleton and fallback-handler slots — neither of which is the `signedMessages` mapping that
`safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:552-554` names as R-4.2's only exception. I re-read
R-4.2 in full and both files are accurate on it, including the point that `SIGN_MESSAGE_LIBS` + `signMessage` is
correct and is the one allow-list entry that does match the Charter.

**Recommended canonical assignment:**

- **F-ENG-039 canonical for the behavioural defect** — it states R-4.1 and R-4.2 together against the single
  file that must change (`checkers/base.rs`), so the fix is not split across two rule codes.
- **This file canonical for the `engine/rule.rs:20-23` doc-comment defect** — that
  `RuleId::R4_2DelegatecallIntegrity` restates an *effects* rule ("changes any storage slot of the Safe") as a
  *target allow-list*. That reframing is the reason the implementation drifted, and it will cause the drift
  again if only `base.rs` is fixed, so it is worth keeping as its own item.

Nothing merged, nothing deleted. See the matching note on F-ENG-001; the report should count one behavioural
finding plus two documentation findings, not three.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**, for
the same reasons as F-ENG-001: the behavioural half is F-ENG-039's canonically, and the doc half is prose
against an external document.

**Certainty unchanged. Severity unchanged.**

### Remediation check — doc fix and behaviour fix, separated

- **Doc half — `rule.rs:20-23`'s description of R-4.2. Sound, zero risk, ship immediately.** Option 1 is
  right, and its most valuable element is the one that is easiest to drop under time pressure: **classify
  each allow-list entry** — Charter exception (sign-message libraries), storage-neutral and therefore
  passing on its merits (`CreateCall`, MultiSend), or deliberate divergence pending a decision
  (migrations). Without that classification the corrected comment still leaves a reader unable to tell
  which entries are justified. Canonical here alongside F-ENG-001 and F-ENG-004.
- **Behaviour half — option 2 (deny migration delegatecalls). Sound against the Charter, and it carries the
  same product dependency as F-ENG-001 option 2**: Safes upgrading through Safenet would need
  `Charter:542`'s settings-change path, whose existence is not established in this checkout. Same
  sequencing conclusion, same bond-exposure reason for caring (see F-ENG-042's `## QA`). Do not ship
  blind.
- **Option 3 (annotate each allow-list entry with the Safe storage it writes) — sound, and the best of the
  three.** It is the only option that makes R-4.2 *expressible* in the code rather than approximated: the
  Charter's rule is about storage effects, and the current model is about target membership, so options 1
  and 2 are editorial patches on a model that cannot state the rule. Option 3 makes them mechanical. Cost:
  someone must determine the storage effect of every entry, which is real research per address — but it is
  research that has to happen anyway before option 2 can be defended, so it is not additional work, only
  earlier work.
- **Test hook: exists** (`BaseChecker` is pure, `base.rs:230-764`). Under option 3, add the case the
  finding names — an entry annotated as writing non-excepted storage is denied *regardless of its address*
  — because that is the assertion that stops the allow-list drifting back into a membership test.
- **Where the fix belongs: the Charter-to-`RuleId` mapping (doc), the checker (behaviour), and — under
  option 3 — the model in between.** Not the combinator.
