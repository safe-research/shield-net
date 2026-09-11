# PoC — F-ENG-002 (High, 85%)

**`RuleId::R4_5ExcessiveApproval` claims `setApprovalForAll` is an unconditional immediate failure "per § 2.5"; the Charter makes operator approval-for-all conditional, so the engine denies standard NFT-marketplace approvals.**

> **Never compiled.** No Rust toolchain on the audit host (`rust-audit/state/baseline.md` §1).

## Read the direction of this one carefully

Every Critical in the engine set is a wrong vote in the **affirming** direction — a malicious transaction rated `secure`. **This finding and F-ENG-042 are the opposite shape: wrong votes in the _denying_ direction, against honest traffic.** The fixture below is therefore an _honest_ transaction — the mandatory first step of listing an NFT from a Safe on any major marketplace — and the defect is that the engine denies it. Do not read the PoC as an attack.

That direction is the one that costs the engine's operator money, and this audit settled the mechanism:

- **Charter § 2.15** (`safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:333`), verbatim: _"An affected Sentinel is a Sentinel whose vote may result in Council-directed slashing in the arbitration."_ § 6.3 step 2 confirms affected Sentinels are identified as part of the arbitration procedure, and § 6.5 that "A Council ruling determines the applicable Sentinel slashing outcome".
- The Charter defers the _amount and direction_ to protocol rules (§ 6.5, "Protocol economic mechanics include: bonds; slashing"), and the in-repo Solidity supplies them: `contracts/src/libraries/SentinelOracleRequests.sol:298-305` — _"A revealed vote is only slashed for losing an arbitrated dispute"_, returning `terms.slashAmount` when `approved != (state == State.RESOLVED_APPROVED)`.

So a sentinel that votes `insecure` on a listing approval, is disputed, and is overruled by the Council **is slashed for a denial it cast in good faith**. See F-ENG-042's `## QA (QA-ENG)` section for the full record of that verification.

## Apply and run

```bash
cat rust-audit/poc/F-ENG-002/append-to-src-checkers-excessive_approval.rs \
  >> crates/sentinel-engine/src/checkers/excessive_approval.rs
cargo test -p sentinel-engine poc_f_eng_002
git checkout -- crates/sentinel-engine/src/checkers/excessive_approval.rs
```

`rust-audit/poc/F-ENG-036/` appends to the same file; the two modules do not collide.

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._denies_a_routine_marketplace_listing_today` | **passes** | fails | Pins the defect. |
| `..._control_the_erc20_max_arm_stays_a_denial` | passes | **must still pass** | The ERC-20 arm is _correct_ and matches § 2.5 exactly. A fix that changes it has over-corrected. |
| `..._control_revocation_abstains` | passes | passes | `approved: false` must never be denied. |
| `..._a_conditional_rule_must_not_be_an_immediate_failure` | **fails** | passes | The finding. |

## The fixture, written out

An honest Safe owner listing an NFT. Nothing here is attacker-chosen.

| Field | Value |
| --- | --- |
| `chainId` | `0x1` |
| `safe` | `0x5aFE3855358E112B5647B952709E6165e1c1eEEe` |
| `to` | `0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D` — a canonical ERC-721 collection the Safe holds |
| `value` | `0x0` |
| `data` | `setApprovalForAll(0x1E0049783F008A0085193E00003D00cd54003c71, true)` — OpenSea's Seaport conduit |
| `operation` | `0` (`Call`) |
| `safeTxGas`/`baseGas`/`gasPrice`/`gasToken`/`refundReceiver` | all zero |
| `nonce` | `0x2a` |

Chain walk: Cancellation abstains (fields not all default) → EscapeHatch abstains (wrong selector) → Base abstains (`base.rs:87-90` allows a `Call` to another contract) → Blocklist abstains (unless the operator listed the collection) → NestedSafe abstains → **ExcessiveApproval denies at position 6.**

**Actual:** `{"verdict":"insecure","rule":"R-4.5"}`. **Charter-correct:** not an immediate failure — the Charter's own test is whether the approval is "plausibly required for the stated interaction", determined from onchain data (§ 2.8) and protocol-recorded purpose (§ 2.11), which the engine has gathered nothing about.

**What is _not_ wrong**, and it sharpens the finding: the ERC-20 arm (`amount == U256::MAX`, `excessive_approval.rs:22`) matches § 2.5's unconditional rule exactly. The defect is confined to the `OperatorApproval` arm and to the doc comment that authorises it.

## Remediation check (QA-ENG)

**This is one of the doc-comment findings, and the two halves must be kept apart.**

- **The doc-comment half.** `rule.rs:28-33` asserts "Per § 2.5, this sub-case is always functionally unlimited and needs no further analysis" of _both_ forms. § 2.5 says that of the ERC-20 max case only. This is a pure documentation fix and can ship on its own, today, with no behavioural risk. It belongs with F-ENG-001 and F-ENG-003, which the Critic named canonical for `RuleId` doc comments; F-ENG-039 is canonical for the _behavioural_ `BaseChecker` defect and is not implicated here.
- **The behavioural half.** `excessive_approval.rs:23` implements the doc comment. Fixing the comment without fixing the code leaves a checker that contradicts its own `RuleId`'s (now correct) documentation — worse than today, because the discrepancy is no longer visible in one file. **Ship both or neither.**
- **Option 1 (abstain on `OperatorApproval`, and fix the doc) — sound, and the honest answer.** An abstain is what the crate already does everywhere it lacks evidence to evaluate a rule. Cost, stated fairly in the finding: the engine stops flagging genuinely hostile `setApprovalForAll` grants, which is a real loss of coverage — but it is coverage the engine was never entitled to, since it evaluates a conditional rule without evaluating the condition.
- **Option 2 (deny unless `operator` is on a configured allow-list of canonical conduits) — sound, and it preserves coverage.** It mirrors what `BaseChecker` already does for fallback handlers and modules (`base.rs:13-30`) and `CowChecker` for `GPv2VaultRelayer`, so it is consistent with the crate's existing shape. Cost: a per-chain address list to maintain, and it still denies honest approvals to any venue not yet listed — moving the failure from "always wrong" to "wrong only for unlisted venues". **Given the bond exposure established above, prefer option 1 or 2 over the status quo, and if option 2 is chosen, size the list generously and fail _open_ (abstain) on an unlisted operator rather than denying** — otherwise the fix reproduces the finding for every new marketplace.
- **Option 3 (reuse `AddressPoisoningChecker`'s history as a proxy for "plausibly required") — unsound as a denial basis, and I would reject it.** Prior interaction with an operator is weak evidence in the affirming direction and near-worthless in the denying one: a Safe listing on a marketplace for the _first_ time has no history with that conduit, and is exactly the honest user this finding is about, so option 3 denies them. It also inherits F-ENG-033's defect that the same history is forgeable, and it makes R-4.5 RPC-backed with the abstain-on-failure and unbounded-fan-out consequences of F-ENG-009.
- **Missing test hook — none needed.** `ExcessiveApprovalChecker` is pure. The existing four tests (`excessive_approval.rs:49-135`) do not distinguish the two § 2.5 branches; the tests above do.
- **Where the fix belongs: the checker _and_ the Charter-to-`RuleId` mapping.** Both, and in that order of importance — the mapping's doc comment is what authorises the wrong behaviour.
