# F-ENG-036 R-4.5 is implemented as an exact `U256::MAX` comparison, so `approve(X, 2^256-2)` evades it — and is then affirmed `secure` by the address-poisoning history bypass

| Field | Value |
| --- | --- |
| Status | Verified |
| Crate and module | sentinel-engine, checkers/excessive_approval.rs |
| Location | crates/sentinel-engine/src/checkers/excessive_approval.rs:19-33 (related: contracts/target_effects.rs:58-132, checkers/address_poisoning.rs:132-137, :321-333) |
| Severity | High / High |
| Certainty | 94% (V-ENG, Phase 5; E1 — PoC executed) |
| Assumptions involved | A2, A3, A15 |
| Tags | verdict-policy, charter, input-validation |

## Claim

`ExcessiveApprovalChecker` denies an ERC-20 approval only when the amount is _bit-for-bit_ `U256::MAX`. Any smaller value — including `2^256 - 2`, `2^255`, or `type(uint128).max` for a token whose total supply is far below that — abstains. Two consequences:

1. **The Charter's R-4.5 is broader than the code.** §2.5 says an approval is functionally unlimited if its amount "materially exceeds what is plausibly needed for the stated interaction", and adds explicitly: "An approval can be functionally unlimited even if not technically max `uint256`." The max-`uint256` case is singled out only as the one that needs no further analysis. The engine implements the shortcut and nothing else.
2. **The evasion is not merely a missed denial.** `ExcessiveApprovalChecker` is 6th; `AddressPoisoningChecker` is 10th and affirms `Secure` on any non-zero `approve` whose spender has prior `Transfer` _or_ `Approval` history with the Safe on that token. So `approve(<any address the Safe has ever paid or approved>, 2^256-2)` is answered **`secure`** — a transaction the Charter rules insecure.

Separately, the effect decoder recognises no allowance-_increasing_ entry point other than `approve`: `decode_call`'s selector chain covers `transfer`, `transferFrom`, `approve`, `setApprovalForAll` and the ERC-721/1155 transfers, but not `increaseAllowance(address,uint256)` — a widely deployed OpenZeppelin/USDC extension. `increaseAllowance(spender, 2^256-1)` from a zero starting allowance yields an allowance of `2^256-1` and produces **no effect at all** for this checker to examine.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The unlimited test is an equality against `U256::MAX` | E2 | crates/sentinel-engine/src/checkers/excessive_approval.rs:19-33 | Q1 |
| 2 | The effect decoder has no `increaseAllowance` arm; an unrecognised selector produces no effect | E2 | crates/sentinel-engine/src/contracts/target_effects.rs:67-71, :121-132 | Q2 |
| 3 | No `increaseAllowance` binding exists in the crate at all | E2 | crates/sentinel-engine/src/contracts/bindings.rs:59-68 | Q3 |
| 4 | `AddressPoisoningChecker` treats any non-zero `approve` as a candidate and affirms on an exact history match | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:132-137 | Q4 |
| 5 | The evidence pool deliberately merges `Transfer` and `Approval` history, so a spender the Safe merely _paid_ once qualifies | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:15-18 | Q5 |
| 6 | The Charter treats non-max amounts as capable of being functionally unlimited | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:635-655, :150-154 | Q6 |

**Q1** `crates/sentinel-engine/src/checkers/excessive_approval.rs:19-33`

```rust
    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        for effect in decode_target_effects(transaction) {
            let unlimited = match effect.kind {
                EffectKind::Erc20Approval { amount } => amount == U256::MAX,
                EffectKind::OperatorApproval { approved } => approved,
                _ => false,
            };
            if unlimited {
                return Verdict::Insecure {
                    rule: RuleId::R4_5ExcessiveApproval,
                };
            }
        }
        Verdict::Abstain
    }
```

**Q2** `crates/sentinel-engine/src/contracts/target_effects.rs:67-71` and `:121-132`

```rust
    let data = &tx.data;
    if data.is_empty() {
        return effects;
    }
    if let Ok(call) = erc20::transferCall::abi_decode(data) {
```

```rust
    } else if let Ok(call) = erc1155::safeBatchTransferFromCall::abi_decode(data) {
        effects.push(TargetEffect {
            recipient: call.to,
            kind: EffectKind::Erc1155BatchTransfer {
                ids: call.ids,
                amounts: call.amounts,
            },
        });
    }

    effects
}
```

**Q3** `crates/sentinel-engine/src/contracts/bindings.rs:59-68`

