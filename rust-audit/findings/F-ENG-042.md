# F-ENG-042 An address-poisoning denial is issued from an evidence set bounded by recency and by provider completeness, so a genuine payee can be denied under R-4.3/R-4.4

| Field | Value |
| --- | --- |
| Status | QA-done |
| Crate and module | sentinel-engine, checkers/address_poisoning.rs |
| Location | crates/sentinel-engine/src/checkers/address_poisoning.rs:182-228, :334-368 |
| Severity | Medium / Medium (QA-ENG: top of band; see `## QA (QA-ENG)` §3 for the escalation condition) |
| Certainty | 78% (QA-ENG, raised from 72% by C-ENG-B; E2 ceiling 89%, read-only run) |
| Assumptions involved | A2, A3, A4, A15 |
| Tags | verdict-policy, input-validation, charter |

## Claim

`established_recipients` builds the "established" set from `eth_getLogs` over `[block - lookback_blocks, block]` and marks the scan `complete` unless a chunk request returned an **error**. Two ways a genuine recipient can be absent from that set while a lookalike is present — both leaving `complete == true`, so the checker denies:

**(a) Recency.** `from_block = current_block.saturating_sub(self.lookback_blocks)`. A counterparty the Safe last paid longer ago than the lookback (the sample config ships 50,000 blocks, `sentinel-engine.sample.toml:26` — roughly a week on both Gnosis Chain and Ethereum) is simply not established. If any _in-window_ recipient shares 4 leading and 4 trailing nibbles with it, the genuine payment is denied `insecure R-4.3`.

An attacker can manufacture the in-window lookalike cheaply, and the module docs describe the primitive: `transferFrom(safe, R', 1)` needs only an existing allowance on that token — Safes routinely hold allowances to routers and relayers, and any holder of one can move a single unit from the Safe to a mined address `R'`. That puts `R'` in the established set with a non-zero amount, while the real payee `R` has aged out.

**(b) Completeness.** `complete` is only cleared when `provider.get_logs` returns `Err` _and_ some recipients were already collected. A provider that returns a successful but truncated log page (A4 puts "stale / rate-limited / incomplete `eth_getLogs` results" explicitly in scope) yields `Ok(logs)` with the candidate's own `Transfer` missing; the checker then reaches the lookalike branch with `complete == true` and denies. The `RecipientLookup::NoExactMatch` doc explains why an incomplete scan must not deny — but the only incompleteness it can detect is a transport error.

The result is a wrong `insecure` on an honest transaction. The sentinel commits and reveals a denial vote with reason `R-4.3` (`crates/sentinel/src/service.rs:175`); if the transaction goes to arbitration and the Council rules it secure, that is the vote class Charter §2.15 calls an "affected Sentinel". Charter §3.8 also cuts the other way here: it directs that genuinely balanced evidence resolves _insecure_, so the policy of denying on a lookalike is defensible — the defect is that the engine cannot tell a balanced case from a truncated one.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The window is bounded by `lookback_blocks` below `current_block` | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:189-192 | Q1 |
| 2 | `complete` is cleared only on a transport error, and only when some recipients were already gathered | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:200-213 | Q2 |
| 3 | With `complete == true`, a single lookalike among the in-window recipients produces `Insecure` | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:348-368 | Q3 |
| 4 | The lookalike threshold is 4 leading and 4 trailing nibbles | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:78-91 | Q4 |
| 5 | The forged-history primitive is cheap and is documented in the module itself | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:26-36 | Q5 |
| 6 | The Charter's ambiguity rule resolves genuinely balanced evidence as insecure, so the failure is about evidence quality, not policy | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:445-450 | Q6 |

**Q1** `crates/sentinel-engine/src/checkers/address_poisoning.rs:189-192`

```rust
        let from_block = current_block.saturating_sub(self.lookback_blocks);
        let mut recipients = HashSet::new();
        let mut complete = true;
        for (chunk_from, chunk_to) in block_chunks(from_block, current_block, self.max_block_range)
```

**Q2** `crates/sentinel-engine/src/checkers/address_poisoning.rs:200-213`

```rust
            let logs = match self.provider.get_logs(&filter).await {
                Ok(logs) => logs,
                Err(err) if !recipients.is_empty => {
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
```

**Q3** `crates/sentinel-engine/src/checkers/address_poisoning.rs:348-359` and `:367`

