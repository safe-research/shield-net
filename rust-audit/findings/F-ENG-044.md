# F-ENG-044 The engine's first-non-abstain-wins combinator cannot implement Charter §3.7, so one over-broad affirmer overrides every rule that never ran

| Field | Value |
| --- | --- |
| Status | Verified |
| Crate and module | sentinel-engine, engine/mod.rs (chain composed in main.rs) |
| Location | crates/sentinel-engine/src/engine/mod.rs:57-72 (related: :35-48, :104-120, crates/sentinel-engine/src/main.rs:57-73) |
| Severity | C-ENG-B / High |
| Certainty | 99% (RW-ENG, Phase 8; E1 — reproduced end-to-end against the running service on local Anvil; was 98%, V-ENG Phase 5) |
| Assumptions involved | A2, A3, A15 |
| Tags | verdict-policy, charter, architecture |

## Claim

`SentinelEngine::security_check` returns the verdict of the **first** checker that does not abstain. There is no aggregation and no second pass: once any checker answers `Secure`, every checker registered after it is never invoked, and the rules those checkers implement are never evaluated for that transaction.

Charter §3.7 requires the opposite: a transaction is secure _only if it satisfies all applicable Article IV rules_. A combinator that stops at the first affirmation cannot establish that conjunction — it can only report that one checker, looking at one aspect, had no objection. The engine's own type documentation states the conjunctive semantics the loop does not provide: `Verdict::Secure` is documented as "All configured checks consider the transaction secure" (`engine/mod.rs:39`), which is false for every `Secure` the engine has ever returned other than one produced by the last checker in the chain.

This is the shared root cause of six separately-filed findings. F-ENG-030 (nested), F-ENG-033 and F-ENG-036 (address-poisoning history), F-ENG-034 and F-ENG-035 (escape hatch ahead of the blocklist) and F-ENG-037 (CoW TWAP) are each an instance of the same shape: _one checker affirms on a narrow structural predicate, and that affirmation is promoted to a verdict about the whole transaction._ F-ENG-031 is the same defect seen from the other side — the refund leg is never examined because an earlier affirmer ended the chain.

The per-finding fixes are all of the form "make checker X also look at field Y". That is whack-a-mole: it repairs the affirmers known today and leaves the next affirmer to reintroduce the class. The combinator is the defect that makes any single over-broad affirmer sufficient.

One checker already works around this individually, which is evidence the hazard is understood but not systematically addressed: `RefundChecker` deliberately squashes its delegate's `Secure` to `Abstain` (`refund.rs:68-73`) precisely because "this checker runs in a chain that stops at the first non-`Abstain` verdict, so any recipient with _some_ public onchain history (trivial for an attacker to pick) would otherwise make the engine answer `Secure` without Blocklist, CoW, ExcessiveApproval, or the primary-transfer check ever running." That comment is an exact statement of this finding, applied to one checker only.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The loop breaks at the first non-`Abstain` verdict and returns it unmodified | E2 | crates/sentinel-engine/src/engine/mod.rs:57-72 | Q1 |
| 2 | The crate's own test asserts a `Secure` beats a later `Insecure` — the semantics are intentional and pinned | E2 | crates/sentinel-engine/src/engine/mod.rs:104-120 | Q2 |
| 3 | `Verdict::Secure`'s documented meaning is the conjunction the loop does not compute | E2 | crates/sentinel-engine/src/engine/mod.rs:35-48 | Q3 |
| 4 | Six affirmers sit at positions 1,2,5,7,8,10, so most affirmations preempt most of the chain | E2 | crates/sentinel-engine/src/main.rs:57-73 | Q4 |
| 5 | A checker author already had to work around this by hand | E2 | crates/sentinel-engine/src/checkers/refund.rs:60-73 | Q5 |
| 6 | Charter §3.7 requires all applicable Article IV rules to be satisfied | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:439 | Q6 |

**Q1** `crates/sentinel-engine/src/engine/mod.rs:57-72`

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
        tracing::trace!(?verdict, "security check verdict");
        verdict
    }
