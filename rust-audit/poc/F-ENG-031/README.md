# PoC — F-ENG-031 (Critical, 85%)

**The gas-refund leg is never vetted on any transaction an affirming checker approves.**

> **Never compiled.** No Rust toolchain on the audit host (`rust-audit/state/baseline.md` §1).

## Apply and run

```bash
cat rust-audit/poc/F-ENG-031/append-to-src-checkers-staking.rs \
  >> crates/sentinel-engine/src/checkers/staking.rs
cargo test -p sentinel-engine poc_f_eng_031
git checkout -- crates/sentinel-engine/src/checkers/staking.rs
```

`StakingChecker` was chosen over the other three unguarded affirmers because it is **pure**: no
`Provider`, no HTTP, no `CheckContext`. The PoC is fully deterministic and needs no fixture chain, no
Anvil and no mock.

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._control_an_unrelayed_claim_is_affirmed` | passes | **must still pass** | The honest case. A fix that breaks this has over-corrected: it now abstains on every staking claim. |
| `..._control_a_claim_for_another_account_is_denied` | passes | passes | Proves the affirmation path is live and the fixture reaches it. |
| `..._affirms_every_hostile_refund_leg_today` | **passes** | fails | Pins the defect; delete with the fix. |
| `..._a_hostile_refund_leg_must_not_be_affirmed` | **fails** (first on `native`) | passes | The finding. |

The signal is the **delta**: the three hostile transactions differ from the control in nothing but
`baseGas`, `gasPrice`, `gasToken` and `refundReceiver`, and today every one of the four returns the
identical `Secure`.

## The fixtures, written out

Common to all four (attacker-chosen under **A2**):

| Field | Value |
| --- | --- |
| `chainId` | `0x1` — required; `staking.rs:89` abstains on any other chain. |
| `safe` | `0x5aFE3855358E112B5647B952709E6165e1c1eEEe` |
| `to` | `0xe5139fc0fb8eae81e30d8a85c22e88c6757120f2` — `REWARDS_DISTRIBUTOR` (`staking.rs:74`). |
| `value` | `0x0` — required; `claim_account` rejects a non-zero value (`staking.rs:157`). |
| `data` | `claim(account = <the Safe>, cumulativeAmount = 0, expectedMerkleRoot = 0x00…00, merkleProof = [])` |
| `operation` | `0` (`Call`) — required. |
| `safeTxGas` | `0x0` |
| `nonce` | `0x2a` |
| `block` | any; never read. |

`account` **must** equal `safe`. One field over, `staking.rs:99-103` denies with `R-4.3` — which is
control test 0b, and which is why this makes a good paired corpus vector.

Then, varying only the refund leg:

| | `baseGas` | `gasPrice` | `gasToken` | `refundReceiver` | Refund paid |
| --- | --- | --- | --- | --- | --- |
| **Control** | `0x0` | `0x0` | `0x0…0` | `0x0…0` | none — Safe.sol only calls `handlePayment` `if (gasPrice > 0)` |
| **A · native** | `0xe8d4a51000` (1e12) | `0x3b9aca00` (1 gwei) | `0x0…0` | attacker | `(gasUsed + 1e12) × min(1 gwei, tx.gasprice)` ≈ **1000 ETH** |
| **B · ERC-20** | `0xe8d4a51000` (1e12) | `0xde0b6b3a7640000` (1e18) | `0xA0b8…eB48` (USDC) | attacker | `(gasUsed + 1e12) × 1e18` USDC units — **no `tx.gasprice` cap on the ERC-20 path** |
| **F · tx.origin** | `0xe8d4a51000` | `0x3b9aca00` | `0x0…0` | `0x0…0` | same as A, paid to `tx.origin` — an address the engine cannot learn before execution |

**Expected today for A, B and F:** `{"verdict":"secure"}`. **Charter-correct:** not `secure`.

### Trigger B is the one that is not already written down

`refund.rs` carries two `TODO(follow-up)` comments. The first (`:83-89`) covers **only** `gasToken == 0`;
the second (`:93-96`) covers **only** `refundReceiver == 0`. Neither covers the ERC-20 path, where
`refund_transfer` *does* build a synthetic transfer — i.e. the team's stated design is that this path
**is** checked. It is not, for two independent reasons:

1. the delegated check is dead, because the synthetic transfer carries `chainId = 0` (F-ENG-032); and
2. `StakingChecker` (position 8) affirms before `RefundChecker` (position 9) ever runs, so even a
   repaired `RefundChecker` would not see this transaction.

And the ERC-20 path has no `tx.gasprice` cap, making it the **highest-impact sub-case of the finding** and
the one carrying no `known` mitigation. Report it that way.

### The other three affirmers

`NestedSafeChecker`, `CowChecker` and `AddressPoisoningChecker` have the same hole; `nested.rs`'s slice is
test 4 in `rust-audit/poc/F-ENG-030/`. `CancellationChecker` and `EscapeHatchChecker` are the only two
immune affirmers — the first compares the whole struct against an all-default value, the second requires
`gas_price.is_zero` (`escape_hatch.rs:53`), which is the guard the other four are missing.

## Remediation check (QA-ENG)

- **Option 1 (a chain-wide `gas_price != 0` → `Abstain` pre-gate at position 1) — sound, and the only
  option that is safe to ship today.** It restores the guard PR #876 removed. Cost is exactly as stated:
  the sentinel casts no vote on any genuinely relayed transaction. Under
  `sentinel/src/service.rs:173-179` an `Unknown` outcome drops the request unanswered rather than voting,
  so the cost is lost participation, not a wrong vote. One thing the finding does not say: this also
  makes `RefundChecker` **unreachable**, since it returns `None` unless `gas_price != 0` — so if option
  1 ships, F-ENG-032's fix stops being observable and must not be judged "no longer needed".
- **Option 2 (move a repaired `RefundChecker` ahead of every affirmer and make it deny rather than
  abstain when the leg cannot be vetted) — sound in direction, unsound as literally specified.** Making
  it *deny* an unvettable leg means denying every transaction with a native refund and every transaction
  with `refundReceiver == 0`, i.e. denying honest relayed traffic — a wrong vote in the denying
  direction, the F-ENG-002/F-ENG-042 failure mode, and now known to expose the denier's bond (see
  F-ENG-042's QA section). The correct form is **abstain, not deny**, promoted ahead of the affirmers:
  that suppresses the affirmation without manufacturing a denial. Deny only where the leg *is* vettable
  and fails (a poisoned ERC-20 receiver).
- **Option 3 (run every denier before considering any affirmation) — sound, and it is F-ENG-044 option 1
  under another name.** It fixes this finding, F-ENG-034 and F-ENG-035 at once. Largest change; the one
  that matches §3.7 directly. Note it does **not** fix this finding on its own: no checker in the chain
  *denies* on a hostile refund leg today, so running all deniers first still yields `secure` unless a
  refund-aware denier exists. Options 3 **and** a refund policy are both required.
- **Option 4 (`gas_price.is_zero` inside each of the four affirming predicates) — sound, minimal, and
  the smallest diff that makes this PoC's regression test pass.** It duplicates the same condition in
  four files, so it decays the moment a fifth affirmer is written — which is precisely F-ENG-044.
- **A cap is not a substitute.** Any "cap the refund amount" variant must cap
  `gasPrice × (safeTxGas + baseGas)`, not `gasPrice` alone: `baseGas` is unbounded and is the multiplier
  the attacker actually turns up.
- **Missing test hook — none for `StakingChecker`.** It is pure; the tests above need no infrastructure.
  For the `CowChecker` and `AddressPoisoningChecker` slices of the same finding, the hooks already exist
  (`CowChecker::with_order_api`, `cow.rs:242`; `Provider::mocked`, `core/src/provider/mod.rs:139`) and
  are used by this audit's F-ENG-037 and F-ENG-033 PoCs. The `sentinel-test-vectors` corpus (A8,
  unavailable) is the wrong oracle here regardless: the assertion is a *relation between two vectors*
  (identical but for refund fields), which a corpus of independent request/response pairs cannot state.
- **Where the fix belongs: the combinator plus a new refund policy.** Not the `RuleId` mapping — R-4.3
  already covers value sent to an unexpected recipient, and the refund is such a value transfer.