```rust
                if !complete {
                    // See `RecipientLookup::NoExactMatch` — can't deny on
                    // an incomplete scan.
                    tracing::warn!(
                        token = %transaction.to,
                        %candidate,
                        %established,
                        rule = kind.rule.code,
                        "address-poisoning: candidate resembles an established recipient, but the scan was incomplete; abstaining rather than denying on partial evidence"
                    );
                    return Verdict::Abstain;
                }
```

```rust
                Verdict::Insecure { rule: kind.rule }
```

**Q4** `crates/sentinel-engine/src/checkers/address_poisoning.rs:78-91`

```rust
fn is_lookalike(established: Address, candidate: Address) -> bool {
    if established == candidate {
        return false;
    }
    let (a, b) = (nibbles(established), nibbles(candidate));
    let prefix = a.iter.zip(b.iter).take_while(|(x, y)| x == y).count;
    let suffix = a
        .iter
        .rev
        .zip(b.iter.rev)
        .take_while(|(x, y)| x == y)
        .count;
    prefix >= 4 && suffix >= 4
}
```

**Q5** `crates/sentinel-engine/src/checkers/address_poisoning.rs:26-36`

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

**Q6** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:445-450`

```text
### § 3.8 Ambiguity

#### Rule

- If admissible evidence is genuinely evenly balanced on a material security question, the Council rules the transaction insecure.
- Minor ambiguity that does not materially affect the security determination does not by itself make a transaction insecure.
```

## Trigger

**Trigger (a) — recency, deterministic given the chain state.** Setup, on the engine's chain, with `address_poisoning_lookback_blocks = 50000` (the sample value, `crates/sentinel-engine/sentinel-engine.sample.toml:26`):

1. The Safe's genuine payee `R` was last paid in token `T` at block `b_R`, with `block - b_R > 50000`.
2. The attacker mines `R'` sharing `R`'s first 4 and last 4 hex digits (about 2^32 keccak trials with a standard vanity-address generator) and, using any spender that already holds an allowance from the Safe on `T`, calls `T.transferFrom(safe, R', 1)` at a block inside the window.
3. The Safe now proposes its ordinary payment: `to = T`, `operation = 0`, `data = transfer(R, <amount>)`, `value = 0`, all gas fields zero, `chainId` matching the provider.

`established_recipients` finds no exact match for `R` (aged out), finds `R'` in the set, `is_lookalike(R', R)` is true, `complete` is true, so the response is `{"verdict":"insecure","rule":"R-4.3"}` for an entirely honest payment. Note the roles are symmetrical: `is_lookalike` is called as `is_lookalike(established, candidate)`, so seeding a lookalike of `R` denies `R`, not the other way round.

**Trigger (b) — truncation.** Configure `address_poisoning_max_block_range` large enough that one chunk covers more logs than the provider will return in a single response, against a provider that truncates rather than erroring. Same three-step setup, except `R` was paid _inside_ the window and its log falls in the truncated tail. Same denial, with no warning logged.

Corpus shape: (a) is reproducible entirely offline against a mocked provider (`Provider::mocked` plus a canned log set) — two log fixtures differing only in whether `R`'s own `Transfer` is present, asserting `secure` and `insecure R-4.3` respectively. That fixture pair is also the regression test for any fix.

## Considered and rejected

- **`complete == false` already covers this.** It covers only transport errors on a _later_ chunk (Q2). A first-chunk error returns `Err` (abstain, correctly); a successful-but-partial response is indistinguishable from a complete one.
- **A wider lookback fixes (a).** It raises the bar but does not remove it — every finite window has an aged-out payee, and `lookback_blocks` has no upper bound in config, so raising it instead trades correctness for per-request RPC cost (⌈(lookback+1)/(max_range+1)⌉ sequential `eth_getLogs` calls, `address_poisoning.rs:192`).
- **This is the documented `transferFrom` gap.** The denial-of-service half _is_ documented at `address_poisoning.rs:33-36` (Q5) — but as a property of forged events, not as a consequence of the window bound, and it is not among the codebase map's §4 known items, so it is filed as a finding rather than tagged `known`. Variant (b), the silent-truncation path, is not documented anywhere.
- **The provider is trusted, so (b) is out of scope.** A4 explicitly keeps stale, rate-limited and **incomplete** `eth_getLogs` results in scope; only an actively malicious RPC is excluded.
- **A false denial is harmless — the transaction just does not execute.** It costs the Safe a legitimate payment and puts the sentinel on the wrong side of a possible arbitration (Charter §2.15); it is also indistinguishable to the operator from a real poisoning attempt, since both log the same `debug!` line (Q3).

