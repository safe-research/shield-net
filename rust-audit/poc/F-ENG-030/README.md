# PoC — F-ENG-030 (Critical, 86%)

**`NestedSafeChecker` rates any `execTransaction`-shaped call `secure` while ignoring `value`.**

> **Never compiled.** No Rust toolchain on the audit host (`rust-audit/state/baseline.md` §1). Identifiers
> were checked against `crates/sentinel-engine/src/checkers/nested.rs` (47 lines) and
> `contracts/bindings.rs` at commit `2893917`. The fixture is exact; the Rust may need mechanical fixes.

## Apply and run

```bash
cat rust-audit/poc/F-ENG-030/append-to-src-checkers-nested.rs \
  >> crates/sentinel-engine/src/checkers/nested.rs
cargo test -p sentinel-engine poc_f_eng_030
git checkout -- crates/sentinel-engine/src/checkers/nested.rs
```

`nested.rs` has no `#[cfg(test)]` block today, so this adds the file's first tests.

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._affirms_a_full_native_drain_today` | **passes** | fails | Pins the defect; delete with the fix. |
| `..._a_truncated_encoding_abstains` | passes | passes | **Control.** If this fails, the fixture never reached the predicate and test 1's `Secure` proves nothing. |
| `..._a_value_bearing_call_must_not_be_affirmed` | **fails** | passes | The finding. |
| `..._a_relayed_call_must_not_be_affirmed_either` | **fails** | passes | Catches a partial fix that only adds `value.is_zero`. |

## The fixture, written out

Under **A2** every field below is attacker-chosen.

| Field | Value | Why this value |
| --- | --- | --- |
| `chainId` | `0x1` | Never read by this checker; set for realism. |
| `safe` | `0x5aFE3855358E112B5647B952709E6165e1c1eEEe` | The victim. |
| `to` | `0x000000000000000000000000000000000000dEaD` | The attacker's **EOA**. `nested.rs:42-47` requires only `to != safe`. Nothing probes whether `to` is a Safe, implements `execTransaction`, or has code at all. A plain `CALL` with calldata to an EOA succeeds on-chain and transfers `value`. |
| `value` | `0x3635c9adc5dea00000` (1000 ETH) | The whole balance. **Never read.** |
| `data` | full ABI encoding of `execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)` with every argument zero/empty | Selector `0x6a761202`. **Must decode**, not merely carry the selector — `nested.rs:46` calls `abi_decode` and a truncated tail abstains. The decoded values are then discarded, so they are free. |
| `operation` | `0` (`Call`) | `nested.rs:43` requires it. |
| `safeTxGas`, `baseGas`, `gasPrice`, `gasToken`, `refundReceiver` | all zero | Test 1 isolates the `value` defect; test 4 flips `baseGas`/`gasPrice`/`refundReceiver` instead. |
| `nonce` | `0x2a` | Arbitrary. |

**Expected today:** `{"verdict":"secure"}`. **Charter-correct:** `{"verdict":"insecure","rule":"R-4.3"}`,
or at minimum `abstain`.

Full-chain walk (`main.rs:57-73`), for a corpus vector rather than a unit test:
Cancellation abstains (fields not all default) → EscapeHatch abstains (wrong selector) → Base abstains
(`to != safe` so `check_calls` returns `true`, `base.rs:91-93`) → Blocklist abstains (fresh address) →
**NestedSafe affirms, chain breaks.** Positions 6–10 never run.

One correction the Critic recorded and the PoC honours: with `nested.rs` removed from the chain the
engine answers **`abstain`, not `insecure`** — none of the suppressed checkers would have denied *this*
transaction either. The defect is that the engine converts "no opinion, cast no vote" into an
affirmative, bonded attestation. That is why test 3 asserts `!= Secure` rather than `== Insecure`: both
remediation directions are acceptable and the test must not prejudge which.

## Remediation check (QA-ENG)

- **Option 1 (`value.is_zero && gas_price.is_zero` before affirming) — sound but incomplete, and the
  finding says so.** It closes both drains in this file. It still affirms with zero evidence about `to`,
  so `gasToken`/`refundReceiver` aside, the checker keeps asserting "this is a nested Safe execution"
  about a call to an arbitrary EOA. Cheap enough to ship immediately; do not present it as the fix.
- **Option 2 (make the checker abstain-only) — sound, and the one I would take.** A nested
  `execTransaction` is a reason *not to deny*, not evidence of security. Under the current combinator an
  `Abstain` costs the sentinel its vote on genuine nested-Safe flows — but per `sentinel/src/service.rs:173-179`
  an `Unknown` outcome drops the request unanswered rather than voting wrongly, and those flows are
  unverified today in any case. This is the same move `refund.rs:68-73` already makes for the refund leg,
  which is precedent inside the crate.
- **Option 3 (probe `to` for Safe-ness at `context.block`) — sound but it changes the checker's class.**
  It makes `nested.rs` RPC-backed, so it must move behind `main.rs:66`'s RPC group, which inverts its
  relationship with `BlocklistChecker` and `CowChecker` — check that reordering against F-ENG-034 and
  F-ENG-035 before taking it. It also inherits the abstain-on-RPC-failure behaviour and the unbounded
  fan-out of F-ENG-009. Highest fidelity, highest cost.
- **A fix here does not close the class.** Under the current combinator (F-ENG-044) any *other*
  over-broad affirmer reproduces the same outcome. Options 1–3 all leave `engine/mod.rs:62-69` intact.
- **Missing test hook — none.** Unlike most engine findings this one needs no infrastructure at all: the
  checker is a pure function of `SafeTransaction`, with no `Provider`, no HTTP, and no `CheckContext`
  use. `AGENTS.md`'s "no unit tests for checkers, the corpus is the oracle" is what left a 47-line file
  with zero tests; the corpus (`sentinel-test-vectors`) is unavailable under A8, and for a checker this
  cheap to test in-process the corpus is the wrong oracle anyway. Recommend the crate relax that rule
  for pure checkers.
- **Where the fix belongs: the checker** (`nested.rs`), plus the combinator (F-ENG-044) for the class.
  Not the `RuleId` mapping — R-4.3 already exists and already means the right thing.
