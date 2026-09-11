# PoC — F-ENG-044 (High, 85%)

**The engine's first-non-abstain-wins combinator cannot implement Charter §3.7.**

> **This code has never been compiled.** There is no Rust toolchain on the host this audit ran on (`rust-audit/state/baseline.md` §1: no `cargo`, `rustc`, `rustup`, `forge`, `anvil`, `just`). Every identifier below was checked by reading `crates/sentinel-engine/src` at commit `2893917`, but expect to need mechanical fixes (an import, a type ascription). The _fixture_ — the transaction fields — is the part that matters and is exact.

## Apply and run

```bash
# from the repository root
cat rust-audit/poc/F-ENG-044/append-to-src-engine-mod.rs \
  >> crates/sentinel-engine/src/engine/mod.rs
cargo test -p sentinel-engine poc_f_eng_044
git checkout -- crates/sentinel-engine/src/engine/mod.rs   # revert
```

`sentinel-engine` is a **binary-only** crate (`src/main.rs`, no `src/lib.rs`, `Cargo.toml` has no `[lib]`), so there is no library for a `tests/*.rs` integration test to import. Every PoC in this audit is therefore an in-crate `#[cfg(test)]` module appended to the file it exercises. That is also why it reaches private items such as `is_nested_exec_transaction` without any visibility change.

## What a run means

| Test | On unfixed code | On fixed code | Reading |
| --- | --- | --- | --- |
| `..._production_chain_returns_secure_today` | **passes** | fails | Pins the defect. Delete or invert it as part of the fix. |
| `..._the_suppressed_checker_denies_the_same_transaction` | passes | passes | Control: R-4.6 _is_ implemented and _is_ violated. If this fails, the fixture is wrong, not the engine. |
| `..._a_denial_must_win_over_an_earlier_affirmation` | **fails** | passes | The finding, on the real chain. Failure text: `left: Secure`, `right: Insecure { rule: R4_6KnownMaliciousTarget }`. |
| `..._ordering_must_not_decide_the_verdict` | **fails** | passes | The combinator alone, two stubs, no transaction content. This is the regression test to keep. |

A run in which tests 3 and 4 _pass_ on unfixed code would refute the finding. Nothing else in the table can refute it.

## The fixture, written out

Under assumption **A2 the whole Safe transaction is attacker-controlled.** Field by field:

| Field | Value | Why |
| --- | --- | --- |
| `chainId` | `0x1` | Mainnet, so `CowChecker` and `StakingChecker` are live rather than short-circuiting on an unsupported chain. |
| `safe` | `0x5aFE3855358E112B5647B952709E6165e1c1eEEe` | The victim. |
| `to` | `0x1111111111111111111111111111111111111111` | Configured in `[engine] blocklist`. This is the operator's only expression of R-4.6. |
| `value` | `0x0` | Must be zero, or `EscapeHatchChecker` refuses to affirm (`escape_hatch.rs:53`). |
| `data` | `0x` + `announceTransaction` selector + `00` | **Bare selector plus one junk byte.** `is_escape_hatch_call` matches with `starts_with` and never ABI-decodes (`escape_hatch.rs:56-60`), so an _invalid_ encoding still affirms — which is the sharpest form of the point. |
| `operation` | `0` (`Call`) | `DelegateCall` is rejected by `escape_hatch.rs:53`. |
| `safeTxGas` | `0x0` | — |
| `baseGas` | `0x0` | — |
| `gasPrice` | `0x0` | Must be zero, or `EscapeHatchChecker` abstains (`escape_hatch.rs:53`). |
| `gasToken` | `0x0…0` | — |
| `refundReceiver` | `0x0…0` | — |
| `nonce` | `0x2a` | Arbitrary. |
| `block` (request field, not the transaction) | `22020096` | Arbitrary; no checker in this chain reads it. |

Config: `blocklist = ["0x1111111111111111111111111111111111111111"]`.

Chain walk, position by position (`main.rs:57-73`):

