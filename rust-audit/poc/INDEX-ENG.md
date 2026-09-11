# sentinel-engine PoC index (QA-ENG)

Ten PoCs, in the priority order the engine findings should be fixed. **None has ever been compiled** — there is no Rust toolchain on the audit host (`rust-audit/state/baseline.md` §1), so `E1` was unreachable and no finding in this run may exceed 89% certainty.

## How they are packaged, and why

`sentinel-engine` is a **binary-only crate**: `src/main.rs`, no `src/lib.rs`, no `[lib]` in `Cargo.toml`. There is therefore **no library for a `tests/*.rs` integration test to import**, and no way to reach `crate::checkers::*` from outside. Every PoC is consequently an in-crate `#[cfg(test)]` module written to be **appended verbatim** to the file it exercises, as a _sibling_ of that file's existing test module (or as its first tests, where none exists). Each is applied with one `cat >>`, run with one `cargo test`, and reverted with one `git checkout --`. Every README carries all three commands.

This is worth knowing before anyone plans engine test infrastructure: adding `[lib]` (or a thin `src/lib.rs` re-exporting `checkers`, `engine` and `contracts`) would let these become ordinary integration tests and is probably the right change.

| # | Finding | Sev / Cert | PoC appends to | Tests | Expected to fail today |
| --- | --- | --- | --- | --- | --- |
| 1 | [F-ENG-044](F-ENG-044/) — first-non-abstain-wins combinator | High / 85 | `src/engine/mod.rs` | 4 | 2 |
| 2 | [F-ENG-030](F-ENG-030/) — nested `execTransaction` ignores `value` | **Critical** / 86 | `src/checkers/nested.rs` | 4 | 2 |
| 3 | [F-ENG-031](F-ENG-031/) — gas-refund leg never vetted | **Critical** / 85 | `src/checkers/staking.rs` | 6 | 1 (×3 legs) |
| 4 | [F-ENG-033](F-ENG-033/) — poisoning affirms from attacker-chosen history | **Critical** / 84 | `src/checkers/address_poisoning.rs` | 5 | 2 |
| 5 | [F-ENG-032](F-ENG-032/) — `RefundChecker` is dead (`chainId = 0`) | Medium / 88 | `src/checkers/refund.rs` | 4 | 2 |
| 6 | [F-ENG-002](F-ENG-002/) — every `setApprovalForAll` denied | High / 85 | `src/checkers/excessive_approval.rs` | 4 | 1 |
| 7 | [F-ENG-034](F-ENG-034/) — escape hatch affirms any `to`, ahead of blocklist | High / 85 | `src/checkers/escape_hatch.rs` | 5 | 2 |
| 8 | [F-ENG-035](F-ENG-035/) — blocklist sees only the top-level `to` | High / 80 | `src/checkers/blocklist.rs` | 3 | 1 (×4 positions) |
| 9 | [F-ENG-036](F-ENG-036/) — R-4.5 is an exact `U256::MAX` equality | High / 82 | `src/checkers/excessive_approval.rs` | 5 | 2 |
| 10 | [F-ENG-037](F-ENG-037/) — TWAP tolerance sized by attacker-chosen `n` | High / 84 | `src/checkers/cow.rs` | 5 | 2 |

F-ENG-002 and F-ENG-036 append to the same file as sibling modules and may both be applied.

## Run them all

```bash
cd <repo root>
for f in 044:engine/mod 030:checkers/nested 031:checkers/staking 033:checkers/address_poisoning \
         032:checkers/refund 002:checkers/excessive_approval 034:checkers/escape_hatch \
         035:checkers/blocklist 036:checkers/excessive_approval 037:checkers/cow; do
  id=${f%%:*}; path=${f##*:}
  cat rust-audit/poc/F-ENG-$id/append-to-src-$(echo "$path" | tr '/' '-').rs \
    >> crates/sentinel-engine/src/$path.rs
done
cargo test -p sentinel-engine poc_f_eng
git checkout -- crates/sentinel-engine/src   # revert everything
```

Expect **17 failures**. Each is a regression test asserting the Charter-correct verdict; the rest pin today's behaviour and are meant to be deleted or inverted alongside the fix.

## The three shapes to read the results with