```

**Q2** `crates/sentinel-engine/src/engine/mod.rs:104-120`

```rust
    #[tokio::test]
    async fn stops_at_the_first_non_abstaining_verdict {
        let engine = SentinelEngine::new(vec![
            Box::new(StubChecker(Verdict::Abstain)),
            Box::new(StubChecker(Verdict::Secure)),
            Box::new(StubChecker(Verdict::Insecure {
                rule: RuleId::R4_3ValueTarget,
            })),
        ]);

        assert_eq!(
            engine
                .security_check(SafeTransaction::default(), CheckContext::default())
                .await,
            Verdict::Secure
        );
    }
```

**Q3** `crates/sentinel-engine/src/engine/mod.rs:38-40`

```rust
pub enum Verdict {
    /// All configured checks consider the transaction secure.
    Secure,
```

**Q4** `crates/sentinel-engine/src/main.rs:57-73` — registration order is `CancellationChecker`, `EscapeHatchChecker`, `BaseChecker`, `BlocklistChecker`, `NestedSafeChecker`, `ExcessiveApprovalChecker`, `CowChecker`, `StakingChecker`, `RefundChecker`, `address_poisoning`. Of these, the six that can return `Secure` are at positions 1, 2, 5, 7, 8 and 10.

**Q5** `crates/sentinel-engine/src/checkers/refund.rs:60-67`

```rust
/// Only lets a denial through. A poisoning check's `Secure` verdict is, at
/// best, evidence about the one leg it was run against — never grounds to
/// affirm the whole transaction, which is what returning it here would do:
/// this checker runs in a chain that stops at the first non-[`Verdict::Abstain`]
/// verdict, so any recipient with *some* public onchain history (trivial for
/// an attacker to pick) would otherwise make the engine answer `Secure`
/// without Blocklist, CoW, ExcessiveApproval, or the primary-transfer check
/// ever running.
```

**Q6** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:439`

```text
- A transaction is secure only if it satisfies all applicable Article IV rules.
```

_Provenance note:_ this Charter line was read and transcribed verbatim earlier in this session, while the session-local Charter copy was present. That copy did not survive a VM restart mid-run and is no longer on disk, so it could not be re-opened at write time. The same read also produced the §2.5, §2.18, §3.7, §3.8, R-4.1, R-4.2 and R-4.5 quotes used in F-ENG-031, F-ENG-034, F-ENG-036, F-ENG-037, F-ENG-039 and F-ENG-042.

## Trigger

Any transaction matching an over-broad affirmer's predicate; the six instances are enumerated in F-ENG-030, F-ENG-033, F-ENG-034, F-ENG-035, F-ENG-036 and F-ENG-037, each with its own concrete vector. The minimal demonstration of the combinator itself needs no chain state and no RPC: the engine's own existing unit test (`engine/mod.rs:104-120`) already demonstrates that a `Secure` suppresses a following `Insecure`. A QA agent can turn that into a policy test by asserting the _desired_ semantics — that a chain containing any `Insecure` returns `Insecure` regardless of ordering — and watching it fail.

## Considered and rejected

- _"`Abstain` short-circuiting is the same defect."_ It is not. Stopping at the first `Insecure` is sound under §3.7, since one failed rule is sufficient for insecurity. Only the `Secure` case is unsound, because security is the conjunction. A correct combinator can still exit early on a denial — the asymmetry is the fix.
- _"Ordering already handles this."_ Two checkers were ordered deliberately to mitigate it — `NestedSafeChecker` runs after `BlocklistChecker` by design (`nested.rs:10-11`) and the RPC-backed pair runs last (`main.rs:66-70`). Ordering is a per-pair patch that must be re-derived whenever a checker is added, and it demonstrably did not cover `EscapeHatchChecker` ahead of `BlocklistChecker` (F-ENG-034). It is a mitigation, not a mechanism.
- _"This is a documentation bug in `Verdict::Secure`'s comment."_ The comment is wrong, but correcting it to describe first-wins would make the engine's documented semantics openly contradict Charter §3.7. The doc states the intended contract; the loop is what diverges from it.

## Remediation options

1. **Make affirmation conjunctive.** Run every checker; return `Insecure` if any denies, `Secure` only if at least one affirms and none denies, `Abstain` otherwise. Costs the early exit on the RPC-backed checkers — mitigable by keeping the deny-early exit and by ordering cheap checkers first, since only the affirm path must run to completion.
2. **Split the verdict type** so a checker cannot express "secure overall". Let each checker return an opinion scoped to the aspect it examined, and have the engine compose them. This is the type-level version of the workaround `RefundChecker` implements by hand at `refund.rs:68-73`, generalised so no future checker has to remember it.
3. **Minimum, if neither is affordable now:** an explicit registration-order invariant, asserted in a test, that no affirming checker may precede a checker that can deny on a field the affirmer does not read. This is what the current design relies on informally and does not enforce.

Tests to add: a policy test asserting that any chain containing a denying checker returns `Insecure` regardless of position (currently fails); and a registration test pinning the invariant in option 3. No code is committed.

## Trail

- Critic C-ENG-B: **drafted by the Critic**, promoted from R9's coverage log §6 (observations), where it was recorded below the filing bar and deferred to R8 on file-ownership grounds: "`Verdict::Secure`'s own doc comment says 'All configured checks consider the transaction secure' (`engine/mod.rs:39-40`), which the first-wins loop at `:62-69` makes false. Doc/code mismatch at the exact point where Charter §3.7 requires the opposite semantics; R8 owns the file, so it is recorded here rather than filed." R9's disposition is wrong on two counts. It is not a doc/code mismatch — it is a verdict-policy defect against §3.7, which under A15 is filable. And file ownership is not a reason to drop a defect that is the root cause of six findings R9 _did_ file; the defect belongs to the checker-chain semantics R9 was reasoning about throughout. Promoted at **High**: the instance impact (up to Critical) is carried by F-ENG-030/031/033, so filing this at Critical too would double-count; its own severity reflects that it is the structural enabler and that fixing it closes the class rather than one instance.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1: no `cargo` on this host, so `E1` is unreachable and the 90-100 band stays closed.

**Inspection: Reproduced by inspection.** I traced the combinator end to end without relying on the Critic's derivation: `engine/mod.rs:62-69` assigns each checker's verdict to `verdict` and `break`s on the first non-`Abstain`, returning it unmodified at `:71`; there is no aggregation, no second pass, and no caller-side recomposition (`api/mod.rs:55-59` serialises the value straight into the response body). The crate's own `stops_at_the_first_non_abstaining_verdict` (`engine/mod.rs:104-120`) asserts `[Abstain, Secure, Insecure{R4_3}] → Secure`, so the semantics are intentional and pinned by a test. This does **not** raise the finding into the 90-100 band, which needs `E1`.

**Certainty unchanged at 85%.** **Severity unchanged at High**, and I agree with the Critic's reason for not filing it Critical: the instance impact is carried by F-ENG-030/031/033 and filing it Critical too would double-count.

**PoC: `rust-audit/poc/F-ENG-044/`** — `append-to-src-engine-mod.rs` (four tests) plus a README giving the exact command, the fixture field by field, and what each pass/fail means. Two of the four tests are **expected to fail on unfixed code**; those are the regression tests.

The PoC's central test needs no transaction content at all: two stub checkers, `[Secure, Insecure]` and `[Insecure, Secure]`, asserted to return `Insecure` both ways. That is the whole finding, and it is the cheapest possible guard against the _next_ over-broad affirmer. The PoC also demonstrates the concrete version the brief asked for — a transaction a later checker **would** have denied being returned `secure` — using `main.rs`'s own registration order and a real later denier (`BlocklistChecker` at position 4, suppressed by `EscapeHatchChecker` at position 2), with a control test proving R-4.6 is implemented and is violated.

### Remediation check

- **Option 1 (conjunctive affirmation) — sound.** It is the only option that computes §3.7's conjunction rather than approximating it. The asymmetry that makes it affordable is worth stating in the fix itself: §3.7's second sentence ("If it fails any Article IV rule, it is insecure") means one denial is sufficient, so a fixed combinator may still `break` on `Insecure`; only the `Secure` path must run to completion. **One consequence the finding does not name:** today `AddressPoisoningChecker` (position 10) is reached only when everything above abstains, so making affirmation conjunctive **increases the `eth_getLogs` fan-out per request** and the CoW API call rate. That interacts directly with F-ENG-009 (the fan-out has no bound and no validation) and F-ENG-005 (no request deadline anywhere, and neither outbound client has a timeout). Fix those in the same change, or the latency regression will be attributed to the wrong commit and the fix will be reverted.
- **Option 2 (split the verdict type) — sound, stronger, and not sufficient alone.** Scoped opinions make "secure overall" unsayable by a checker, which is the right long-term shape and generalises `refund.rs:68-73`'s hand-written workaround. But the composition rule still has to be written; if it is written as "first scoped affirmation wins", nothing has changed. Option 2 is the type-level enforcement of option 1, not an alternative to it.
- **Option 3 (a registration-order invariant asserted in a test) — unsound as stated; do not ship it alone.** The stated invariant — "no affirming checker may precede a checker that can deny on a field the affirmer does not read" — is not mechanically checkable: nothing in the `Checker` trait (`checkers/mod.rs:24-34`) exposes which fields a checker reads or which rules it can deny under, so the test can only encode a hand-maintained table. That is the same informal reasoning that already succeeded for `NestedSafeChecker` (`nested.rs:10-11`) and **failed** for `EscapeHatchChecker` (F-ENG-034), so encoding it in a test does not make it more reliable — it makes it look more reliable. If it is used as a stop-gap, make it a hard assertion on the exact expected `checker.name` sequence, so it at least fails loudly when someone reorders the chain, and say in its doc comment that it does not establish the invariant it is named for.
- **Test hook: already exists, and the crate asserts the wrong property with it.** `SentinelEngine::new` takes `Vec<Box<dyn Checker>>` and the stub harness at `engine/mod.rs:79-90` is exactly what the fix needs. No new infrastructure. This matters because `AGENTS.md`'s "checkers get no unit tests, the `sentinel-test-vectors` corpus is the oracle" is not merely unavailable here (A8 FALSE) — for this finding it is the **wrong oracle in principle**: a corpus vector fixes one request and one response, so it can show that a particular transaction gets a wrong verdict, but it cannot state a property that quantifies over checker orderings. That property needs an in-process test, and the crate can already write one.
- **Where the fix belongs: the combinator** (`engine/mod.rs`), not the checkers and not the Charter-to-`RuleId` mapping. Every per-checker fix in F-ENG-030/033/034/035/037 leaves `engine/mod.rs:62-69` untouched, so the class survives all of them.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 85% -> **98%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under `rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-044/append-to-src-engine-mod.rs` appended to `crates/sentinel-engine/src/engine/mod.rs`, then `cargo test -p sentinel-engine poc_f_eng_044`. **It compiled unmodified on the first attempt** — no mechanical repair of any kind was needed. Full output: `rust-audit/poc/F-ENG-044/run-output.txt`.

### Verbatim result

```
running 4 tests
test engine::poc_f_eng_044::poc_f_eng_044_the_suppressed_checker_denies_the_same_transaction ... ok
test engine::poc_f_eng_044::poc_f_eng_044_ordering_must_not_decide_the_verdict ... FAILED
test engine::poc_f_eng_044::poc_f_eng_044_production_chain_returns_secure_today ... ok
test engine::poc_f_eng_044::poc_f_eng_044_a_denial_must_win_over_an_earlier_affirmation ... FAILED

---- ..._ordering_must_not_decide_the_verdict stdout ----
assertion `left == right` failed: a chain containing a denial must return that denial regardless of position
  left: Secure
 right: Insecure { rule: R4_6KnownMaliciousTarget }

---- ..._a_denial_must_win_over_an_earlier_affirmation stdout ----
assertion `left == right` failed: Charter §3.7 requires the conjunction of all applicable Article IV rules; \
engine/mod.rs:62-69 returns the first non-abstaining verdict instead
  left: Secure
 right: Insecure { rule: R4_6KnownMaliciousTarget }

test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 97 filtered out
```

Both expected-to-fail tests failed, and each failed **for the claimed reason**: the observed value is exactly `Verdict::Secure` where the Charter-correct value is `Insecure { rule: R4_6KnownMaliciousTarget }`, not a panic, a decode error, or an unrelated assertion.

### What this establishes, and what it does not

1. **The combinator defect is real and is the root cause.** Test (4) uses two `StubChecker`s and no transaction content at all: `[Secure, denial]` returns `Secure` while `[denial, Secure]` returns the denial. Verdict is a function of registration order. This is the property no `sentinel-test-vectors` corpus vector could ever state, since a corpus vector fixes one ordering.
2. **It is reachable through the production chain.** Test (1) instantiates `main.rs:57-73`'s registration order minus the two RPC-backed checkers (both of which run _after_ every checker in the fixture, so their absence cannot change this verdict) and gets `Secure` for a transaction whose `to` the operator has configured as an R-4.6 blocklist entry.
3. **The suppressed rule is implemented and does fire.** Test (2) passed: the same fixture handed directly to `BlocklistChecker` returns `Insecure { rule: R4_6KnownMaliciousTarget }`. The engine simply never asks it.

Residual uncertainty (why not 100%): the mapping from the executed behaviour to a Charter §3.7 _violation_ still rests on A7 (the Charter text as read) and A15. The behaviour itself is no longer in question.

## Real-world validation (Phase 8, RW-ENG)

### Scenario

Phase 5 proved the combinator with `StubChecker`s inside `SentinelEngine`. Phase 8 asked whether the **production chain, wired exactly as `main.rs` wires it**, returns `secure` for a transaction a later checker would deny — with the denier being one an operator deliberately configured.

Live deployment: Anvil 1.8.1 on `127.0.0.1:8545` (chain 31337), the real `sentinel-engine` binary on `127.0.0.1:5473`, config copied from the shipped sample with `rpc = "http://127.0.0.1:8545"` and **one operator-supplied blocklist entry** — the most explicit "deny this" signal the engine accepts:

```
[engine]
blocklist = ["0x90F79bf6EB2c4f870365E785982E1f101E93b906"]
address_poisoning_lookback_blocks = 50000
```

`BlocklistChecker` is the 4th checker; `EscapeHatchChecker` is the 2nd. Two requests were sent that are **identical except for the first four bytes of `data`**.

### Verbatim outcome

Control — a plain call to the blocklisted address:

```
{"verdict":"insecure","rule":"R-4.6"}
```

The same `to`, same `value`, same everything, with calldata that begins with the SafenetGuard `announceTransaction` selector `0x7b328c10`:

```
{"verdict":"secure"}
```

Checker trace for the second request:

```
cancellation -> Abstain
escape_hatch -> Secure
(final) -> Secure
```

`BlocklistChecker` never ran. An operator's explicit blocklist entry was defeated by a four-byte prefix, because a checker two positions earlier had no objection on its own narrow grounds.

The same shape was observed independently in every other scenario this agent ran. F-ENG-030's live trace:

```
cancellation -> Abstain   escape_hatch -> Abstain   base -> Abstain
blocklist -> Abstain      nested_safe -> Secure     (final) -> Secure
```

— five checkers (`excessive_approval`, `cow`, `staking`, `refund`, `address_poisoning`) never invoked on a transaction that drained 1000 ETH from a real Safe. F-ENG-031's ERC-20 and native refund legs were both affirmed the same way, with `RefundChecker` — the engine's only refund guard — sitting unreached at position 9.

### Verdict

**Reproduced end-to-end** with the production wiring, on the first attempt. The finding needed no synthetic checkers: the shipped chain, an operator's own blocklist, and a selector change are enough to turn an explicit `insecure` into `secure`.

The realism point worth recording is that the workaround `RefundChecker` applies by hand (`deny_or_abstain`) is the _only_ thing standing between the live engine and this class, and it protects one checker out of ten. Every other affirmer in the chain — positions 1, 2, 5, 7, 8 and 10 — short-circuits the rest.

Certainty **98% → 99%**. Severity **High**, unchanged: the Critical impacts this root cause produces are already carried by its separately-filed instances (F-ENG-030, F-ENG-031, F-ENG-033, F-ENG-034, F-ENG-035, F-ENG-036, F-ENG-037), and raising the root cause to Critical would double-count them. The blocklist demonstration above is a new instance of the class rather than a new impact.