1. `CancellationChecker` — abstains (`data` and `to` are not the all-default cancellation shape).
2. `EscapeHatchChecker` — **`Secure`**. Chain breaks here.
3. `BaseChecker` — never runs. Would abstain (`to != safe`, plain `Call`, `base.rs:91-93`).
4. `BlocklistChecker` — **never runs. Would deny `insecure R-4.6`.** 5–8. `NestedSafe`, `ExcessiveApproval`, `Cow`, `Staking` — never run; all would abstain. 9–10. `Refund`, `AddressPoisoning` — never run; omitted from the PoC chain because they need a live `Provider`, and they sit behind every checker above so they cannot alter this verdict.

`CowChecker::new` builds a `reqwest::Client` but issues **no** HTTP request for this fixture: the presignature path needs an exactly-2-call batch containing a `setPreSignature` call (`cow.rs:283-293`), and this is a single call. The test is hermetic.

## Why fixing the three Criticals individually is not enough

State this plainly in any remediation plan:

**F-ENG-030, F-ENG-031, F-ENG-033, F-ENG-034, F-ENG-035 and F-ENG-037 are six instances of one shape** — a checker affirms on a narrow structural predicate, and `engine/mod.rs:62-69` promotes that affirmation to a verdict about the whole transaction. Each per-finding remediation is of the form "make checker X also read field Y". Applying all six leaves the combinator exactly as it is, so **the seventh affirmer reintroduces the class**, and it will be introduced by someone who has read neither this audit nor `refund.rs:60-67` — the one place in the crate where a checker author noticed the hazard and worked around it by hand, for one checker only.

Test 4 above is the cheapest possible guard: it needs no transaction content, no chain, and no RPC, and it fails for _any_ future checker that affirms too broadly, not only the six found here.

The asymmetry that makes the fix affordable: **early exit on a denial stays sound.** §3.7's second sentence — "If it fails any Article IV rule, it is insecure" — means one denial is sufficient, so a fixed combinator may still `break` on `Insecure`. Only the `Secure` path has to run to completion.

## Remediation check (QA-ENG)

- **Option 1 (make affirmation conjunctive) — sound.** It closes the mechanism directly and is the only option that satisfies §3.7's conjunction rather than approximating it. Cost is real but bounded: the affirm path must run all ten checkers, which under the current chain means the two RPC-backed checkers execute on every transaction some earlier checker affirms. Note one consequence the finding does not: today `AddressPoisoningChecker` is reached only when everything above abstains, so making affirmation conjunctive **increases `eth_getLogs` fan-out per request** and interacts with F-ENG-009 (no bound on that fan-out) and F-ENG-005 (no request deadline anywhere). Fix those together or the latency regression will be blamed on the wrong change.
- **Option 2 (split the verdict type) — sound and stronger, but not sufficient alone.** Scoped opinions make it impossible for a checker to _say_ "secure overall", which is the right long-term shape. But the composition rule still has to be written, and if it is written as "first scoped affirmation wins" nothing has changed. Option 2 is the type-level enforcement of option 1, not an alternative to it.
- **Option 3 (a registration-order invariant asserted in a test) — unsound as stated; do not ship it alone.** The invariant "no affirming checker may precede a checker that can deny on a field the affirmer does not read" is not mechanically checkable in Rust: nothing in the `Checker` trait exposes which fields a checker reads, so the test can only encode a hand-maintained table, which is the same informal reasoning that already failed for `EscapeHatchChecker` (F-ENG-034). If it is used as a stop-gap, make it a hard-coded assertion on the exact expected checker order (`checker.name` sequence), which at least fails loudly when someone reorders the chain, and say in the test's doc comment that it does not establish the invariant it is named for.
- **Missing test hook.** Per `AGENTS.md` the checkers deliberately have no unit tests and the external `sentinel-test-vectors` corpus is the oracle. That corpus is unavailable here (A8 FALSE) and, more importantly, **it cannot express this finding at all**: a corpus vector fixes one request and one response, so it can show that _this_ transaction gets the wrong verdict, but not that the _combinator_ is wrong for all orderings. The hook that has to exist is the one test 4 uses — a `SentinelEngine` constructed from stub checkers — and it already does exist (`engine/mod.rs:79-90`). No new infrastructure is needed; the crate simply asserts the wrong property with it.
- **Where the fix belongs: the combinator.** Not the checkers, not the `RuleId` mapping.
