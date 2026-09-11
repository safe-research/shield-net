# Unresolved dependency questions — sentinel-engine (QA-ENG)

Commit `2893917`. Written by **QA-ENG** as a companion to `rust-audit/poc/UNRESOLVED-DEPENDENCY-QUESTIONS.md` (QA-XC's cross-crate list), kept as a separate file so the two can be merged without either overwriting the other. Numbering is `Q-ENG-*` and does not collide with QA-XC's `Q1…Q23`.

Everything below is a question a Rust toolchain would answer in minutes and that this run could not, because no `cargo` exists on the audit host and no dependency source is on disk (`rust-audit/state/baseline.md` §1-2; assumptions A9 and A6 both FALSE).

---

## Q-ENG-A. Does `alloy::transports::mock::Asserter` expose a queue-emptiness accessor?

> **SETTLED by V-ENG, Phase 5 . Answer: NO — there is no `Asserter::is_empty`.** Class `I` -> **`E1`**. Read from `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/alloy-transport-2.0.5/src/mock.rs:44-89`, the version this workspace resolves. `Asserter`'s complete inherent API is `new`, `push`, `push_success`, `push_failure`, `push_failure_msg`, `pop_response`, `read_q`, `write_q`. **However, the fact the assertion wanted is still reachable**, because `read_q` returns `impl Deref<Target = VecDeque<MockResponse>>`: writing `asserter.read_q.is_empty` compiles and asserts exactly the same thing. V-ENG ran both forms — the instructed deletion (`rust-audit/poc/F-ENG-032/run-output.txt`) and the `read_q` repair (`.../run-output-variant-read_q.txt`) — and the `read_q` form is the discriminating one: it failed with "the queued eth_getLogs response was never consumed: no lookup was issued", proving `RefundChecker` issues no RPC at all. Prefer `read_q.is_empty` over deleting the assertion. Everything below is QA-ENG's original text, kept for the record.

**Effort: 2 minutes.** **Class:** `I` (no dependency source on disk, assumption A6).

This is the **only identifier used anywhere in the ten engine PoCs that could not be verified against a checkout.** Everything else in `rust-audit/poc/F-ENG-{002,030,031,032,033,034,035,036,037,044}/` was read in `crates/sentinel-engine/src` or `crates/core/src` at commit `2893917`.

**Why it cannot be settled offline.** `Asserter` lives in `alloy-transport`, not in this repository. There is no `vendor/`, no `target/`, and no `*/cargo/registry` anywhere on this host, so the type's inherent methods cannot be read. `crates/core/src/index/events.rs` uses `Asserter::new`, `push_success` and `push_failure_msg` (e.g. `:786`, `:1036`), which confirms those three exist at the locked version — but nothing in this checkout exercises a queue-length or emptiness accessor, so its existence is genuinely unknown rather than merely unread.

**The check.** With a toolchain, either:

- `cargo doc -p alloy --open` and look at `alloy::transports::mock::Asserter`; or
- simply compile the PoC — `cat rust-audit/poc/F-ENG-032/append-to-src-checkers-refund.rs >> crates/sentinel-engine/src/checkers/refund.rs && cargo test -p sentinel-engine poc_f_eng_032`. A missing method is a compile error naming it exactly.

**Where it is used.** `rust-audit/poc/F-ENG-032/append-to-src-checkers-refund.rs`, in the test `poc_f_eng_032_an_established_refund_receiver_abstains_after_a_real_lookup`, as the **final assertion**. Its purpose is to distinguish "the checker abstained _after_ issuing a real `eth_getLogs` lookup" from "the checker abstained _without_ issuing one" — the two cases that are today indistinguishable on the wire (both emit `{"verdict":"abstain"}`), and the reason F-ENG-032 went unnoticed in the first place.

**If the method does not exist: delete exactly these three lines** from that test, leaving the verdict assertion above them intact:

```rust
        assert!(
            asserter.is_empty(),
            "the queued eth_getLogs response was never consumed: no lookup was issued"
        );
```

(The PoC carries a `NOTE:` comment immediately above them saying the same thing, so whoever runs it does not need this file to know what to do.)

**Nothing is lost if it goes.** The same fact is already established, more robustly, by the first test in that file — `poc_f_eng_032_abstains_without_any_rpc_call_today` — which leaves the `Asserter` queue **empty** and relies on `alloy`'s mock transport panicking if a request is ever issued. Merely reaching that test's assertion is therefore proof that no `eth_getLogs` call was made. That formulation depends on no accessor at all, and it is the one to keep if only one can be had.

---

## Standing note on all ten engine PoCs

`rust-audit/poc/F-ENG-{002,030,031,032,033,034,035,036,037,044}/` contain Rust that has **never been compiled or run**. Each README says so at the top and states, per test, what a pass and a fail mean.

They are written as in-crate `#[cfg(test)]` modules appended to the file they exercise, because `sentinel-engine` is a **binary-only crate** — `src/main.rs`, no `src/lib.rs`, no `[lib]` section in `crates/sentinel-engine/Cargo.toml` — and therefore has **no library for a `tests/*.rs` integration test to import**. This is not a dependency question, but it is the single most useful fact for anyone planning engine test infrastructure, and it was discovered while writing these PoCs: adding a thin `src/lib.rs` re-exporting `checkers`, `engine` and `contracts` would let all ten become ordinary integration tests.

## Cross-references into QA-XC's list

Two engine findings depend on questions QA-XC already owns; they are noted here only so this file is a complete picture of what blocks the engine set, and they should **not** be duplicated on merge:

- **Q4** (can an un-timed `reqwest` request hang indefinitely?) blocks `F-ENG-005` (80%) and `F-ENG-043` (74%). `cow.rs:234`'s `CowChecker::with_client` is the seam the fix uses either way.
- **Q10** (does axum's `JsonRejection` echo a fragment of a malformed body?) blocks `F-ENG-008` (80%).
