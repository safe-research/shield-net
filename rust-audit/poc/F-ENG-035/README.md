# PoC — F-ENG-035 (High, 80%)

**The blocklist is applied only to the top-level `to`, so R-4.6 misses token recipients, approval spenders, batch sub-calls and the refund receiver.**

> **Never compiled.** No Rust toolchain (`rust-audit/state/baseline.md` §1).

## Apply and run

```bash
cat rust-audit/poc/F-ENG-035/append-to-src-checkers-blocklist.rs \
  >> crates/sentinel-engine/src/checkers/blocklist.rs
cargo test -p sentinel-engine poc_f_eng_035
git checkout -- crates/sentinel-engine/src/checkers/blocklist.rs
```

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._control_the_top_level_to_is_denied` | passes | **must still pass** | The one position covered today. |
| `..._every_other_position_abstains_today` | **passes** | fails | Pins all four gaps at once. |
| `..._every_address_the_transaction_reaches_must_be_checked` | **fails** on `erc20 recipient` | passes | The finding. Iterate to see which positions a partial fix closed. |

Config for all: `blocklist = ["0xBadBadBadBadBadBadBadBadBadBadBadBadBad0"]`.

## The four fixtures, written out

All share `chainId 0x1`, `safe 0x5aFE…eEEe`, `operation 0` unless noted, `nonce 0x2a`.

|  | `to` | `operation` | `value` | `data` | Flagged address sits in |
| --- | --- | --- | --- | --- | --- |
| **A** | `0xA0b8…eB48` (USDC) | `0` | `0x0` | `transfer(0xBadBad…Bad0, 1000000000)` | the ERC-20 **recipient** |
| **B** | `0xA0b8…eB48` (USDC) | `0` | `0x0` | `approve(0xBadBad…Bad0, 1000000000)` | the approval **spender** |
| **C** | `0x40A2aCCbd92BCA938b02010E17A5b8929b49130D` (a canonical **call-only** MultiSend, `multi_send.rs:53-57`) | `1` (`DelegateCall`) | `0x0` | `multiSend(<one packed entry: op 0, to = 0xBadBad…Bad0, value 1, dataLength 0>)` | a **sub-call destination** |
| **D** | `0xA0b8…eB48` | `0` | `0x0` | `0x` — with `safeTxGas 0x186a0`, `baseGas 0x5208`, `gasPrice 0x1`, `gasToken` USDC, `refundReceiver 0xBadBad…Bad0` | the **refund receiver** |

Packed MultiSend entry layout (`multi_send.rs:83-89`): `uint8 operation | address to | uint256 value | uint256 dataLength | bytes data`.

**Actual for all four:** `abstain`. **Charter-correct:** `insecure R-4.6`. § 2.4 defines the target address as "the address that receives value or tokens, is granted approvals or permissions, or otherwise receives economically relevant effects from the transaction; **not merely an intermediate contract address called by the Safe transaction**" — which is a description of `transaction.to` in cases A–C.

For C, note `BaseChecker` also allows it: `check_calls` returns `true` for a sub-call whose `to != safe` (`base.rs:91-93`).

## The variant this PoC does not cover, and why

**Trigger B of the finding — the false `secure`** — is the worst case: when the flagged address _also_ has genuine prior history with the Safe (the normal situation for a counterparty flagged _after_ it was compromised), `AddressPoisoningChecker` at position 10 finds an `ExactMatch` and the engine answers `secure`. The operator's explicit "this address is malicious" configuration is then not merely ignored but **contradicted**.

That needs a mixed chain plus a mocked provider, so it is not in this file. The harness for it is in `rust-audit/poc/F-ENG-033/` — reuse `checker(&asserter)` and `transfer_log(...)` there, seed a `Transfer(safe -> FLAGGED, 1000)` log, and assert the engine (with `BlocklistChecker` and `AddressPoisoningChecker` both registered) does not return `Secure`. Write it as a fifth test in that file rather than duplicating the mock plumbing here.

## Remediation check (QA-ENG)

- **Option 1 (check every address the transaction reaches) — sound, and the right shape.** The ingredients already exist: `sub_transactions(tx)` (`multi_send.rs:165-169`) for batch destinations, and `decode_target_effects(tx)` (`contracts/target_effects.rs:44-51`) which already returns a `TargetEffect { recipient, kind }` for every ERC-20/721/1155 recipient, approval spender and native transfer, **and already recurses through MultiSend**. So option 1 is close to `decode_target_effects` plus `gas_token` and `refund_receiver`, not a new decoder. Two cautions: (a) `decode_target_effects` recurses through MultiSend with **no depth limit** (F-ENG-006), and today the only thing keeping attacker-chosen depth away from it is checker ordering — moving it into `BlocklistChecker` at position 4 changes that, so fix F-ENG-006 in the same change; (b) it does not decode the inner `to` of a nested `execTransaction` payload, so that position stays uncovered and must be documented as such.
- **Option 2 (also give the blocklist precedence over affirmations) — sound and necessary.** Option 1 alone does not close Trigger B: it makes `BlocklistChecker` _able_ to see the flagged recipient, but `EscapeHatchChecker` (F-ENG-034) can still affirm ahead of it, and once affirmations are conjunctive (F-ENG-044) the denial wins anyway. The "run it before every affirming checker" half is a per-pair patch; the "give denials priority in `security_check`" half is F-ENG-044 option 1 and is the durable form. Take the latter.
- **Option 3 (ERC-20 recipients and MultiSend sub-calls only, documenting the rest as out of scope) — sound as a staged first step**, and it closes A and C, the two positions with the clearest § 2.4 standing. If it is taken, close D too: `refund_receiver` is a one-line comparison with no decoding at all, so leaving it out is not a scope decision but an omission.
- **Missing test hook — none.** `BlocklistChecker` is pure; the three existing tests (`blocklist.rs:50-88`) all exercise the top-level `to`, which is precisely the position that works. Trigger B needs `Provider::mocked`, which exists.
- **Where the fix belongs: the checker, plus the combinator for Trigger B.** The `RuleId` mapping is fine.
