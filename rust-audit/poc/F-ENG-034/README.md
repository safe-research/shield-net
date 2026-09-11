# PoC — F-ENG-034 (High, 85%)

**`EscapeHatchChecker` affirms the announcement shape for *any* `to` and runs ahead of the blocklist, so
an R-4.6 target is rated `secure`.**

> **Never compiled.** No Rust toolchain (`rust-audit/state/baseline.md` §1). `escape_hatch.rs` has **no
> test block at all** today, so this adds the file's first tests.

## Apply and run

```bash
cat rust-audit/poc/F-ENG-034/append-to-src-checkers-escape_hatch.rs \
  >> crates/sentinel-engine/src/checkers/escape_hatch.rs
cargo test -p sentinel-engine poc_f_eng_034
git checkout -- crates/sentinel-engine/src/checkers/escape_hatch.rs
```

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._affirms_any_to_today` | **passes** | fails | The checker in isolation. |
| `..._the_blocklist_never_runs_today` | **passes** | fails | The ordering, in `main.rs`'s own order. |
| `..._control_a_different_selector_is_denied` | passes | passes | **Control.** R-4.6 is implemented and the fixture violates it; only the selector separates (2) from (3). |
| `..._an_r4_6_target_must_not_be_affirmed` | **fails** | passes | The finding. Closed by option 1, 2 **or** 3. |
| `..._a_malformed_announcement_must_not_be_affirmed` | **fails** | passes | The argument-validation half. Closed **only** by option 4. |

## The fixture, written out

Config: `blocklist = ["0x1111111111111111111111111111111111111111"]`.

| Field | Value | Why |
| --- | --- | --- |
| `chainId` | `0x1` | Never read by this checker. |
| `safe` | `0x5aFE3855358E112B5647B952709E6165e1c1eEEe` | |
| `to` | `0x1111111111111111111111111111111111111111` | The blocklisted address. **`is_escape_hatch_call` places no constraint on `to` at all** (`escape_hatch.rs:52-61`). |
| `value` | `0x0` | Required (`escape_hatch.rs:53`). |
| `data` | 4-byte `announceTransaction((address,uint256,bytes,uint8,uint256,uint256,uint256,address,address))` selector **+ one arbitrary trailing byte** | The checker uses `starts_with` and never ABI-decodes, so this is accepted despite decoding as nothing. |
| `operation` | `0` (`Call`) | Required. |
| `gasPrice` | `0x0` | Required — a relayed call is deliberately excluded (`escape_hatch.rs:53`). |
| everything else | zero / `0x2a` nonce | |

**Actual:** `{"verdict":"secure"}` from position 2. **Charter-correct:** `{"verdict":"insecure","rule":"R-4.6"}`.

The on-chain rule this checker mirrors is strictly narrower: `SafenetGuard._isAutoAllowed` requires
`to == address(this)`, and Charter § 2.18 states the boundary in the same terms — "Calls to **the Safenet
Guard's** `announceTransaction` and `cancelAnnouncement` functions are auto-allowed by the Guard without
Sentinel review". The gap between the Guard's shape and this checker's shape **is** the set of
announcement-shaped calls that reach the sentinel and need a verdict.

## Remediation check (QA-ENG)

- **Option 1 (move `EscapeHatchChecker` after `BlocklistChecker`) — sound for the tested case, unsound as
  a general fix.** It is one line and it restores R-4.6, which is worth doing today. But it is exactly
  the per-pair patch F-ENG-044 identifies as a mitigation rather than a mechanism: it must be re-derived
  every time a checker is added, and it is *the same reasoning that already succeeded for
  `NestedSafeChecker`* (`nested.rs:10-11`) *and failed to be applied here*. Shipping option 1 alone
  records the same hazard in a second place instead of removing it.
- **Option 2 (affirm only when `to` is a registered SafenetGuard deployment) — sound, and it is the fix
  that makes the engine's rule match `_isAutoAllowed` exactly.** Cost: the engine must learn the guard
  address, either from config (cheap, another per-chain list) or from an RPC `getStorageAt` of the Safe's
  guard slot (accurate, but makes the checker RPC-backed and forces it behind `main.rs:66`'s group,
  which re-raises the ordering question option 1 was meant to settle).
- **Option 3 (return `Abstain` instead of `Secure`) — sound, and the best value for the effort.** The
  legitimate case (`to == guard`) is auto-allowed **on-chain by the Guard** and therefore never reaches
  the engine at all, so this checker's affirmation buys nothing that anyone consumes; abstaining costs
  nothing and removes an unevidenced affirmation. This is the argument `refund.rs:60-67` makes for the
  refund leg, applied here. **Recommended.**
- **Option 4 (ABI-decode the announcement argument) — sound, necessary, and insufficient alone.** It
  closes test 5 and nothing else: a *well-formed* announcement to a blocklisted address is still
  affirmed. Combine with 1, 2 or 3.
- **Do not combine option 1 with option 3 and call it done.** If the checker abstains (3), its position
  no longer matters and option 1 is redundant; if it still affirms, option 1 protects only against the
  blocklist and not against any other later denier. Pick 3 (+4), or 2 (+4).
- **Missing test hook — none.** `EscapeHatchChecker` is pure. This file having *zero* tests is a direct
  consequence of `AGENTS.md`'s "no unit tests for checkers, the `sentinel-test-vectors` corpus is the
  oracle" — and the corpus is unavailable (A8). Note that a corpus vector *can* express this finding
  (unlike F-ENG-032 or F-ENG-044), because it is a single request with a single wrong response; the
  reason it was not caught is simply that no such vector was written.
- **Where the fix belongs: the checker** (`escape_hatch.rs`) primarily, with the combinator (F-ENG-044)
  removing the class. The `RuleId` mapping is fine — R-4.6 exists and means the right thing.