1. **Wrong verdicts in the affirming direction** (F-ENG-030/031/032/033/034/035/036/037) — a malicious Safe transaction rated `secure`, or a denial that never runs. The fixtures are attacker-constructed under A2.
2. **Wrong verdicts in the denying direction** (F-ENG-002, and F-ENG-042 which has no PoC) — an _honest_ transaction denied. The fixtures are honest transactions, the opposite shape, and this direction costs the engine's operator money: `contracts/src/libraries/SentinelOracleRequests.sol:298-305` slashes a revealed vote on the losing side of a resolved dispute, with no good-faith exception. See F-ENG-042's `## QA (QA-ENG)` for the full record and the Charter §2.15 verification behind it.
3. **The root cause** (F-ENG-044) — six of the above are instances of one combinator defect. Fixing them individually leaves the next over-broad affirmer exploitable; F-ENG-044's fourth test is the guard that catches it.

## Recommended fix sequence

Several fixes are mutually blocking. The order that works:

1. **F-ENG-005 option 1** — outbound client timeouts. Precondition for everything that adds RPC.
2. **F-ENG-009 options 1+2** — bound and log the `eth_getLogs` fan-out.
3. **F-ENG-032 option 1/2 and F-ENG-043 option 2** — free, independent, no behaviour risk.
4. **F-ENG-033 option 2 and F-ENG-034 option 3** — make over-broad affirmers abstain. Closes several instances at once with no new RPC.
5. **F-ENG-044 option 1** — conjunctive affirmation. Only safe after 1 and 2.
6. **F-ENG-031 refund policy, F-ENG-035 option 1, F-ENG-036/037 amount policies** — the remaining per-checker work.
7. **F-ENG-001/003/004 doc fixes** — shippable at any point, at zero risk.
8. **F-ENG-039 behaviour** — blocked on a product decision (does `Charter:542`'s settings-change path exist?), not on engineering.

---

# Phase 5 execution results (V-ENG)

A toolchain now exists (cargo/rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`). **All ten PoCs were executed against commit `2893917`.** Each was appended to its target file, run with `cargo test -p sentinel-engine <filter>`, archived as `<id>/ran-source-*.rs` + `<id>/run-output.txt`, and the tracked file restored with `git checkout --`. No tracked file was left modified.

**Every PoC compiled on the first attempt with zero mechanical repairs.** The single exception is the one identifier QA-ENG itself flagged (`Q-ENG-A`, `Asserter::is_empty`), now settled: the method does not exist in `alloy-transport 2.0.5`, but `read_q.is_empty` does and asserts the same fact.

**All 17 expected-to-fail tests failed, and each failed for the reason the finding claims** — the observed value in every case is the wrong verdict itself (`Secure` where a denial is required, `Abstain` where a denial is required, `Insecure` where honest traffic must not be denied), never a panic, decode error or unrelated assertion. No finding was refuted or weakened by execution.

| # | Finding | Tests | Passed | Failed | Expected failures | Certainty before -> after |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | F-ENG-044 | 4 | 2 | 2 | 2 ✓ | 85 -> **98** |
| 2 | F-ENG-030 | 4 | 2 | 2 | 2 ✓ | 86 -> **97** |
| 3 | F-ENG-031 | 4 | 3 | 1 | 1 ✓ | 85 -> **96** |
| 4 | F-ENG-033 | 5 | 3 | 2 | 2 ✓ | 84 -> **96** |
| 5 | F-ENG-032 | 4 | 2 | 2 | 2 ✓ (with the `read_q` repair) | 88 -> **99** |
| 6 | F-ENG-002 | 4 | 3 | 1 | 1 ✓ | 85 -> **95** |
| 7 | F-ENG-034 | 5 | 3 | 2 | 2 ✓ | 85 -> **97** |
| 8 | F-ENG-035 | 3 | 2 | 1 | 1 ✓ (×4 positions, via test 1) | 80 -> **93** |
| 9 | F-ENG-036 | 5 | 3 | 2 | 2 ✓ | 82 -> **94** |
| 10 | F-ENG-037 | 5 | 3 | 2 | 2 ✓ | 84 -> **96** |

43 test functions, 26 passed, 17 failed. All ten findings are now `E1` / **Verified**.

Two notes for whoever fixes these:

- **A loop-based regression test reports only its first failing iteration.** F-ENG-031's and F-ENG-035's regression tests panic on iteration 1, so their output names only `native` and `erc20 recipient`. The _other_ legs/positions are proven by the paired pin test in the same module, which asserts today's verdict for every iteration and passed. Both finding files spell this out in a table; do not read the panic text as the whole result.
- **F-ENG-044 is the root cause and its test (4) is the guard.** Six of the other nine are instances of the same combinator defect. Fixing them one at a time leaves the next over-broad affirmer exploitable; `poc_f_eng_044_ordering_must_not_decide_the_verdict` catches that class for free, using only the stub harness the crate already has at `engine/mod.rs:79-90`.
