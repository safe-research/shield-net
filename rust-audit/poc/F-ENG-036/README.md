# PoC — F-ENG-036 (High, 82%)

**R-4.5 is implemented as an exact `U256::MAX` comparison, so `approve(X, 2^256-2)` evades it — and is then affirmed `secure` by the address-poisoning history bypass.**

> **Never compiled.** No Rust toolchain (`rust-audit/state/baseline.md` §1).

## Apply and run

```bash
cat rust-audit/poc/F-ENG-036/append-to-src-checkers-excessive_approval.rs \
  >> crates/sentinel-engine/src/checkers/excessive_approval.rs
cargo test -p sentinel-engine poc_f_eng_036
git checkout -- crates/sentinel-engine/src/checkers/excessive_approval.rs
```

`rust-audit/poc/F-ENG-002/` appends to the same file; the two modules do not collide.

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._control_exact_max_is_denied` | passes | **must still pass** | The single-bit delta between this and the next row is the whole finding. |
| `..._abstains_one_bit_below_max_today` | **passes** | fails | Pins the evasion. |
| `..._increase_allowance_produces_no_effect_today` | **passes** | fails | Pins the decoder gap. |
| `..._a_functionally_unlimited_approval_must_not_abstain` | **fails** | passes | Half 1. Closed by option 1 (or by option 2 at the engine level). |
| `..._increase_allowance_must_decode_to_an_approval_effect` | **fails** | passes | Half 2. Closed **only** by option 3. |

## The fixtures, written out

| Field | Trigger A | Trigger B | Trigger C (control) |
| --- | --- | --- | --- |
| `chainId` | `0x1` | `0x1` | `0x1` |
| `safe` | `0x5aFE…eEEe` | same | same |
| `to` | `0xA0b8…eB48` (USDC) | same | same |
| `value` | `0x0` | `0x0` | `0x0` |
| `data` | `approve(0x1111…1111, 0xffff…fffe)` — selector `0x095ea7b3`, amount `2^256 - 2` | `increaseAllowance(0x1111…1111, 0xffff…ffff)` — selector `0x39509351` | `approve(0x1111…1111, 0xffff…ffff)` — amount `U256::MAX` |
| gas/refund | all zero | all zero | all zero |
| `nonce` | `0x2a` | `0x2a` | `0x2a` |

`0x1111…1111` is a spender the Safe has already paid or approved on that token within `address_poisoning_lookback_blocks` — which is what turns a missed denial into a **false affirmation**.

Full-chain walk for A: Base abstains → Blocklist abstains → Nested abstains → **ExcessiveApproval abstains** (`amount != U256::MAX`) → Cow abstains (spender is not `GPv2VaultRelayer`) → Staking abstains → Refund abstains (`gas_price == 0`) → **AddressPoisoning returns `Secure`** on the exact-match spender. **Actual: `secure`. Charter-correct: `insecure R-4.5`.**

For B the whole chain abstains — a coverage gap, not a false `secure`, because `AddressPoisoningChecker::decode_target` also fails to decode `increaseAllowance`.

Charter § 2.5, verbatim on this point: **"An approval can be functionally unlimited even if not technically max `uint256`."** The max-`uint256` case is singled out only as the one needing no further analysis; the engine implements the shortcut and nothing else.

## Remediation check (QA-ENG)

- **Option 1 (a policy that can express "functionally unlimited") — sound in direction, but both concrete forms in the finding have problems worth stating.** A multiple of `totalSupply` needs one `eth_call` per request, which moves this checker into the RPC-backed group — re-raising the ordering question (it would then run _after_ `CowChecker` and `StakingChecker`, both of which can affirm) and inheriting F-ENG-009's unbounded fan-out and F-ENG-005's missing deadline. It also fails for rebasing/elastic-supply tokens and for tokens whose `totalSupply` reverts. A fixed threshold such as `2^128` needs no RPC and is robust, but is a **decimals-dependent** guess: for an 18-decimal token `2^128` is ~3.4×10^20 whole units (far above any real allowance), while for a 0-decimal NFT-like ERC-20 it is absurd in the other direction. Recommendation: ship the fixed threshold first because it is RPC-free and strictly better than today, and make it configurable per chain rather than a constant.
- **Option 2 (stop `AddressPoisoningChecker` affirming `approve` calls at all) — sound, cheap, and the one I would ship first.** History says the _spender_ is not a poisoned lookalike, which is orthogonal to whether the _amount_ is acceptable; returning `Abstain` for `TargetKind::Approval` closes the false `secure` half of Trigger A with no new RPC and no threshold to tune. It does not make the transaction _denied_ — that still needs option 1 — but it stops the engine bonding an approving vote on it, which is the part that is Critical-adjacent. Note this overlaps F-ENG-033 option 2, which goes further (deny-only for both `TargetKind`s); if F-ENG-033 option 2 ships, this is subsumed.
- **Option 3 (add `increaseAllowance`, and `permit`, to `contracts/bindings.rs` and `decode_call`) — sound and independent.** It is the only option that closes half 2. Caution on `permit`: it is an _off-chain-signature_ grant carried in calldata, so decoding it as an `Erc20Approval` effect is right for `ExcessiveApprovalChecker` but would give `AddressPoisoningChecker` a spender it should probably treat differently — scope the change to the effect decoder and re-check both consumers.
- **Option 4 (fix this so an earlier checker catches CoW's extreme cases too) — sound and worth stating as a dependency, not a fix.** F-ENG-037's degenerate TWAP batch is only reachable _because_ `2^256-2` is not `U256::MAX`. Options 1 or 2 here reduce F-ENG-037's blast radius but do not close it; F-ENG-037 still needs its own bound on `n`.
- **Missing test hook — none.** Both `ExcessiveApprovalChecker` and `decode_target_effects` are pure. The existing four tests (`excessive_approval.rs:49-135`) cover only `MAX`, a small amount, and `setApprovalForAll` both ways.
- **Where the fix belongs: the checker (amount policy) and the effect decoder (`increaseAllowance`).** The `RuleId` mapping is fine — R-4.5's doc comment is wrong about `setApprovalForAll` (F-ENG-002) but correct about the ERC-20 amount case.