```rust
pub mod erc20 {
    alloy::sol! {
        function transfer(address to, uint256 amount);
        function transferFrom(address from, address to, uint256 amount);
        function approve(address spender, uint256 amount);

        event Transfer(address indexed from, address indexed to, uint256 amount);
        event Approval(address indexed owner, address indexed spender, uint256 amount);
    }
}
```

**Q4** `crates/sentinel-engine/src/checkers/address_poisoning.rs:132-137`

```rust
    if let Ok(call) = approveCall::abi_decode(&tx.data) {
        // `approve(spender, 0)` is the standard way to *revoke* an
        // allowance — including to a poisoned lookalike — and must never
        // itself be denied.
        return (!call.amount.is_zero).then_some((call.spender, TargetKind::Approval));
    }
```

**Q5** `crates/sentinel-engine/src/checkers/address_poisoning.rs:15-18`

```rust
//! returns [`Verdict::Insecure`]. A novel candidate, with nothing to
//! compare it against, returns [`Verdict::Abstain`]: novelty alone isn't
//! grounds for denial. The evidence pool covers `Transfer` and `Approval`
//! together, so a lookalike of an address `safe` only ever paid still
```

**Q6** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:635-655` (extract) and `:150-154`

```text
### R-4.5 — Authorization-target manipulation: excessive approval amount

#### Rule

- A transaction is insecure if it grants a functionally unlimited approval (§ 2.5).

#### Immediate failure

- Max `uint256` ERC- 20 approval is always functionally unlimited.
- The Council rules immediately without further analysis.
```

```text
- An approval can be functionally unlimited even if not technically max `uint256`.
```

```text
### § 2.5 Functionally unlimited token approval

#### Definition