## Remediation options

1. Make evidence quality explicit: only deny when the candidate's absence is _evidence of absence_. E.g. require that the candidate itself has no `Transfer`/`Approval` involving the Safe over a much longer (or unbounded) window before treating it as novel — a second, candidate-scoped `eth_getLogs` with `topic2 = candidate` is far cheaper than widening the whole scan.
2. Detect truncation: compare the returned log count against a configured provider page size, or re-issue the chunk split in half when the count hits a suspicious boundary, and clear `complete` when it does.
3. Weight the evidence rather than thresholding it: require the established lookalike itself to have more than a single 1-unit event before it can ground a denial (a forged `transferFrom(safe, R', 1)` would then not qualify), per the Charter's §3.8 "no single circumstantial factor is determinative".
4. Distinguish the two abstentions and the denial in metrics/logs so operators can see how often this fires.

Tests to add: the mocked-provider fixture pair described above; a test that a truncated log page does not produce a denial. The current suite tests only `nibbles`, `is_lookalike` and `block_chunks` (`address_poisoning.rs:388-457`) — `check` itself is never exercised. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 70%. (Not in the analysis file's hypothesis list; the DoS half is acknowledged in the module docs but the window-bound and truncation mechanisms are not. Variant (a) is E2; variant (b) depends on provider behaviour that cannot be observed offline, so its trigger is E2 for the code path and I for the provider, which is why the self-estimate is lower than for the other findings.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts

All five code claims re-opened against this checkout; all **Supported**, quotes accurate at the stated lines.

| # | Verdict | Note from re-opening |
| --- | --- | --- |
| 1 | **Supported** | `address_poisoning.rs:189` is `let from_block = current_block.saturating_sub(self.lookback_blocks);`. The window is `[block - lookback, block]` with no floor other than 0. |
| 2 | **Supported** | `:200-213`. Worth spelling out the two arms, because the asymmetry is the defect: `Err(err) if !recipients.is_empty` sets `complete = false` and breaks, while a _first-chunk_ error hits `Err(err) => return Err(err)` and becomes `Abstain` at `:369-378`. Neither arm can observe a successful-but-short response. `complete` starts `true` (`:191`) and is only ever cleared by a transport error. |
| 3 | **Supported** | `:348-359` returns `Abstain` when `!complete`; `:367` returns `Insecure { rule: kind.rule }` otherwise. So `complete == true` plus one lookalike is sufficient for a denial. |
| 4 | **Supported** | `:78-91`, `prefix >= 4 && suffix >= 4`. |
| 5 | **Supported** | `:26-36`. The module's own docs describe the denial-of-service direction explicitly ("it can instead deny later _genuine_ payments to `R` — a denial-of-service, not just a false approval"), so the primitive is acknowledged by the authors. |
| 6 | **Supported** | I read §3.8 in full earlier in this run and confirm `:449`: "If admissible evidence is genuinely evenly balanced on a material security question, the Council rules the transaction insecure." R9's use of it is fair — it cuts _against_ the finding being a policy complaint, and R9 says so rather than hiding it. |

**One citation I could not re-open, flagged rather than passed.** The prose cites Charter §2.15's "affected Sentinel" for the consequence of a wrong denial. The session-local Charter copy did not survive a VM restart mid-run, so I verified §2.5, §2.18, §3.7, §3.8, R-4.1, R-4.2 and R-4.5 while it was available but **never re-opened §2.15**. It is not a `Basis` row, so no basis claim depends on it, but the report should treat that one sentence as unverified until someone re-checks it — and it matters, because if a wrong denial does expose an honest sentinel's bond, the severity analysis in §3 below changes.

### 2. My own analysis — the two paths are not independent, which R9 does not say

I worked through both mechanisms before reading R9's argument and reached the same code reading, plus one structural point that changes how the finding should be presented.

**Path (b) cannot fire on its own.** A truncated-but-`Ok` page only produces a wrong denial if, among the recipients that _did_ survive truncation, one is a 4+4-nibble lookalike of the candidate. Absent an attacker, that coincidence is what the module itself calls "astronomically unlikely (roughly 1 in 16^8)" (`address_poisoning.rs:76-77`) — and I agree with that arithmetic. So truncation alone does not generate false denials; it converts a situation an attacker has _already_ constructed from `Abstain` into `Insecure`. Path (b) is an amplifier of path (a), not a second independent trigger. That is worth stating because a reader could otherwise take (b) as "any flaky provider starts denying honest payments", which is not so.

**Path (a) is the real one, and it is attacker-driven, not accidental.** It needs, conjunctively: a mined lookalike `R'` of the real payee `R` (16^8 ≈ 4.3×10⁹ trials — a routine vanity/CREATE2 grind, so not a barrier); a non-zero `Transfer` with `topics[1] == safe` on **the same token** inside the window; and `R` itself aged out past `lookback_blocks`. The middle condition is the binding one: the only route an attacker has to it is `transferFrom(safe, R', 1)`, which requires a pre-existing allowance from the Safe on that specific token. The `Approval` half of the evidence pool is not available to them — `Approval`'s `topics[1]` is the owner, so producing one would require the Safe itself to call `approve`. R9's supporting premise that "Safes routinely hold allowances to routers and relayers" is plausible but is class `I`: nothing in this checkout establishes it for any particular Safe.

### 3. Severity — I judged this deliberately against the High band, and land on Medium

The coordinator is right that this is a different severity class from the Criticals: it is a wrong vote in the _denying_ direction, against honest traffic. PROMPT.md §8's High band includes "wrong votes on honest transactions **at scale**", and that qualifier is what decides it.

This is not at scale. Per §2 the effective trigger is conjunctive and targeted: one attacker, one victim Safe, one specific counterparty relationship `R`, one specific token on which the attacker must already hold an allowance, and a payee that has aged past the configured lookback (50,000 blocks in the shipped sample, `crates/sentinel-engine/sentinel-engine.sample.toml:26` — roughly a week). Nothing here scales across Safes or across transactions; each victim relationship must be attacked individually and re-attacked as the window slides. The honest-traffic harm is a targeted denial-of-service against one payment corridor.

Two considerations I weighed for raising it and rejected:

- _Availability._ A wrong `insecure` blocks a legitimate payment, which is real harm to that user. But §8's High band is about an honest validator or sentinel losing liveness, and here the sentinel is fully live — it votes, correctly per its own evidence. The Safe owner can re-propose; the window slides.
- _Bond exposure._ If a wrong denial exposes the honest sentinel's bond, "loss of bonded funds" enters the picture. This is exactly the §2.15 citation I could not re-open (§1 above). If a later reader verifies that a sentinel on the losing side of an arbitration is slashed for a denial it cast in good faith, **this finding should be re-scored upward**, and I am flagging that explicitly so the possibility is not lost.

On the evidence I can actually verify: **Medium** — §8's "incorrect behaviour under unusual but reachable conditions" — which matches R9's own rating. I am confirming their number after independent derivation, not inheriting it.

### 4. Trigger quality

Adequate but the weakest-specified of R9's set, and QA should not attempt it as a static corpus vector: both paths need chain state (an aged-out payee, a mined lookalike, an in-window forged log) plus, for (b), a provider that truncates without erroring. Path (b) in particular is only reachable with a mocked provider — and `Provider::mocked_with_chain` (`crates/core/src/provider/mod.rs:147-149`) exists, so a truncating-provider test is feasible even though it cannot be an external-corpus vector. That is the cheapest reproduction available and I would point QA at it first.

### 5. Finding verdict

**Confirmed.** Mechanism verified line by line; trigger verified for path (b) under A4, which places "incomplete `eth_getLogs` results" explicitly in scope, and for path (a) with one `I` step (allowance availability). Certainty **72%** — genuinely Confirmed, but at the bottom of the 70-89 band because the highest-impact framing depends on that `I` step and on the unverified §2.15 citation. Severity **Medium** (unchanged, after the deliberate re-derivation in §3).

## QA (QA-ENG)

**Execution: Not attempted (no toolchain).** No `cargo` on this host (`rust-audit/state/baseline.md` §1); `E1` is unreachable this run, so the 90-100 band stays closed. No PoC was written for this finding — see "Why no PoC" below.

**Certainty: raised 72% → 78%** (still within the `E2` ceiling of 89%). The raise is on new evidence of my own, below, not on re-reading the Critic's argument: the one citation C-ENG-B flagged as unverified is now verified verbatim, and the substantive question that flag was standing in for — _does a wrong denial expose an honest sentinel's bond?_ — is answered **yes**, from in-repo Solidity rather than from inference.

**Severity: I do not raise it to High. Recommend Medium (top of band), with the escalation condition recorded.** Reasoning in §3 below; C-ENG-B asked for a re-score if the bond question resolved, and I am declining to apply it mechanically while recording exactly what would change my mind.

### 1. Charter § 2.15 — settled, verbatim

The Charter copy was restored at upstream commit `44a1e53` and § 2.15 is at **line 329**, exactly where F-ENG-002's Basis row 6 places it. Quoted in full, lines 329-333:

```text
### § 2.15 Affected Sentinel

#### Definition

- An affected Sentinel is a Sentinel whose vote may result in Council-directed slashing in the arbitration.
```

**Does it support this finding's claim?** The finding's prose says a sentinel that reveals a denial the Council later contradicts "is the vote class Charter § 2.15 calls an 'affected Sentinel'". That is **supported as a definition** and the quote is accurate. Two qualifications a careful reader must keep:

1. § 2.15 says _"may result in"_. It defines a category of exposure, not an outcome. On the Charter's text alone the finding could claim only "this vote is in the category the Charter says may be slashed" — not that it is slashed.
2. The Charter deliberately does not decide the question. § 6.5 assigns it elsewhere — "Protocol economic mechanics include: bonds; slashing" and, at line 33, "Execution, fees, bonds, slashing, and timeout consequences remain governed by Safenet protocol rules". Corroborating context: § 6.3 step 2, "The disputed transaction, Sentinel votes, and affected Sentinels are identified"; § 6.5, "A Council ruling determines the applicable Sentinel slashing outcome"; § 6.5, "Council-directed slashing may occur only following a valid Council ruling".

So § 2.15 alone is **necessary but not sufficient** for the bond-exposure claim. I therefore went to the protocol rules it defers to.

### 2. The protocol rules settle it: a good-faith denial that loses arbitration is slashed

`contracts/src/libraries/SentinelOracleRequests.sol:298-305`, the whole of `slashAmountFor`'s revealed-vote branch:

```solidity
        // A revealed vote is only slashed for losing an arbitrated dispute -- a winner, a lone
        // unopposed revealer, or anyone made whole by a timeout (total or arbitration) keeps
        // their full bond.
        if (state != State.RESOLVED_APPROVED && state != State.RESOLVED_DENIED) return 0;
        if (approveSentinelCount == 0 || denySentinelCount == 0) return 0;
        bool approved = vote == SentinelOracleCommitment.Vote.APPROVED;
        return approved != (state == State.RESOLVED_APPROVED) ? self.terms.slashAmount : 0;
```

A sentinel whose revealed vote is `DENIED` on a request the Council resolves `RESOLVED_APPROVED` satisfies `approved != (state == RESOLVED_APPROVED)` and is charged `terms.slashAmount`. Nothing in that expression asks whether the vote was cast in good faith, or whether the engine that produced it was correct on its own evidence. `contracts/docs/SentinelOracle.md:109` states the same in prose — "the winner (Alice) keeps her bond plus the fee, and the loser (Bob) is slashed" — and `:119` defines the amount as `fee × slashingMultiplier`, "charged per sentinel on the losing side of a resolved dispute".

**So yes: a wrong denial does expose the honest sentinel's bond.** This is class `E2`, from repository Solidity under A7, not `I`. It is exactly the fact C-ENG-B could not check.

Three bounds on it, which is why §3 does not follow automatically:

- The loss is **`slashAmount`, not the bond**. `contracts/docs/SentinelOracle.md:117-119` gives the launch parameters as `bondTarget = 800 USDC` against `slashAmount = 4.00 USDC`, with `slashingMultiplier` described as "kept low at launch". `slashingMultiplier` is governance-set (`SentinelOracle.sol:357-359`), so this is a parameter, not a constant.
- It requires a **split vote** — `approveSentinelCount == 0 || denySentinelCount == 0` returns 0 — plus a freeze, plus arbitration, plus a Council ruling _for_ approve. A denial nobody contests costs nothing.
- If no valid ruling lands within four weeks, Charter § 6.3 step 10 returns all bonds and applies no Council-directed slashing.

### 3. Severity: Medium (top of band), and what would move it

PROMPT § 8's bands that could apply are Critical's "loss of bonded funds **at scale**", High's "unbounded fund drain through gas or bonds", and High's "wrong votes on honest transactions **at scale**". The bond loss established in §2 is neither at scale nor unbounded: it is `fee × slashingMultiplier` per contested request, and C-ENG-B's §2 conjunctive-trigger analysis — one attacker, one victim Safe, one counterparty relationship, one token on which the attacker already holds an allowance, and a payee aged past the configured lookback — stands unchanged. What §2 changes is the **kind** of harm, from availability-only to bounded monetary loss, which is why I place it at the top of Medium rather than in the middle.

Recorded escalation conditions, so this is not lost again:

- **If `slashingMultiplier` is raised materially** relative to `bondMultiplier` (both are governance parameters, `SentinelOracle.sol:357`), the "unbounded fund drain through bonds" reading of High becomes arguable. Re-score then.
- **If the same primitive is shown to be replicable across Safes** — i.e. one attacker forcing many wrong denials — "at scale" is met and this is High. C-ENG-B's analysis says it is not, and I agree on the evidence available.

### 4. A consistency note for the report

C-ENG-B recorded § 2.15 as **unverified** here, while the _same critic_ wrote in F-ENG-002's Critic section "The operator-cost argument R8 raises (§ 2.15 affected-Sentinel slashing exposure) is real and I confirmed the citation", and F-ENG-002 carries it as an `E2` Basis row (row 6) with the full quote. Both files are now consistent with the restored Charter, and the quote in F-ENG-002 row 6 is **character-exact** against lines 329-333. No change is needed to F-ENG-002's Basis; this section is the record for both.

### 5. Why no PoC

C-ENG-B's §4 is right and I follow it. Path (a) needs real chain state — an aged-out payee, a mined 4+4 lookalike, an in-window forged log — which no static fixture can carry. Path (b) needs a provider that returns a **successful but truncated** `eth_getLogs` page, which is reachable with `Provider::mocked_with_chain` (`crates/core/src/provider/mod.rs:147-149`) plus an `Asserter` that pushes a short page. That is the cheapest reproduction and is a ~30-line test in the harness this audit already wrote for F-ENG-033 (`rust-audit/poc/F-ENG-033/append-to-src-checkers-address_poisoning.rs`): seed two logs' worth of history, return only the one that is _not_ the candidate's, and assert the verdict is `Abstain` rather than `Insecure`. I did not write it because path (b) "cannot fire on its own" (C-ENG-B §2) — it converts an attacker-constructed `Abstain` into an `Insecure`, so a truncation test in isolation asserts a property (`complete` must not be trusted) rather than reproducing the finding. Whoever fixes this should write it as the regression test for the fix.

### 6. Remediation check

The finding files no numbered remediation options, so I state what a fix must do:

- **Detect truncation, do not infer it.** `complete` is cleared only by a transport error and only when recipients were already gathered (`address_poisoning.rs:200-213`); a successful short page is indistinguishable from a complete one. A fix must make incompleteness _observable_ — compare the returned logs' block span against the requested chunk, or cap the result count and treat a full page as suspicious — rather than adding more error arms. **Sound and necessary.**
- **Do not "fix" it by widening the lookback.** A larger `address_poisoning_lookback_blocks` narrows path (a) but multiplies the per-request `eth_getLogs` fan-out, which F-ENG-009 establishes is already unvalidated and unbounded. That trade must be made deliberately, in `Config::validate`, not by editing the sample TOML.
- **The durable fix is per-log provenance** (F-ENG-033 option 4, and `address_poisoning.rs:38-39`'s own note): if only logs whose originating transaction was sent by the Safe count as evidence, the attacker's `transferFrom(safe, R', 1)` stops establishing `R'` and path (a) closes at its root. Cost is an extra RPC round trip per candidate log, so it is blocked behind F-ENG-009. **Sound; sequence it after F-ENG-009.**
- **Charter § 3.8 cuts against over-correcting.** Line 449: "If admissible evidence is genuinely evenly balanced on a material security question, the Council rules the transaction insecure." So _denying on a lookalike_ is the Charter-aligned policy and must not be softened to an abstain across the board; the defect is that the engine cannot tell a balanced case from a truncated one. A fix that simply stops denying trades this finding for F-ENG-041's coverage gap.
- **Test hook.** `Provider::mocked_with_chain` already exists and is sufficient for path (b). Nothing new is needed. The `sentinel-test-vectors` corpus (A8, unavailable) cannot express this finding: it needs a provider that misbehaves, not a request that is unusual.
- **Where the fix belongs: the checker** (`address_poisoning.rs`), specifically the evidence-completeness model. Not the combinator, and not the `RuleId` mapping.
