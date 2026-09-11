# F-ENG-002 `RuleId::R4_5ExcessiveApproval` claims `setApprovalForAll` is an unconditional immediate failure "per § 2.5"; the Charter makes operator approval-for-all conditional, so the engine denies standard NFT-marketplace approvals

| Field                | Value                                                                                     |
| -------------------- | ----------------------------------------------------------------------------------------- |
| Status               | Verified                                                                                         |
| Crate and module     | sentinel-engine, `engine/rule.rs` (behavioural half in `checkers/excessive_approval.rs`, R9's scope) |
| Location             | `crates/sentinel-engine/src/engine/rule.rs:28-33` (related: `crates/sentinel-engine/src/checkers/excessive_approval.rs:19-33`, `crates/sentinel-engine/src/contracts/target_effects.rs:33-35`, `92-98`) |
| Severity             | Medium / **High** |
| Certainty            | 99% (RW-ENG, Phase 8; E1 — reproduced end-to-end against the running service on local Anvil; was 95%, V-ENG Phase 5) |
| Assumptions involved | A7, A15                                                                                     |
| Tags                 | input-validation, charter-mismatch                                                          |

## Claim

The Charter draws a sharp line inside § 2.5 between two kinds of functionally unlimited approval. A max
`uint256` ERC-20 approval "is **always** functionally unlimited". An ERC-721/ERC-1155 operator approval for
all tokens is functionally unlimited "**unless plausibly required for the stated interaction**", where the
stated interaction is determined from onchain data (§ 2.8) and protocol-recorded purpose (§ 2.11). R-4.5's
"Immediate failure" branch — the one that lets the Council rule "immediately without further analysis" —
names only the max-`uint256` ERC-20 case.

`RuleId::R4_5ExcessiveApproval`'s doc comment erases that line. It lists both forms together and then asserts
"Per § 2.5, this sub-case is always functionally unlimited and needs no further analysis". § 2.5 says the
opposite for the operator-approval half.

`ExcessiveApprovalChecker` implements the doc comment: `EffectKind::OperatorApproval { approved } => approved`
denies on the boolean alone, with no reference to the interaction the approval belongs to. Every
`setApprovalForAll(operator, true)` a Safe proposes is therefore answered `insecure R-4.5`.

`setApprovalForAll(conduit, true)` is not an edge case — it is the mandatory first step of listing an NFT on
every major marketplace, and it is precisely the case the Charter carved out as conditional. So the reference
engine deterministically produces a *denying* vote on a routine, honest transaction class. Two consequences
follow: the honest user's transaction is blocked, and, if the proposer disputes and the Council finds the
approval plausibly required for the stated interaction, the denying sentinels are "affected Sentinels" — those
"whose vote may result in Council-directed slashing in the arbitration" (§ 2.15). The engine's error direction
is the one that costs its operator money.

Note what is *not* wrong here, because it sharpens the finding: the ERC-20 arm
(`amount == U256::MAX`, `excessive_approval.rs:22`) matches the Charter's unconditional rule exactly. The
defect is confined to the `OperatorApproval` arm and to the doc comment that authorises it.

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| - | ----- | ----- | -------- | -------------- |
| 1 | The doc comment groups `setApprovalForAll` with max-`uint256` ERC-20 and asserts, citing § 2.5, that the sub-case is *always* unlimited and needs no further analysis. | E2 | `crates/sentinel-engine/src/engine/rule.rs:28-33` | <pre>    /// Article IV Part B, R-4.5: an authorization-target grant that is<br>    /// functionally unlimited — max `uint256` for an ERC-20 `approve`, or an<br>    /// ERC-721/ERC-1155 "approval for all tokens" (`setApprovalForAll`).<br>    /// Per §2.5, this sub-case is always functionally unlimited and needs no<br>    /// further analysis (unlike the rest of §2.5's amount-reasonableness<br>    /// factors, which remain out of scope for this MVP).</pre> |
| 2 | § 2.5 makes exactly this distinction: the ERC-20 max case is unconditional, the operator approval-for-all case is conditional on the stated interaction. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:169-175` | <pre>#### Rules<br><br>- Max `uint256` ERC-20 approval is always functionally unlimited.<br>- ERC-721 or ERC-1155 operator approval for all tokens is functionally unlimited unless plausibly required for the stated interaction.<br>- “Stated interaction” is determined from onchain data (§ 2.8) and protocol-recorded purpose (§ 2.11).<br>- Offchain context is admissible only if it qualifies as public offchain security evidence under Article III.<br>- Private or unverifiable user-intent statements carry no weight.</pre> |
| 3 | R-4.5's "rules immediately without further analysis" branch is scoped to the max-`uint256` ERC-20 case alone; other amounts are weighed. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:637-650` | <pre>#### Rule<br><br>- A transaction is insecure if it grants a functionally unlimited approval (§ 2.5).<br><br>#### Immediate failure<br><br>- Max `uint256` ERC- 20 approval is always functionally unlimited.<br>- The Council rules immediately without further analysis.<br><br>#### For other amounts, Council weighs<br><br>- token supply and decimals;<br>- nature and scale of the interaction, using onchain data (§ 2.8) and protocol-recorded purpose (§ 2.11);<br>- standard user behavior in comparable interactions (§ 2.9).</pre> |
| 4 | The checker denies on the boolean alone, with no interaction context consulted; the ERC-20 arm beside it is correctly conditioned on `U256::MAX`. | E2 | `crates/sentinel-engine/src/checkers/excessive_approval.rs:19-33` | <pre>    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {<br>        for effect in decode_target_effects(transaction) {<br>            let unlimited = match effect.kind {<br>                EffectKind::Erc20Approval { amount } => amount == U256::MAX,<br>                EffectKind::OperatorApproval { approved } => approved,<br>                _ => false,<br>            };<br>            if unlimited {<br>                return Verdict::Insecure {<br>                    rule: RuleId::R4_5ExcessiveApproval,<br>                };<br>            }<br>        }<br>        Verdict::Abstain<br>    }</pre> |
| 5 | Any `setApprovalForAll` with `approved == true`, on either ERC-721 or ERC-1155 (they share a selector), produces that effect. | E2 | `crates/sentinel-engine/src/contracts/target_effects.rs:92-98` | <pre>    } else if let Ok(call) = erc721::setApprovalForAllCall::abi_decode(data) {<br>        effects.push(TargetEffect {<br>            recipient: call.operator,<br>            kind: EffectKind::OperatorApproval {<br>                approved: call.approved,<br>            },<br>        });</pre> |
| 6 | A denying vote that the Council later contradicts exposes the sentinel to Council-directed slashing. | E2 | `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:329-333` | <pre>### § 2.15 Affected Sentinel<br><br>#### Definition<br><br>- An affected Sentinel is a Sentinel whose vote may result in Council-directed slashing in the arbitration.</pre> |

## Trigger

`POST /v1/security-check` with `operation: 0`, `value: "0x0"`, `to` = any ERC-721 or ERC-1155 collection the
Safe holds, and `data` = `setApprovalForAll(<marketplace conduit>, true)` — the standard, required first
transaction for listing an NFT from a Safe on any major marketplace. The chain reaches
`ExcessiveApprovalChecker` (checker #6, `main.rs:63`) because checkers #1–#5 all abstain on this shape
(Cancellation needs all-zero fields; EscapeHatch needs the announcement selectors; Base allows a `Call` to
another contract at `base.rs:87-90`; Blocklist only fires if the collection itself is listed; NestedSafe
needs `execTransaction` calldata). The response is `{"verdict":"insecure","rule":"R-4.5"}`.

The equivalent Charter-conditional case that should *not* be an immediate failure: the same call where onchain
data (§ 2.8) shows the operator is a canonical marketplace conduit and the Safe's own history shows comparable
listings — "plausibly required for the stated interaction" (`Charter:172`).

## Considered and rejected

- **"§ 3.8's ambiguity standard means a denial is safe anyway."** Rejected. `Charter:450` says the Council
  rules insecure only "If admissible evidence is genuinely evenly balanced on a material security question".
  That is a tie-breaker for genuine ambiguity, not a licence to deny when § 2.8 onchain data and § 2.9
  standard user behaviour both support the approval. § 3.2 (`Charter:368-369`) directs the Council to infer
  objective intent from "calldata, decoded transaction effects, Safe transaction `to` addresses, target
  addresses under § 2.4, value transfers, prior onchain behavior" — exactly the evidence that resolves an
  NFT-listing approval in the proposer's favour.
- **"Abstaining would be worse, so denying is the conservative choice."** Rejected as a defence of *this*
  code: the third option exists and is what the rest of the crate does when it lacks the evidence a rule
  requires. `RuleId`'s own neighbours say so — `rule.rs:24-26` and `rule.rs:58-67` both carry explicit "MVP
  note" caveats about the evidence they do and do not have, and `AddressPoisoningChecker` abstains rather than
  denying when it has no established history to compare against (`address_poisoning.rs:303-307`, the known
  TODO). An abstain costs a vote; a wrong denial costs the user their transaction and can cost the operator
  their bond.
- **"The ERC-20 arm has the same defect."** Rejected — I checked and it does not.
  `excessive_approval.rs:22` requires `amount == U256::MAX`, which is exactly `Charter:171`'s unconditional
  rule and `Charter:643-644`'s immediate-failure branch. (`approve(X, 2^256 - 2)` slipping through is the
  *opposite* direction and is R9's ENG-H7, a separate matter.)
- **"`OperatorApproval` might be reachable only from an unusual shape."** Rejected: it is produced by the
  single `setApprovalForAll` arm at `target_effects.rs:92-98`, reachable both at top level and through one
  level of MultiSend (`target_effects.rs:46-49`), and ERC-721 and ERC-1155 share the selector
  (`target_effects.rs:33-35`), so both standards route here.
- **"The doc comment's parenthetical already disclaims the rest of § 2.5."** It disclaims "the rest of § 2.5's
  amount-reasonableness factors ... out of scope for this MVP", i.e. it says the engine will *not* weigh
  borderline amounts. That is a narrowing, and a fair one. The defect is the sentence before it, which
  *widens* the unconditional branch to cover a case § 2.5 explicitly conditions — the opposite direction, and
  not covered by the disclaimer.
- **Severity: why not High.** PROMPT § 8's High band includes "wrong votes on honest transactions at scale".
  This is deterministic and affects a whole standard transaction class, which argues for High; what I cannot
  establish offline is how much NFT-listing traffic the deployed Safenet Safes actually generate, so
  "at scale" is unproven. I file Medium and flag High for the Critic if that traffic exists.

## Remediation options

1. **Abstain instead of denying on `OperatorApproval`, and fix the doc comment.** One-line behavioural change
   (`excessive_approval.rs:23` -> `false`, or a distinct branch returning `Verdict::Abstain`) plus a doc
   comment that states § 2.5's condition. Tradeoff: the engine stops flagging genuinely hostile
   `setApprovalForAll` grants, which is a real loss — but an abstain is the honest answer for a rule the
   engine has no evidence to evaluate, and it is what the crate already does elsewhere.
2. **Keep the denial but condition it on the operator, as the Charter's "stated interaction" test implies.**
   Deny `setApprovalForAll(operator, true)` unless `operator` is on a configured allow-list of canonical
   marketplace conduits, mirroring how `BaseChecker` treats fallback handlers and modules
   (`base.rs:13-30`) and how `CowChecker` treats `GPv2VaultRelayer`. Tradeoff: an address list that must be
   maintained per chain, and it still denies legitimate approvals to anything not yet listed — but it moves
   the failure from "always wrong" to "wrong only for unlisted venues".
3. **Reuse the evidence the engine already gathers.** `AddressPoisoningChecker` already answers "has this Safe
   interacted with this address before?" from `eth_getLogs` on `Transfer`/`Approval`
   (`address_poisoning.rs:189-228`). An operator the Safe has previously approved or transacted with is the
   closest available proxy for § 2.5's "plausibly required for the stated interaction". Tradeoff: makes
   R-4.5 RPC-backed and therefore subject to the same abstain-on-RPC-failure behaviour as address poisoning.

Tests to add: an `excessive_approval.rs` case pinning the intended verdict for
`setApprovalForAll(<conduit>, true)`; a case asserting `approved: false` stays `Abstain` (currently implied
by `excessive_approval.rs:23` but not pinned); and, if option 2 or 3 is taken, a case for an
operator that is *not* recognised. The existing four tests in that file do not distinguish the two § 2.5
branches.

## Trail

- Reviewer R8: drafted, self-estimate 80%. The Charter text and the code are both direct reads and
  I consider the *mismatch* itself close to certain; the 20% is the impact argument — whether the Council
  would in fact rule a marketplace approval-for-all secure (I read `Charter:172` plus § 2.8/§ 2.9 as saying
  yes) and how much such traffic exists. `checkers/excessive_approval.rs` is R9's file; R9 and `C-ENG` should
  reconcile with ENG-H7, which points at the same checker from the opposite direction.

## Critic (C-ENG-A)

I read § 2.5 and R-4.5 in the Charter and `excessive_approval.rs` before reading R8's argument. The Charter
text is unambiguous and the code contradicts it in one line. **No claim in this finding is `H`.**

### Per-claim verdicts — the two that decide it

**Supported.** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:169-175`, § 2.5 `#### Rules`:

```
- Max `uint256` ERC-20 approval is always functionally unlimited.
- ERC-721 or ERC-1155 operator approval for all tokens is functionally unlimited unless plausibly required for the stated interaction.
- "Stated interaction" is determined from onchain data (§ 2.8) and protocol-recorded purpose (§ 2.11).
```

The two bullets are deliberately different sentences. The first is unconditional; the second carries
"**unless plausibly required for the stated interaction**", and the third bullet says the qualifier is
*determinable* from evidence the engine could in principle consult, so it is not decorative.

**Supported.** R-4.5's `#### Immediate failure` block, `Charter:641-644`:

```
#### Immediate failure

- Max `uint256` ERC- 20 approval is always functionally unlimited.
- The Council rules immediately without further analysis.
```

The "rules immediately without further analysis" licence attaches to the max-`uint256` ERC-20 case **only**.
`rule.rs:31-33` transplants it onto the operator-approval case ("Per §2.5, this sub-case is always
functionally unlimited and needs no further analysis"), which is the precise inversion of Charter:172.

**Supported.** `excessive_approval.rs:21-30` implements the doc: `EffectKind::OperatorApproval { approved }
=> approved` denies on the boolean alone. `excessive_approval.rs:93-114`
(`denies_operator_approval_for_all`) pins it as intended behaviour, so this is not an oversight that a test
would catch — it is the specification the suite enforces. I also confirmed R8's sharpening: the ERC-20 arm
(`amount == U256::MAX`, `:22`) matches Charter:171 exactly, so the defect really is confined to one arm.

**Supported.** Reachability: `target_effects.rs:92-98` is the only producer of `OperatorApproval`, and
`main.rs:63` places `ExcessiveApprovalChecker` sixth, after five checkers that all abstain on this shape.

### Severity: Medium → **High**

R8 filed Medium and explicitly asked the Critic to settle "at scale". I raise it to **High**, on three
grounds R8 did not have in front of it.

1. **The wrong vote is actually cast, with a bond behind it.** Unlike F-ENG-001's abstain, an `Insecure`
   verdict reaches `crates/sentinel/src/service.rs:175` as `CheckOutcome::Denied(rule) => (false,
   rule.to_string)` and then `service.rs:182` `self.commit_vote(state, request_id, approve, reason,
   request)` — described at `service.rs:196-197` as "Starts voting on an open request by locking a bond
   behind a blind commitment". So the engine converts an honest NFT approval into a committed, bonded,
   denying vote.
2. **The honest proposer has no recovery path.** Charter:874-875:
   `- A Council ruling determines the applicable Sentinel slashing outcome but does not authorize the
   disputed transaction for validator attestation or execution.` / `- A transaction that enters arbitration
   remains ineligible for validator attestation regardless of the ruling.` Winning the arbitration does not
   get the user their transaction; the denial is terminal for that proposal.
3. **"At scale" is a property of the transaction class, not of today's volume.** The scale question is
   whether a *standard, deterministic* class of honest traffic is affected, and it is:
   `setApprovalForAll(operator, true)` is the mandatory grant for every ERC-721/ERC-1155 marketplace
   listing, vault deposit, bridge and staking flow. Every single one is denied, every time, with no
   evidence gathered. That is PROMPT § 8's "wrong votes on honest transactions at scale". I do not need the
   deployed Safes' NFT volume to establish it, and R8's caution on that point — while good discipline — set
   the bar in the wrong place.

The operator-cost argument R8 raises (§ 2.15 affected-Sentinel slashing exposure) is real and I confirmed
the citation, but it is secondary; the primary harm is to the user whose transaction is killed.

### Finding verdict and certainty

**Confirmed — 85%.** Mechanism verified line by line; trigger is a single well-formed request body whose
path through the checker chain I walked independently. The residual is the same interpretive question as
F-ENG-001 (is `rule.rs` normative?) plus the fact that the "plausibly required" test needs judgement the
engine cannot mechanise — which argues for **abstaining**, the third option R8 correctly identifies, not for
denying. 89% is this run's ceiling.

### Relationship to other findings

Complementary to **F-ENG-036** (R9), which is the *opposite* error direction on the same rule
(`approve(X, 2^256-2)` evades the `U256::MAX` equality). Both should be reported; neither is canonical over
the other. Together they say R-4.5 is implemented as two exact predicates where the Charter specifies one
unconditional case and one evidence-weighed case.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection.** `decode_call` maps `setApprovalForAll` to
`EffectKind::OperatorApproval { approved }` (`contracts/target_effects.rs:26-42` for the type),
`excessive_approval.rs:23` reads `EffectKind::OperatorApproval { approved } => approved`, and `:27-29`
returns `Insecure { rule: R4_5ExcessiveApproval }` — with no reference to the operator, the collection, or
any evidence about the interaction. I also confirmed the ERC-20 arm at `:22` is `amount == U256::MAX`,
which matches §2.5's unconditional case exactly, so the defect really is confined to the
`OperatorApproval` arm and to the `rule.rs:28-33` doc comment that authorises it. Not `E1`.

**Certainty unchanged at 85%. Severity unchanged at High.**

**Charter §2.15 verified verbatim.** This finding's Basis row 6 cites §2.15 for the operator-cost argument.
The Charter copy is restored at upstream `44a1e53`, and §2.15 is at **line 329**; row 6's quote is
**character-exact** against lines 329-333: "An affected Sentinel is a Sentinel whose vote may result in
Council-directed slashing in the arbitration." No change to the Basis table is needed. I went further and
settled the substantive question §2.15 only gestures at — §2.15 says "*may* result", and Charter §6.5
defers bonds and slashing to protocol rules — by reading the protocol rules in this repository:
`contracts/src/libraries/SentinelOracleRequests.sol:298-305` charges `terms.slashAmount` to a revealed vote
whenever `approved != (state == State.RESOLVED_APPROVED)`, with no good-faith exception.
**So a sentinel that denies a routine marketplace listing, is disputed, and is overruled by the Council is
slashed for a correct-by-its-own-lights denial.** Full record in F-ENG-042's `## QA (QA-ENG)`.

**PoC: `rust-audit/poc/F-ENG-002/`** — four tests appended to `excessive_approval.rs`. The fixture is an
**honest** transaction (`setApprovalForAll(<Seaport conduit>, true)` on a collection the Safe holds), not
an attack: this and F-ENG-042 are the two engine findings whose error direction is *denying*, and the PoC
README says so at the top so a reader does not mistake it for a Critical.

### Remediation check

**Keep the two halves apart** — the brief asks for this explicitly, and this is one of the three
doc-comment findings.

- **The doc-comment half (`rule.rs:28-33`).** It asserts "Per §2.5, this sub-case is always functionally
  unlimited and needs no further analysis" of *both* forms; §2.5 says that of the ERC-20 max case only, and
  makes operator approval-for-all conditional on being "plausibly required for the stated interaction". A
  pure documentation fix, shippable today with zero behavioural risk. It belongs with **F-ENG-001 and
  F-ENG-003**, which the Critic named canonical for `RuleId` doc comments. **F-ENG-039 is canonical for the
  behavioural `BaseChecker` defect and is not implicated here** — this finding's behavioural half lives in
  `excessive_approval.rs`, not `base.rs`.
- **The behavioural half (`excessive_approval.rs:23`).** Fixing the comment alone leaves a checker that
  contradicts its own (now correct) `RuleId` documentation — arguably worse than today, because the
  discrepancy is no longer visible in a single file. **Ship both or neither.**
- **Option 1 (abstain on `OperatorApproval`, and fix the doc) — sound, and the honest answer.** Abstaining
  is what the crate already does wherever it lacks evidence for a rule. The cost is stated fairly in the
  finding: real coverage of hostile `setApprovalForAll` grants is lost — but it is coverage the engine was
  never entitled to, since it decides a conditional rule without evaluating the condition.
- **Option 2 (deny unless the operator is on an allow-list of canonical conduits) — sound, preserves
  coverage, but must fail open.** It mirrors `base.rs:13-30`'s handling of fallback handlers and modules and
  `CowChecker`'s handling of `GPv2VaultRelayer`, so it fits the crate. Cost: a per-chain list, and it still
  denies honest approvals to unlisted venues. **Given the bond exposure established above, if option 2 is
  chosen it must abstain — not deny — on an unlisted operator**, or the fix reproduces the finding for every
  marketplace that launches after the list was written.
- **Option 3 (reuse `AddressPoisoningChecker`'s history as a proxy for "plausibly required") — unsound as a
  denial basis; I reject it.** Prior interaction is weak evidence in the affirming direction and
  near-worthless in the denying one: a Safe listing on a marketplace for the *first* time has no history
  with that conduit and is precisely the honest user this finding is about, so option 3 denies them. It also
  inherits F-ENG-033's result that the same history is forgeable, and makes R-4.5 RPC-backed with
  F-ENG-005's and F-ENG-009's consequences.
- **Test hook: none needed.** `ExcessiveApprovalChecker` and `decode_target_effects` are pure. The four
  existing tests (`excessive_approval.rs:49-135`) do not distinguish §2.5's two branches.
- **Where the fix belongs: the checker *and* the Charter-to-`RuleId` mapping** — both, with the mapping
  first in importance, because its doc comment is what authorises the behaviour.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 85% -> **95%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a
binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source
file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under
`rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file
was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only
executable oracle for this checker.

### Note the direction: this one denies honest traffic

Unlike the Criticals verified this phase, the fixture here is an **honest** transaction that the engine denies —
the mandatory first step of listing an NFT from a Safe on any major marketplace. This is the error direction
that costs the engine's own operator money (Charter §2.15; `SentinelOracleRequests.sol:298-305` slashes a
revealed vote on the losing side of a resolved dispute, with no good-faith exception).

### What was run

`rust-audit/poc/F-ENG-002/append-to-src-checkers-excessive_approval.rs` appended to
`crates/sentinel-engine/src/checkers/excessive_approval.rs` (alongside F-ENG-036's module, which appends to the
same file and does not collide), then `cargo test -p sentinel-engine poc_f_eng_002`. Compiled on the first
attempt. Full output: `rust-audit/poc/F-ENG-002/run-output.txt`.

### Verbatim result

```
running 4 tests
test checkers::excessive_approval::poc_f_eng_002::poc_f_eng_002_control_revocation_abstains ... ok
test checkers::excessive_approval::poc_f_eng_002::poc_f_eng_002_control_the_erc20_max_arm_stays_a_denial ... ok
test checkers::excessive_approval::poc_f_eng_002::poc_f_eng_002_denies_a_routine_marketplace_listing_today ... ok
test checkers::excessive_approval::poc_f_eng_002::poc_f_eng_002_a_conditional_rule_must_not_be_an_immediate_failure ... FAILED

---- ..._a_conditional_rule_must_not_be_an_immediate_failure stdout ----
assertion `left != right` failed: § 2.5 conditions operator approval-for-all on the stated interaction; the \
engine has gathered no evidence about that interaction and must not deny on the boolean
  left: Insecure { rule: R4_5ExcessiveApproval }
 right: Insecure { rule: R4_5ExcessiveApproval }

test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 102 filtered out
```

The single expected-to-fail test failed for the claimed reason.

### What this establishes

`setApprovalForAll(operator = OpenSea Seaport 1.6 conduit `0x1E0049…3c71`, approved = true)` on a canonical
ERC-721 collection, sent from a Safe with everything else zero, is returned
`Insecure { rule: R4_5ExcessiveApproval }`. The denial is reached from the boolean alone
(`EffectKind::OperatorApproval { approved } => approved`, `excessive_approval.rs:23`) — no reference to the
operator's identity, the collection, or the interaction the approval belongs to. **The engine deterministically
votes `insecure` on routine honest marketplace listings.**

Both controls passed and bound any fix: the ERC-20 max-`uint256` arm must stay a denial (Charter §2.5 makes
that case unconditionally functionally unlimited, and `excessive_approval.rs:22` implements it exactly), and
revocation (`approved: false`) must stay non-denying.

Residual uncertainty: the behaviour is settled; what remains is the Charter reading under A7/A15 — that §2.5's
operator-approval arm is *conditional* ("unless plausibly required for the stated interaction") and that R-4.5's
immediate-failure branch names only the max-`uint256` ERC-20 case. That reading is C-ENG-B's and is not itself
executable.

## Real-world validation (Phase 8, RW-ENG)

### Scenario

This is the finding whose error direction costs honest users, so it was given a realistic payload:
a Safe that **genuinely owns** an NFT, approving the **real OpenSea/Seaport conduit** address.

Live deployment: Anvil 1.8.1 on `127.0.0.1:8545` (chain 31337); a real Safe 1.5.0 proxy at
`0x643d887734c637f108B095dc3EE0e06F79bC320C`; an ERC-721 collection at
`0x0165878A594ca255338adfa4d48449f69242Eb8F` with token #1 minted to the Safe
(`ownerOf(1)` → `0x643d887734c637f108B095dc3EE0e06F79bC320C`); the real `sentinel-engine` binary on
`127.0.0.1:5473`, config copied from the shipped sample with `rpc = "http://127.0.0.1:8545"`. The
operator is `0x1E0049783F008A0085193E00003D00cd54003c71`, the OpenSea Seaport conduit — the address a
real listing actually approves.

### Verbatim outcome

Four requests, differing only in the approval they carry:

```
setApprovalForAll(conduit, true)   [the mandatory first step of an NFT listing]
  -> {"verdict":"insecure","rule":"R-4.5"}

setApprovalForAll(conduit, false)  [revocation]
  -> {"verdict":"abstain"}

approve(conduit, MAX_UINT256)      [Charter: always functionally unlimited]
  -> {"verdict":"insecure","rule":"R-4.5"}

approve(conduit, 1e18)             [bounded]
  -> {"verdict":"abstain"}
```

The denied transaction is then executed against the real Safe and the real collection, and is
entirely ordinary:

```
status               1 (success)
isApprovedForAll(safe, conduit) = true
```

### Verdict

**Reproduced end-to-end** on the first attempt. The running service deterministically returns
`insecure R-4.5` for the standard, required, honest first transaction of listing an NFT from a Safe —
a transaction that executes without incident and grants exactly the authority the marketplace needs.

The two control results sharpen the finding rather than soften it, exactly as the claim predicts. The
ERC-20 arm is **correct**: `MAX_UINT256` denies (the Charter's unconditional case) and a bounded
amount abstains. The revocation case is also correct (`approved == false` abstains). The defect is
confined precisely to the `OperatorApproval { approved: true }` arm, where §2.5 makes the judgement
conditional on the stated interaction and the checker makes it unconditional.

No mitigating condition was found in a live deployment. There is no configuration that exempts a
marketplace conduit — `blocklist` is a deny-list only, and `ExcessiveApprovalChecker` (position 6)
reads nothing but the boolean, so no allow-list, no chain state, and no operator setting can prevent
this verdict. Nothing later in the chain can overturn it either, since the chain stops at the first
non-abstaining verdict (F-ENG-044).

Certainty **95% → 99%**. Severity **Medium / High**, unchanged.