- A token approval is functionally unlimited if its amount or permission scope materially exceeds what is plausibly needed for the stated interaction.
```

## Trigger

**Trigger A — false `secure` on a near-max approval.** Pick a token the Safe has used and an address `X` that appears as the `to` of a `Transfer` (or the `spender` of an `Approval`) from the Safe on that token within `address_poisoning_lookback_blocks` of `block`:

```json
{
  "block": "<a block at or after that log>",
  "transaction": {
    "chainId": "<the engine provider's chain id>",
    "safe": "<the Safe>",
    "to": "<that token>",
    "value": "0x0",
    "data": "0x095ea7b3<X, 32-byte left-padded>fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffe",
    "operation": 0,
    "safeTxGas": "0x0",
    "baseGas": "0x0",
    "gasPrice": "0x0",
    "gasToken": "0x0000000000000000000000000000000000000000",
    "refundReceiver": "0x0000000000000000000000000000000000000000",
    "nonce": "0x2a"
  }
}
```

i.e. `approve(X, 2^256 - 2)`. Chain: Base abstains, Blocklist abstains, Nested abstains, **ExcessiveApproval abstains** (`amount != U256::MAX`), Cow abstains (`X != GPv2VaultRelayer`), Staking abstains, Refund abstains (`gas_price == 0`), **AddressPoisoning returns `Secure`**. Expected per the Charter: `insecure R-4.5`.

**Trigger B — no effect decoded at all.** Same shape with `data = increaseAllowance(X, 0xffff…ffff)` (selector `0x39509351`). `decode_target_effects` returns an empty vector, so `ExcessiveApprovalChecker` abstains; `AddressPoisoningChecker::decode_target` also fails to decode it and abstains. The whole chain abstains — no denial, but also no affirmation, so this half is a coverage gap rather than a false `secure`.

**Trigger C — control.** The identical body with amount `0xffff…ffff` (`U256::MAX`) must return `insecure R-4.5`; the delta from Trigger A across a single bit is the regression signal for a corpus vector.

## Considered and rejected

- **`ExcessiveApprovalChecker` runs before `AddressPoisoningChecker`, so it wins.** It runs first but _abstains_ (`excessive_approval.rs:32`), so the chain continues (`engine/mod.rs:66-68`) and the later affirmation stands. Ordering only helps when the earlier checker actually denies.
- **`CowChecker` denies the dangling approval.** Only when the spender is `GPv2VaultRelayer` (`cow.rs:460-473`); Trigger A uses an arbitrary spender.
- **A near-max approval is out of MVP scope by design.** The code carries no such comment; `rule.rs`'s `R4_5ExcessiveApproval` documentation and this checker's module doc ("Detection of functionally unlimited token allowances", `excessive_approval.rs:1`) both claim the general property. Even if the _denial_ is deliberately narrow, the _affirmation_ at `address_poisoning.rs:332` is what makes this a finding: the engine should abstain where it has no policy, not affirm.
- **ERC-721 `approve(address,uint256)` shares the selector, so `U256::MAX` could be a token id.** True and noted in `target_effects.rs:29-32`; it makes the existing check slightly over-broad, not under-broad, and does not affect this finding.
- **`increaseAllowance` is not part of ERC-20.** It is not in the standard, but it is present on widely held tokens (OpenZeppelin's `ERC20` carried it for years; USDC exposes it). The engine's own decoder already goes beyond ERC-20 into ERC-721/1155, so the omission is a gap rather than a scope decision.

## Remediation options

1. Replace the equality with a policy that can express "functionally unlimited": e.g. deny when the approval exceeds a configurable multiple of the token's `totalSupply` (needs one `eth_call`, moving this checker into the RPC-backed group), or a fixed high threshold such as `2^128` for tokens with ≤ 18 decimals.
2. Independently of the amount policy, stop `AddressPoisoningChecker` from affirming `approve` calls at all — history says the _spender_ is not a poisoned lookalike, which is orthogonal to whether the _amount_ is acceptable. Returning `Abstain` for `TargetKind::Approval` closes Trigger A without any new RPC.
3. Add `increaseAllowance` (and, if a token-permission policy is wanted, `permit`) to `contracts/bindings.rs` and to `decode_call`'s selector chain, mapping to `EffectKind::Erc20Approval`.

Tests to add: unit tests for `U256::MAX - 1` and for `increaseAllowance` (the four existing tests cover only `MAX`, a small amount, and `setApprovalForAll` both ways — `excessive_approval.rs:49-135`); corpus vectors A–C. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 80%. (Confirms lead ENG-H7 and adds the `increaseAllowance` decoding gap, which is not in the analysis file. The Charter now available makes the "non-max amounts can be functionally unlimited" point textual rather than inferred. Cited lines re-read at commit 2893917; not executed — A9 is FALSE this run.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts

**Claim 1 — Supported.** `excessive_approval.rs:19-33`, verbatim:

```rust
        for effect in decode_target_effects(transaction) {
            let unlimited = match effect.kind {
                EffectKind::Erc20Approval { amount } => amount == U256::MAX,
                EffectKind::OperatorApproval { approved } => approved,
                _ => false,
            };
```

An exact equality, nothing else. Note the `_ => false` arm also drops `EffectKind::ValueTransfer` entirely, which is why this checker cannot help against F-ENG-030 or F-ENG-033 either.

**Claim 2 — Supported.** I read `decode_call`'s full selector chain (`target_effects.rs:58-132`): `transfer`, `transferFrom`, `approve`, `setApprovalForAll`, the two ERC-721 `safeTransferFrom` overloads, and the two ERC-1155 transfers. There is no `increaseAllowance` arm, and `grep -rn increaseAllowance crates/` returns nothing in the whole workspace — no binding exists to decode it with (`contracts/bindings.rs:59-66` defines only `transfer`/`transferFrom`/`approve` for ERC-20). An unrecognised selector with `value == 0` produces an empty effect vector (`:60-70`), so the checker's loop body never executes.

### 2. Charter check — I read §2.5 and R-4.5 in full, not just the quoted lines

Both citations are accurate and, read in context, they support R9 more strongly than the excerpt suggests. `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:154`:

> - A token approval is functionally unlimited if its amount or permission scope materially exceeds what is plausibly needed for the stated interaction.

and `:643-644`, `:654`:

> - Max `uint256` ERC- 20 approval is always functionally unlimited.
> - The Council rules immediately without further analysis. …
> - An approval can be functionally unlimited even if not technically max `uint256`.

The structure is explicit: max-`uint256` is the _immediate-failure shortcut_, and R-4.5's substance is the `§2.5` "materially exceeds" test, for which the Charter directs the Council to weigh token supply, decimals and the nature of the interaction (`:646-650`). The engine implements the shortcut and nothing else. Under A15 this verdict-policy claim reaches **Confirmed**.

In fairness to the code — and R9 does not say this — the omitted half genuinely requires token supply and decimals, which the engine does not fetch, and PROMPT.md's framing rewards a checker that abstains rather than guesses. A checker that only implements the deterministic half is a defensible _design_; what makes it a finding is consequence 2, below, where abstention is not the outcome.

### 3. The consequence that makes this a finding, re-derived

`approve(X, 2^256-2)` where `X` has prior non-zero `Transfer` **or** `Approval` history with the Safe on that token: `ExcessiveApprovalChecker` (6th) abstains because `2^256-2 != U256::MAX`; nothing between positions 7 and 9 has an opinion on a lone `approve`; `AddressPoisoningChecker` (10th) decodes it as `(spender, TargetKind::Approval)` (`address_poisoning.rs:132-137`), finds `ExactMatch`, and returns `Verdict::Secure` (`:325-333`). Verified end to end. This is not a missed denial — it is an affirmation, and per `crates/sentinel/src/service.rs:173-176` the sentinel bonds an approving vote on it.

The reachability caveat, which R9 states but should be read alongside the trigger: `X` must already have history with the Safe on that token. The natural attacker for this is a compromised or malicious _existing counterparty_ — a router, relayer or vendor the Safe has legitimately paid before — not a stranger. That is a realistic threat model, not a contrived one, but it is narrower than "any address".

### 4. Severity

**High** confirmed. It produces a `secure` verdict on a functionally unlimited approval, which is one step from a drain rather than a drain itself (the spender must still pull the funds), and it needs a spender with prior history. Not Critical for that reason; clearly above Medium because the outcome is an affirmative attestation of a Charter-insecure transaction.

The `increaseAllowance` half is a separate, weaker sub-claim: it produces `Abstain`, not `secure`, so on its own it would be Low. It belongs in this file — same rule, same checker — but the remediation section should treat it as a distinct item so it is not lost behind the amount-threshold fix.

### 5. Finding verdict

**Confirmed.** Certainty **82%**. Severity **High** (unchanged). Cross-reference: this finding is the enabler for F-ENG-037 — the CoW TWAP path also relies on `ExcessiveApprovalChecker` not catching `U256::MAX - 1`, so a fix here would blunt (though not close) that one too.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection.** `excessive_approval.rs:22` is `EffectKind::Erc20Approval { amount } => amount == U256::MAX`, a bit-for-bit equality, so `U256::MAX - 1` falls to `_ => false` at `:24` and the checker abstains at `:32`. `decode_call`'s selector chain (`contracts/target_effects.rs`) covers `transfer`, `transferFrom`, `approve`, `setApprovalForAll` and the ERC-721/1155 transfers, and `contracts/bindings.rs:59-68` declares no `increaseAllowance`, so that call yields an **empty** effect vector — nothing for the checker to examine. The false-`secure` half follows from `address_poisoning.rs:325-333` affirming on an `ExactMatch` spender. Not `E1`.

**Certainty unchanged at 82%. Severity unchanged at High.**

**PoC: `rust-audit/poc/F-ENG-036/`** — five tests appended to `excessive_approval.rs` (a sibling of F-ENG-002's module; the two do not collide). The single-bit delta between the `U256::MAX` control and the `U256::MAX - 1` case is the whole finding and is asserted as a pair. Two tests are expected to fail on unfixed code, kept separate because **only option 3 closes the `increaseAllowance` half**.

### Remediation check

- **Option 1 (a policy that can express "functionally unlimited") — sound in direction; both concrete forms have problems the finding does not state.** A multiple of `totalSupply` needs one `eth_call` per request, moving this checker from position 6 into the RPC-backed group — behind `CowChecker` and `StakingChecker`, both of which can affirm, which would _re-open_ F-ENG-031's ordering problem for this rule — and it inherits F-ENG-009's unbounded fan-out and F-ENG-005's missing deadline. It also fails for rebasing or elastic-supply tokens and for tokens whose `totalSupply` reverts. A fixed threshold such as `2^128` needs no RPC and is robust, but is **decimals-dependent**: for an 18-decimal token `2^128` is ~3.4×10^20 whole units (far above any real allowance), while for a 0-decimal token it is absurd in the other direction. **Recommendation: ship the fixed threshold first** — RPC-free and strictly better than today — and make it a per-chain config value rather than a constant, so it can be tuned without a release.
- **Option 2 (stop `AddressPoisoningChecker` affirming `approve` calls) — sound, cheap, and the one to ship first.** History says the _spender_ is not a poisoned lookalike, which is orthogonal to whether the _amount_ is acceptable; returning `Abstain` for `TargetKind::Approval` closes the false-`secure` half of Trigger A with no new RPC and no threshold to tune. It does not make the transaction _denied_ — that still needs option 1 — but it stops the engine bonding an approving vote, which is the part that is Critical-adjacent. **Note the overlap: F-ENG-033 option 2 goes further (deny-only for both `TargetKind`s) and subsumes this.** If F-ENG-033 option 2 ships, do not implement this separately.
- **Option 3 (add `increaseAllowance`, and `permit`, to `bindings.rs` and `decode_call`) — sound and independent; the only option that closes the second half.** One caution on `permit`: it is an off-chain-signature grant carried in calldata, so decoding it as an `Erc20Approval` effect is right for `ExcessiveApprovalChecker` but hands `AddressPoisoningChecker` a spender it should probably treat differently. Scope the change to the effect decoder and re-check both consumers.
- **Option 4 (fix this so an earlier checker catches CoW's extreme cases) — a dependency, not a fix.** F-ENG-037's degenerate TWAP batch is reachable _because_ `2^256-2` is not `U256::MAX`, so options 1 or 2 reduce its blast radius — but not its plausible variant (`2^129 - 1`, far below any max-adjacent threshold). F-ENG-037 still needs its own bound on `n`.
- **Test hook: none needed.** Both `ExcessiveApprovalChecker` and `decode_target_effects` are pure. The four existing tests (`excessive_approval.rs:49-135`) cover only `MAX`, a small amount, and `setApprovalForAll` both ways.
- **Where the fix belongs: the checker (amount policy) and the effect decoder (`increaseAllowance`).** The `RuleId` mapping is fine on this point — R-4.5's doc comment is wrong about `setApprovalForAll` (F-ENG-002) but correct about the ERC-20 amount case.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 82% -> **94%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under `rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-036/append-to-src-checkers-excessive_approval.rs` appended to `crates/sentinel-engine/src/checkers/excessive_approval.rs` (alongside F-ENG-002's module; the two do not collide), then `cargo test -p sentinel-engine poc_f_eng_036`. Compiled on the first attempt. Full output: `rust-audit/poc/F-ENG-036/run-output.txt`.

### Verbatim result

```
running 5 tests
test checkers::excessive_approval::poc_f_eng_036::poc_f_eng_036_control_exact_max_is_denied ... ok
test checkers::excessive_approval::poc_f_eng_036::poc_f_eng_036_abstains_one_bit_below_max_today ... ok
test checkers::excessive_approval::poc_f_eng_036::poc_f_eng_036_increase_allowance_produces_no_effect_today ... ok
test checkers::excessive_approval::poc_f_eng_036::poc_f_eng_036_a_functionally_unlimited_approval_must_not_abstain ... FAILED
test checkers::excessive_approval::poc_f_eng_036::poc_f_eng_036_increase_allowance_must_decode_to_an_approval_effect ... FAILED

---- ..._a_functionally_unlimited_approval_must_not_abstain stdout ----
assertion `left != right` failed: § 2.5: an approval can be functionally unlimited even if not technically \
max uint256
  left: Abstain
 right: Abstain

---- ..._increase_allowance_must_decode_to_an_approval_effect stdout ----
an allowance-increasing call must produce an approval effect

test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 102 filtered out
```

Both expected-to-fail tests failed for the claimed reason, and they are independent halves:

1. **The one-bit evasion.** `approve(SPENDER, 2^256 - 2)` on USDC returns `Abstain`, while the identical transaction with `2^256 - 1` returns `Insecure { rule: R4_5ExcessiveApproval }` (control, test 0). A single-bit delta flips the verdict, because `excessive_approval.rs:22` is a bit-for-bit `amount == U256::MAX` equality. Closed by remediation option 1 or 2.
2. **`increaseAllowance` decodes to nothing at all.** `decode_target_effects(&tx)` returns an **empty** slice for `increaseAllowance(SPENDER, 2^256 - 1)` — test (2) asserts emptiness and passed; test (4) asserts an `EffectKind::Erc20Approval` effect exists and failed. There is therefore nothing for the checker to examine, whatever threshold it uses. Closed **only** by remediation option 3 (add the binding and the selector arm); options 1 and 2 leave it open. From a zero starting allowance this call yields an allowance of `2^256 - 1`.

Residual uncertainty: the behaviour is settled. What remains is the A7/A15 Charter reading (§2.5's "An approval can be functionally unlimited even if not technically max `uint256`") and the second half of the title — that such an approval is then _affirmed_ `secure` by the address-poisoning bypass, which is a composition claim across F-ENG-033 and F-ENG-044 (both independently verified this phase) that this PoC did not run end to end.
