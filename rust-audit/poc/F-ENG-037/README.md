# PoC — F-ENG-037 (High, 84%)

**The CoW TWAP approval tolerance is sized by an attacker-chosen `n`, so a near-unlimited relayer
approval is rated `secure`.**

> **Never compiled.** No Rust toolchain (`rust-audit/state/baseline.md` §1).

## Apply and run

```bash
cat rust-audit/poc/F-ENG-037/append-to-src-checkers-cow.rs \
  >> crates/sentinel-engine/src/checkers/cow.rs
cargo test -p sentinel-engine poc_f_eng_037
git checkout -- crates/sentinel-engine/src/checkers/cow.rs
```

**No HTTP is issued.** `CowChecker::new` builds a `reqwest::Client`, but the presignature path requires
a `setPreSignature` call in the batch (`cow.rs:283-293`) and this batch has none, so `check_twap_batch`
decides everything from calldata. The module is appended as a *sibling* of `cow.rs`'s `mod tests`, so it
cannot reach that module's private helpers (`multisend`, `pack`, `twap_create_data`) — they are
re-declared. File-level constants (`GP_V2_VAULT_RELAYER`, `TWAP_HANDLER`, …) do come through `use super::*`.

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._control_a_real_order_behaves_as_documented` | passes | **must still pass** | Restates `cow.rs:879-943`: a 3-part order selling 30, approved for exactly 30, is `Secure`; approved for 1000 it is `insecure R-4.5`. A fix that breaks this has broken genuine TWAP support. |
| `..._the_tolerance_is_sized_by_n` | **passes** | fails | The arithmetic alone, no calldata: `max_approval_for_twap_total(0, U256::MAX) == 2^256 - 2`. Clearest single statement of the defect. |
| `..._affirms_a_near_unlimited_relayer_approval_today` | **passes** | fails | Pins both variants. |
| `..._a_degenerate_order_must_not_size_the_tolerance` | **fails** | passes | The finding. Closed by option 1, 2 or 3. |
| `..._a_large_part_count_must_not_size_the_tolerance` | **fails** | passes | Closed by option 1 or 3 — **not** by option 2. |

## The fixtures, written out

Outer transaction (both variants): `chainId 0x1` (must be 1, 100 or 42161 — `cow.rs:106`),
`safe 0x5aFE…eEEe`, `to 0x218543288004CD07832472D464648173c77D7eB7` (a canonical MultiSend,
`multi_send.rs:28-32`), `value 0x0`, `operation 1` (`DelegateCall` — required, `multi_send.rs:142`), all
gas/refund fields zero, `nonce 0x2a`.

`data` = `multiSend(<two packed Call entries>)`, layout
`uint8 operation | address to | uint256 value | uint256 dataLength | bytes data`:

1. `to = 0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` (the sell token), `value = 0`,
   `data = approve(0xC92E8bdf79f0507f65a392b0ab4667716BFE0110, <amount>)` — `GPv2VaultRelayer`.
2. `to = 0xfdaFc9d1902f4e0b84f65F49f244b32b31013b74` (ComposableCoW), `value = 0`,
   `data = createWithContext(params, factory, data, dispatch)` with
   - `params.handler = 0x6cF1e9cA41f7611dEf408122793c358a3d11E5a5` (**enforced**, `cow.rs:536`)
   - `params.salt = 0x00…00`
   - `params.staticInput = abi.encode(TwapData{ sellToken: <the same token>, buyToken: WETH,
     receiver: <the Safe>, partSellAmount: P, minPartLimit: 0, t0: 0, n: N, t: 0, span: 0,
     appData: 0x00…00 })`
   - `factory = 0x52eD56Da04309Aca4c3FECC595298d80C2f16BAc` (`CurrentBlockTimestampFactory`, **enforced**)
   - `data = 0x`, `dispatch = true`

| Variant | `P` (`partSellAmount`) | `N` (`n`) | `total = P × N` | ceiling = `total + (N-1)` | approved | verdict today |
| --- | --- | --- | --- | --- | --- | --- |
| **degenerate** | `0` | `2^256 - 1` | `0` (`checked_mul` does not overflow on a zero operand) | `2^256 - 2` | `2^256 - 2` | **`secure`** |
| **plausible** | `1` | `2^128` | `2^128` | `2^129 - 1` | `2^129 - 1` | **`secure`** |
| **control** | `10` | `3` | `30` | `32` | `30` → `secure`; `1000` → `insecure R-4.5` | as documented |

The plausible variant matters more than the degenerate one for a corpus: every field reads like a real
TWAP order, and `2^129 - 1` is functionally unlimited for any 18-decimal token.

Chain walk: Base allows (both sub-calls target other contracts, `base.rs:210-212`) → Blocklist abstains →
Nested abstains (outer is a delegatecall) → **ExcessiveApproval abstains** (`2^256-2 != U256::MAX` —
F-ENG-036) → **Cow returns `Secure`** (`cow.rs:382`): `check_dangling_approval` abstains because a TWAP
create is present (`cow.rs:392-396`), `check_presignature_batch` abstains, `check_twap_batch` computes
`total = 0`, `max = 2^256 - 2`, and `approved > max` is false.

**Charter-correct:** `insecure R-4.5` — § 2.5, an approval that "materially exceeds what is plausibly
needed for the stated interaction", where the stated interaction is an order selling zero tokens.

## Remediation check (QA-ENG)

- **Option 1 (`min(n - 1, SOME_ABSOLUTE_CAP)`, or reject `n` above a plausible part count) — sound, and
  the smallest change that closes both variants.** The rounding argument the tolerance rests on
  (`cow.rs:544-551`: the Safe app computes `partSellAmount = floor(desired_total / n)`, so up to `n - 1`
  units are truncated away) is *only* valid for `n` in the range the Safe app actually produces. Capping
  `n` restores the argument's own precondition rather than working around it. Pick the cap from the Safe
  app's UI limit and say so in the comment.
- **Option 2 (require `partSellAmount > 0` and `n > 0`) — sound but insufficient, and the PoC proves
  it.** Test 5 uses `partSellAmount = 1`, `n = 2^128`: both non-zero, internally consistent, and still
  affirmed. Do not close the finding on option 2 alone.
- **Option 3 (`approved >= total` and `approved - total < n` and `n <= MAX_PARTS`) — sound, and it is
  the most faithful.** It preserves the rounding argument exactly and fails closed on everything else.
  One caution the finding does not raise: adding `approved >= total` **changes behaviour for honest
  under-approvals**, which `cow.rs:348-350` currently and deliberately treats as a trade-soundness
  concern rather than a security one ("An approval *smaller* than that total … doesn't affect this
  verdict either way"). Under option 3 an under-approving but otherwise genuine batch stops being
  affirmed. That is defensible, but it is a deliberate policy change and must be called out, not slipped
  in — and it is a change in the *denying* direction against honest traffic, the direction that exposes
  the sentinel's bond (see F-ENG-042's QA section). Prefer `approved - total < n` **without** the
  `approved >= total` clause, i.e. keep under-approval unopinionated.
- **Option 4 (fix F-ENG-036's `U256::MAX`-only test) — sound as a defence in depth, not a fix here.** It
  would catch the degenerate variant (`2^256-2`) at position 6 before CoW ever runs, but not the
  plausible variant (`2^129 - 1`), which is far below any max-adjacent threshold.
- **Missing test hook — none; the hooks are unusually good here.** `CowChecker::with_order_api`
  (`cow.rs:242`) already exists for the network-dependent path, and the TWAP path needs nothing at all.
  `cow.rs` has the crate's largest test suite (`cow.rs:641-1407`); it simply covers only `n = 3`. This is
  the one engine finding where "the corpus is the oracle" is a reasonable position — a corpus vector
  *can* express it — and where the unit-test hook is nevertheless plainly cheaper.
- **Where the fix belongs: the checker** (`cow.rs`, in `check_twap_batch` / `max_approval_for_twap_total`).
  The `RuleId` mapping is fine.
