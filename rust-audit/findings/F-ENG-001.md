# F-ENG-001 `RuleId::R4_1SettingsChange`'s stated meaning is far wider than Charter R-4.1's allowed exception, and the base checker implements the doc comment rather than the Charter

| Field | Value |
| --- | --- |
| Status | QA-done |
| Crate and module | sentinel-engine, `engine/rule.rs` (behavioural half in `checkers/base.rs`, R9's scope) |
| Location | `crates/sentinel-engine/src/engine/rule.rs:16-19` (related: `crates/sentinel-engine/src/checkers/base.rs:103-147`, `13-30`, `60-69`, `86-99`, `205-213`) |
| Severity | Medium / Medium |
| Certainty | 85% |
| Assumptions involved | A2, A7, A15 |
| Tags | input-validation, charter-mismatch |

## Claim

`RuleId::R4_1SettingsChange`'s doc comment states that R-4.1 permits "owner/threshold/guard/module/fallback handler changes, or a known singleton migration". Charter R-4.1 permits nothing of the sort. Its allowed exception is a conjunction of seven conditions culminating in a two-function list: **`disableModule` with any valid parameter**, and **`setFallbackHandler` with the zero address**. Everything else that modifies a Safe setting — including every owner change, every threshold change, every `setGuard`, every `enableModule`, every `setFallbackHandler` to a non-zero handler, and every singleton migration — is insecure under R-4.1 with no exception available.

This is not a stray comment. `checkers/base.rs` implements exactly the set the doc comment describes, so the divergence is the engine's actual behaviour:

- Five settings functions are allowed on a bare selector prefix with no argument inspection at all (`addOwnerWithThreshold`, `removeOwner`, `swapOwner`, `changeThreshold`, `disableModule`, `base.rs:104-122`). Four of those five are outside the Charter's exception entirely.
- `setFallbackHandler` is allowed for any of **seven** handlers (`base.rs:13-21`), where the Charter allows the zero address only.
- `setGuard(0)`, `enableModule` to either of two modules, and `setModuleGuard(0)` are allowed (`base.rs:125-144`, `23-30`); none appears in the Charter's exception list.
- The Charter's `value == 0` condition is absent: `check_self_calls` (`base.rs:103-147`) never reads `tx.value`, so a `disableModule` self-call carrying arbitrary native value — a transaction the Charter's exception explicitly does _not_ cover — passes.
- The Charter's "non-batched" condition is absent: `check_multi_send` (`base.rs:205-213`) evaluates each packed sub-call through the very same `check_calls` -> `check_self_calls` path, so a settings change wrapped in a MultiSend batch is treated as if the exception applied to it. Under the Charter no batched settings change can ever qualify, not even `disableModule`.

The outcome is a systematically missed denial, not a false affirmation. `BaseChecker` returns `Verdict::Abstain` when its check passes (`base.rs:41-46`), and no later checker in the shipped chain affirms or denies a self-call carrying settings calldata, so the engine answers `abstain` and the sentinel casts **no vote** on a transaction the Charter deems insecure. Under A2 the transaction contents are attacker-controlled, so an adversary holding enough Safe owner signatures to propose an owner-list takeover, a guard change, or a singleton migration gets silence from the reference engine where Charter R-4.1 and § 2.14 call for a denying vote citing `R-4.1`. The engine is the reference implementation operators are told to be compatible with, so the gap propagates to every sentinel that runs it.

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The doc comment states R-4.1's allowance as a broad family of settings-management functions plus singleton migration. | E2 | `crates/sentinel-engine/src/engine/rule.rs:16-19` | <pre> /// Article IV Part A, R-4.1: a self-call must target an allow-listed Safe<br> /// settings-management function (owner/threshold/guard/module/fallback<br> /// handler changes, or a known singleton migration).<br> R4_1SettingsChange,</pre> |
| 2 | The Charter's R-4.1 exception is a seven-part conjunction whose function list has exactly two members, and it requires `value == 0` and a non-batched transaction. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:516-528` | <pre>#### Allowed exceptions<br><br>A settings change is not insecure under this rule only if all of the following apply:<br><br>- transaction is a non-batched multisignature Safe transaction;<br>- the transaction is triggered via `execTransaction`;<br>- `to` is the Safe itself;<br>- `operation` is `0` for CALL;<br>- `value` is `0`;<br>- `data` is a valid setting-change function following canonical Solidity ABI encoding, as interpreted under § 2.2 Solidity;<br>- the setting-change function is one of:<br> - `disableModule` with any valid parameter;<br> - `setFallbackHandler` with the zero address as the `handler` parameter.</pre> |
| 3 | The owner list, signing threshold, guard, module guard, enabled modules and singleton address are all Safe _settings_, so changing any of them engages R-4.1. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:504-514` | <pre>- A transaction is insecure if it modifies any Safe setting (§ 2.10), unless it falls within an allowed exception below.<br><br>#### Applies to<br><br>- singleton implementation address;<br>- enabled modules;<br>- fallback handler;<br>- guard;<br>- module guard;<br>- owner list;<br>- signing threshold.</pre> |
| 4 | Four settings functions outside the Charter's exception are allowed on a bare selector prefix, with no argument decoding and no `value` check (a fifth, `disableModule`, follows at `base.rs:120-122`). | E2 | `crates/sentinel-engine/src/checkers/base.rs:105-119` | <pre> if tx<br> .data<br> .starts_with(&safe::addOwnerWithThresholdCall::SELECTOR)<br> {<br> return true;<br> }<br> if tx.data.starts_with(&safe::removeOwnerCall::SELECTOR) {<br> return true;<br> }<br> if tx.data.starts_with(&safe::swapOwnerCall::SELECTOR) {<br> return true;<br> }<br> if tx.data.starts_with(&safe::changeThresholdCall::SELECTOR) {<br> return true;<br> }</pre> |
| 5 | `setGuard`, `enableModule` and `setModuleGuard` — none of which the Charter exempts — are allowed on an address allow-list, and `setFallbackHandler` is allowed for seven handlers rather than only the zero address. | E2 | `crates/sentinel-engine/src/checkers/base.rs:125-139` | <pre> if tx.data.starts_with(&safe::setFallbackHandlerCall::SELECTOR) {<br> return safe::setFallbackHandlerCall::abi_decode(&tx.data)<br> .ok<br> .is_some_and(\|call\| SUPPORTED_FALLBACK_HANDLERS.contains(&call.handler));<br> }<br> if tx.data.starts_with(&safe::setGuardCall::SELECTOR) {<br> return safe::setGuardCall::abi_decode(&tx.data)<br> .ok<br> .is_some_and(\|call\| SUPPORTED_GUARDS.contains(&call.guard));<br> }<br> if tx.data.starts_with(&safe::enableModuleCall::SELECTOR) {<br> return safe::enableModuleCall::abi_decode(&tx.data)<br> .ok<br> .is_some_and(\|call\| SUPPORTED_MODULES.contains(&call.module));<br> }</pre> |
| 6 | The fallback-handler allow-list has seven entries, six of them non-zero. | E2 | `crates/sentinel-engine/src/checkers/base.rs:13-21` | <pre>const SUPPORTED_FALLBACK_HANDLERS: &[Address] = &[<br> Address::ZERO,<br> address!("85a8ca358D388530ad0fB95D0cb89Dd44Fc242c3"),<br> address!("2f55e8b20D0B9FEFA187AA7d00B6Cbe563605bF5"),<br> address!("3EfCBb83A4A7AfcB4F68D501E2c2203a38be77f4"),<br> address!("fd0732Dc9E303f09fCEf3a7388Ad10A83459Ec99"),<br> address!("f48f2B2d2a534e402487b3ee7C18c33Aec0Fe5e4"),<br> address!("017062a1dE2FE6b99BE3d9d37841FeD19F573804"),<br>];</pre> |
| 7 | A packed MultiSend sub-call is evaluated through the same self-call path, so batched settings changes are treated as exempt even though the Charter's exception requires the transaction to be non-batched. | E2 | `crates/sentinel-engine/src/checkers/base.rs:205-213` | <pre>fn check_multi_send(tx: &SafeTransaction) -> bool {<br> let Some((sub_txs, allows_delegate_calls)) = decode_multi_send_call(tx) else {<br> return false;<br> };<br><br> sub_txs.iter.all(\|sub_tx\| {<br> check_calls(sub_tx) \|\| (allows_delegate_calls && check_delegate_calls(sub_tx))<br> })<br>}</pre> |
| 8 | A passing base check yields `Abstain`, i.e. no vote, rather than a denial — so the gap manifests as silence. | E2 | `crates/sentinel-engine/src/checkers/base.rs:41-46` | <pre> async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {<br> match check_transaction(transaction) {<br> Ok() => Verdict::Abstain,<br> Err(rule) => Verdict::Insecure { rule },<br> }<br> }</pre> |

## Trigger

Any of the following bodies to `POST /v1/security-check`, each of which the Charter makes insecure under R-4.1 and each of which the engine answers `{"verdict":"abstain"}`:

1. **Owner-list takeover.** `to == safe`, `operation: 0`, `value: "0x0"`, `data` = `addOwnerWithThreshold(<attacker>, 1)` ABI-encoded. `check_self_calls` returns at `base.rs:107-110` without decoding the arguments; `addOwnerWithThreshold` is not in the Charter's two-function exception list, and the owner list is a setting (`Charter:513`).
2. **Non-zero value on an otherwise-exempt function.** `to == safe`, `operation: 0`, `value: "0x3635c9adc5dea00000"`, `data` = `disableModule(prev, module)`. The Charter's exception requires `value` to be `0` (`Charter:524`); `check_self_calls` never reads `tx.value` (`base.rs:103-147`).
3. **Batched settings change.** `operation: 1`, `to` = a canonical MultiSend deployment (e.g. `0xA1dabEF33b3B82c7814B6D82A79e50F4AC44102B`, `multi_send.rs:64`), `data` = `multiSend(bytes)` wrapping one packed entry `{operation: 0, to: safe, value: 0, data: setFallbackHandler(<any of the six non-zero allow-listed handlers>)}`. The Charter's exception requires the transaction to be _non-batched_ (`Charter:520`) **and** the handler to be the zero address (`Charter:528`); the engine allows both.

In all three the earlier checkers abstain (Cancellation requires all-zero fields; EscapeHatch requires the `announceTransaction`/`cancelAnnouncement` selector) and the later ones have nothing to say about settings calldata, so the chain (`main.rs:57-73`, `engine/mod.rs:62-69`) ends in `Abstain`.

## Considered and rejected

- **"The doc comment is describing the implementation's MVP scope, not claiming to restate the Charter."** Rejected on the module's own terms. `rule.rs:1-8` says the enum is "Shared vocabulary between check logic and the Safenet Arbitration Charter... a variant is added in the same change that implements the check giving it meaning", and `AGENTS.md:111` requires each variant to be "doc-commented with the Charter article/rule it corresponds to". Every other variant that narrows scope says so explicitly with an "MVP note" (`rule.rs:24-27`, `58-68`) or an out-of-scope caveat (`rule.rs:31-33`). `R4_1SettingsChange` carries no such caveat; it reads as a restatement of the rule, and it is wrong in the _widening_ direction, which no MVP-scoping note would justify.
- **"This is a false positive because the engine returns `Insecure` for these cases anyway."** Rejected: `base.rs:41-45` maps a passing check to `Abstain`, and `check_settings_change` (`base.rs:60-69`) returns `Some(Ok())` — not an error — as soon as `check_calls` is true.
- **"A later checker denies it."** Rejected by walking the shipped chain at `main.rs:57-73`. `BlocklistChecker` only tests `to` against the operator list and `to == safe` here; `NestedSafeChecker` requires `tx.to != tx.safe` and `execTransaction` calldata; `ExcessiveApprovalChecker` decodes only transfer/approve effects (`target_effects.rs:71-129`); `CowChecker` and `StakingChecker` gate on their own contract addresses; `RefundChecker` inspects only the refund leg; `AddressPoisoningChecker` requires an ERC-20 transfer/approve target. None of them looks at settings calldata.
- **"The engine would at least deny the _un-allow-listed_ settings functions, so only the allow-listed ones are affected."** True and already accounted for: this finding is about the allow-list being wider than the Charter's, plus the two missing conditions (`value == 0`, non-batched) that apply even to the two functions the Charter does exempt.
- **"There is a false-`secure` path here."** Rejected — I looked for one and there is none. `BaseChecker` never returns `Secure` (`base.rs:41-45`), and neither `CancellationChecker` nor `EscapeHatchChecker`, the only affirmers that run before it, accepts settings calldata. So this is a missed denial, not a Critical false affirmation, which is why the severity is Medium rather than Critical.
- **"A different Charter version might have a wider exception."** The Charter available this run is upstream commit `44a1e53`, which the Manager's brief §3 confirms defines exactly the R-4.1…R-4.6 set `rule.rs` cites, and § 2.10 (`Charter:253-267`) enumerates the same seven settings. I did not find any other Charter text in or referenced by the repository (`grep -rn "Charter" docs/ AGENTS.md crates/sentinel-engine/`).

## Remediation options

1. **Rewrite the doc comment to state R-4.1 accurately and record the divergence explicitly.** Cheapest and strictly an improvement: say that the Charter's exception is `disableModule` / `setFallbackHandler(0)`, non-batched, `value == 0`, and that the engine deliberately abstains on (rather than denies) a wider set pending a policy decision. This does not fix behaviour but stops the code from asserting a false reading of the Charter to every future reader.
2. **Bring `check_self_calls` to the Charter's exception and deny the rest.** Restrict the exempt set to `disableModule` and `setFallbackHandler(0)`, add `tx.value.is_zero` as a precondition, ABI-decode rather than `starts_with` (the Charter requires "canonical Solidity ABI encoding"), and have `check_multi_send` refuse a self-call sub-transaction outright so batched settings changes are never exempt. Tradeoff: this is a large denial-surface increase; every Safe that changes owners or threshold through Safenet starts getting `insecure R-4.1`. That is what the Charter says, but it should be a deliberate product decision, and `Charter:542` ("Other legitimate Safe settings changes must use the protocol-defined settings-change path outside Safenet's standard transaction-security flow") is the sentence that decides it.
3. **Middle path: keep the wider allow-list but make the two missing _conditions_ mandatory** — reject a non-zero `value` on any exempt self-call and reject any settings self-call reached through `check_multi_send`. These two are pure hardening with no legitimate traffic behind them and no Charter ambiguity.

Tests to add (none are committed here): a `base.rs` case per Trigger item asserting the intended verdict; a `check_multi_send` case wrapping a self-call `setFallbackHandler`; and a `value != 0` case on `disableModule`. `base.rs`'s existing suite (`base.rs:230-764`) has allow-list cases but none that pins the Charter's `value == 0` or non-batched conditions.

## Trail

- Reviewer R8: drafted, self-estimate 80%. The code and Charter citations are direct reads of this checkout and of `safenet-charter@44a1e53`; the residual 20% is the judgement call over whether the project intends `rule.rs`'s doc comments to be normative restatements of the Charter (I argue yes, from `rule.rs:1-8` and `AGENTS.md:111`) and whether the Council would in practice read R-4.1's exception list as exhaustive (the Charter says "only if all of the following apply", which reads as exhaustive to me). The behavioural half lives in `checkers/base.rs`, which is R9's scope — R9 and `C-ENG` should reconcile.

## Critic (C-ENG-A)

I re-derived R-4.1 from the Charter and the behaviour from `base.rs` before reading R8's argument, and reached the same conclusion by an independent route. **No claim in this finding is `H`.** Every one of the eight `Basis` rows was re-opened at its stated `path:line-range` in this checkout and contains the quoted text.

### Per-claim verdicts

| # | Verdict | Note from re-opening the citation |
| --- | --- | --- |
| 1 | **Supported** | `rule.rs:16-19` reads exactly as quoted. |
| 2 | **Supported** | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:516-528` verbatim. The conjunction really is closed — "A settings change is not insecure under this rule **only if all of the following apply**" — and the function list really has two members. `value` is `0` is line 524; "non-batched multisignature Safe transaction" is line 520. |
| 3 | **Supported** | Charter:504-514. Note that this is the rule's **"Applies to"** list, i.e. the set of settings that _engage_ R-4.1. `rule.rs:16-19` has inverted it into an allow-list — that inversion is the cleanest way to state the defect and I recommend it be added to the Claim. § 2.10 (Charter:259-267) repeats the same seven items as the definition of "Settings". |
| 4 | **Supported** | `base.rs:105-119`; `disableModule` follows at `:120-122` as stated. |
| 5 | **Supported** | `base.rs:125-139`; `setModuleGuard` follows at `:140-144`. |
| 6 | **Supported** | `base.rs:13-21`, seven entries, six non-zero. |
| 7 | **Supported** | `base.rs:205-213`. |
| 8 | **Supported** | `base.rs:41-46`. |

### Additional evidence the reviewer did not cite

**The divergence is pinned by a test as intended behaviour.** `crates/sentinel-engine/src/checkers/base.rs:349-364`:

```
    #[test]
    fn allows_self_call_with_nonzero_value {
        assert!(
            check_transaction(&tx(
                address!("F01888f0677547Ec07cd16c8680e699c96588E6B"),
                address!("F01888f0677547Ec07cd16c8680e699c96588E6B"),
                U256::from(1u64),
                hex("0xe318b52b\
```

`0xe318b52b` is `swapOwner`. So the suite asserts that a self-call changing the owner list **with `value == 1`** is `Ok`. Under Charter:520-528 that transaction fails the exception on three independent grounds (`swapOwner` is not one of the two exempt functions; `value` is not `0`). A test that codifies the divergence is stronger evidence than the absence of one, and it also tells the team that fixing this is a deliberate behaviour change, not a bug fix — remediation option 3 in this file cannot be applied without deleting or inverting `allows_self_call_with_nonzero_value`.

### Severity

**Medium / Medium — the reviewer's severity is correct, and I confirmed the step it rests on.** The question is whether `abstain` becomes a `secure` vote. It does not: `crates/sentinel/src/service.rs:173-179` —

```
        let (approve, reason) = match outcome {
            CheckOutcome::Approved => (true, String::new()),
            CheckOutcome::Denied(rule) => (false, rule.to_string),
            CheckOutcome::Unknown => {
                tracing::warn!(%request_id, "engine check failed; dropping request unanswered");
                return (state, Vec::new());
            }
        };
```

and `crates/sentinel/src/engine.rs:175` maps `Response::Abstain` to `CheckOutcome::Unknown`. The sentinel returns before `commit_vote`, so no commitment is made and no vote is cast. This is a **missed denial**, not "a malicious Safe transaction rated `secure`", so PROMPT § 8's Critical band does not apply. It is above Low because Article I (Charter:22) names settings-change blocking as one of the two things Safenet's transaction security _is_, and the reference engine does not do it for the whole owner/threshold/guard/module family.

### Finding verdict and certainty

**Confirmed — 85%.** Mechanism and trigger are both verified: each of the three Trigger bodies is a single JSON request whose path through `main.rs:57-73` I walked independently, and I confirmed R8's "no later checker looks at settings calldata" by re-reading `blocklist.rs:24-32` (tests `to` against the operator set only) and `nested.rs:42-47` (requires `tx.to != tx.safe`, so a self-call cannot reach it). The 15% held back is the same interpretive residue R8 names — whether the team intends `rule.rs`'s doc comments as normative restatements — not any doubt about the code or the Charter text. 89% is the ceiling this read-only run allows regardless (`state/baseline.md` § 2).

### Relationship to F-ENG-039 (R9)

**Yes: `F-ENG-001` and `F-ENG-039` are the same defect seen from two sides, and `F-ENG-003` is the R-4.2 half of the same pair.** F-ENG-039 documents the behaviour (`checkers/base.rs`' allow-lists exceed the Charter's two exceptions); F-ENG-001 documents the rule vocabulary (`engine/rule.rs`'s doc comment asserts that wider set _is_ R-4.1) and F-ENG-003 the R-4.2 equivalent. They are not redundant — a doc comment that misstates the Charter is a distinct artifact that survives any behavioural fix, and it is what a future maintainer adding an address to the allow-list will read. But they share one severity-carrying claim ("settings changes are never denied"), which must be counted **once**.

**Canonical: `F-ENG-039`** for the behaviour and its severity. `F-ENG-001` and `F-ENG-003` should be retained and reported as the documentation half, cross-referenced to F-ENG-039, and should not carry the behavioural impact a second time in the report's severity totals. Neither file should be merged or deleted.

## Cross-reference (Critic C-ENG-B, covering R9)

Not a critique of this file — C-ENG-A owns that. This is the overlap note §6 of the Critic brief requires, recorded in both places so the two do not contradict each other.

**F-ENG-039 (R9) makes the same behavioural claim as this file**, from the checker side: `BaseChecker`'s allow-lists (`base.rs:13-30`, `:103-147`) are wider than Charter R-4.1's two exceptions. I verified the Charter text independently against `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:516-528` and both files are accurate on it.

**Recommended canonical assignment:**

- **F-ENG-039 canonical for the behavioural defect** — one fix site (`checkers/base.rs`), it covers R-4.1 and R-4.2 together, and it additionally reaches the two Charter conditions this file does not: the "non-batched" requirement (`:520`) and the "`value` is `0`" requirement (`:524`), neither of which `check_calls` (`base.rs:87-99`) or `check_multi_send` (`base.rs:205-213`) implements.
- **This file canonical for the `engine/rule.rs:16-19` doc-comment defect** — that `RuleId::R4_1SettingsChange` _documents_ a rule the Charter does not contain. That is a genuinely separate and worth-keeping issue: the doc comment is what a future maintainer implements against, so leaving it stating a non-existent rule invites the behaviour to be reintroduced after any fix to `base.rs`.

Nothing merged, nothing deleted. Whoever compiles the report should present this as **one behavioural finding plus two documentation findings** (with F-ENG-003), not three independent Charter violations — otherwise the same defect is counted three times in the severity roll-up.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the behavioural half of this finding is filed canonically as F-ENG-039 (per the Critic), and a second PoC over the same `base.rs` allow-lists would duplicate it; the doc-comment half is prose against an external document and is not executable in principle.

**Certainty unchanged at 85%. Severity unchanged.**

### Remediation check — the two halves must be shipped separately, and labelled

The brief asks that a doc fix and a behaviour fix be kept apart here. They are genuinely separable and their risk profiles could not be more different.

- **Doc half — `rule.rs:16-19`'s description of R-4.1. Sound, zero risk, ship immediately.** Option 1 is correct as written and I would add one requirement: the corrected comment must say _which_ wider set the engine actually allows and that the divergence is deliberate and unresolved, rather than merely deleting the overreaching sentence. A comment that is silent about a known divergence is a worse artefact than one that is wrong, because the next reader has nothing to react to. **This is the canonical home for the `RuleId` doc comment alongside F-ENG-003 and F-ENG-004; F-ENG-039 is canonical for the behaviour.**
- **Behaviour half — option 2. Sound as Charter fidelity, but it is a product decision, not an engineering one, and the finding is right to say so.** Restricting `check_self_calls` to `disableModule` and `setFallbackHandler(address(0))` turns owner, threshold, guard and module changes into `insecure R-4.1` for every Safe using Safenet. `Charter:542` ("Other legitimate Safe settings changes must use the protocol-defined settings-change path outside Safenet's standard transaction-security flow") is indeed the sentence that decides it — but **whether that path exists today is not established anywhere in this checkout**, and shipping option 2 before it does converts this finding into a mass wrong-denial incident. Given that a good-faith denial overruled in arbitration is slashed (`contracts/src/libraries/SentinelOracleRequests.sol:301-304`; see F-ENG-042's `## QA`), the sequencing matters materially. **Verify the settings-change path exists before taking option 2.**
- **Option 3 (middle path) — sound, and the only part I would ship without a product decision.** Requiring `value == 0` on an exempt self-call, and refusing settings self-calls reached through `check_multi_send`, have no legitimate traffic behind them and no Charter ambiguity. They are pure hardening. Take option 3 now and option 1 now; hold option 2.
- **One mechanical defect worth pulling out of option 2 and shipping with option 3:** the allow-list is matched with `starts_with(&…::SELECTOR)` rather than `abi_decode`, so malformed calldata rides an allow-listed selector. The Charter requires canonical ABI encoding; this is F-ENG-039 option 4 and is risk-free.
- **Test hook: exists.** `BaseChecker` is pure and `base.rs:230-764` already has 20 tests. The hooks are fine; what is missing is cases pinning the Charter's `value == 0` and non-batched conditions, which the finding lists. Because the existing suite asserts the _permissive_ answer for several of these, a Charter alignment shows up as a visible test change rather than a silent one — that is a feature, and the fix should be reviewed as such.
- **Where the fix belongs: the Charter-to-`RuleId` mapping (doc) and the checker (behaviour).** Not the combinator — this finding is about what a checker denies, not about which checker's verdict wins.
