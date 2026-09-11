# F-ENG-034 `EscapeHatchChecker` affirms the announcement shape for **any** `to` and runs ahead of the blocklist, so an R-4.6 target is rated `secure`

| Field | Value |
| --- | --- |
| Status | Verified |
| Crate and module | sentinel-engine, checkers/escape_hatch.rs |
| Location | crates/sentinel-engine/src/checkers/escape_hatch.rs:52-61 (related: main.rs:57-61, checkers/blocklist.rs:24-32, contracts/src/guard/SafenetGuard.sol:355-368) |
| Severity | High / High |
| Certainty | 97% (V-ENG, Phase 5; E1 — PoC executed) |
| Assumptions involved | A2, A3, A7, A15 |
| Tags | verdict-policy, charter, input-validation |

## Claim

`is_escape_hatch_call` affirms on four conditions: `operation == Call`, `value == 0`, `gas_price == 0`, and the calldata's first four bytes being the `announceTransaction` or `cancelAnnouncement` selector. It places no constraint on `to` and never ABI-decodes the arguments — any suffix after the selector is accepted.

The on-chain rule the checker mirrors is narrower on exactly that point: `SafenetGuard._isAutoAllowed` requires `to == address(this)`. So the shape the Guard auto-allows without any Sentinel review is a strict subset of the shape this checker affirms; the difference is precisely the set of announcement-shaped calls that _do_ reach the sentinel and _do_ need a verdict. Charter §2.18 states the boundary in the same terms — "Calls to the Safenet Guard's `announceTransaction` and `cancelAnnouncement` functions are auto-allowed by the Guard".

Because `EscapeHatchChecker` is second in the chain (`main.rs:59`) and the chain breaks at the first non-abstain, its affirmation suppresses `BaseChecker`, `BlocklistChecker`, and every later checker. Concretely, a zero-value `Call` to an address on the operator's blocklist whose calldata merely begins with the `announceTransaction` selector is answered `secure` instead of `insecure R-4.6` — the operator's only configured expression of R-4.6 never runs. `NestedSafeChecker`'s module doc records that this ordering hazard was recognised and worked around for that checker (`nested.rs:10-11`); `EscapeHatchChecker` did not get the same treatment.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The predicate never constrains `to` and never decodes the arguments | E2 | crates/sentinel-engine/src/checkers/escape_hatch.rs:52-61 | Q1 |
| 2 | Matching it yields `Secure` | E2 | crates/sentinel-engine/src/checkers/escape_hatch.rs:41-47 | Q2 |
| 3 | The checker's own doc states the "any `to`" choice explicitly | E2 | crates/sentinel-engine/src/checkers/escape_hatch.rs:16-21 | Q3 |
| 4 | It runs second, ahead of Base and Blocklist | E2 | crates/sentinel-engine/src/main.rs:57-61 | Q4 |
| 5 | `BlocklistChecker` is the operator's R-4.6 mechanism and is never reached | E2 | crates/sentinel-engine/src/checkers/blocklist.rs:24-32 | Q5 |
| 6 | The on-chain guard auto-allows this shape only for `to == address(this)` | E2 (reference-only Solidity, A7) | contracts/src/guard/SafenetGuard.sol:355-368 | Q6 |
| 7 | The Charter's carve-out is scoped to the Guard's own functions, and R-4.6 covers any interaction with a flagged target | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:350-352, :659-663 | Q7 |

**Q1** `crates/sentinel-engine/src/checkers/escape_hatch.rs:52-61`

```rust
fn is_escape_hatch_call(tx: &SafeTransaction) -> bool {
    if tx.operation != Operation::Call || !tx.value.is_zero || !tx.gas_price.is_zero {
        return false;
    }
    tx.data
        .starts_with(&safenet_guard::announceTransactionCall::SELECTOR)
        || tx
            .data
            .starts_with(&safenet_guard::cancelAnnouncementCall::SELECTOR)
}
```

**Q2** `crates/sentinel-engine/src/checkers/escape_hatch.rs:41-47`

```rust
    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        if is_escape_hatch_call(transaction) {
            Verdict::Secure
        } else {
            Verdict::Abstain
        }
    }
```

**Q3** `crates/sentinel-engine/src/checkers/escape_hatch.rs:16-21`

```rust
/// on its own merits, when it is itself proposed for execution. This holds
/// structurally for any `to`, not just a canonical, registered SafenetGuard
/// deployment (none is tracked here): a zero-value plain `CALL` cannot move
/// the Safe's funds or touch its storage. Both guards matter — a
/// `DELEGATECALL` would run arbitrary code from `to` inside the Safe's own
/// storage context (which is exactly why `SafenetGuard` itself refuses to
```

**Q4** `crates/sentinel-engine/src/main.rs:57-61`

```rust
    let engine = SentinelEngine::new(vec![
        Box::new(CancellationChecker),
        Box::new(EscapeHatchChecker),
        Box::new(BaseChecker),
        Box::new(BlocklistChecker::new(engine_config.blocklist)),
```

**Q5** `crates/sentinel-engine/src/checkers/blocklist.rs:24-32`

```rust
    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        if self.0.contains(&transaction.to) {
            Verdict::Insecure {
                rule: RuleId::R4_6KnownMaliciousTarget,
            }
        } else {
            Verdict::Abstain
        }
    }
```

**Q6** `contracts/src/guard/SafenetGuard.sol:355-368`

```solidity
    function _isAutoAllowed(address to, uint256 value, bytes calldata data, Enum.Operation operation, uint256 gasPrice)
        private
        view
        returns (bool allowed)
    {
        if (to != address(this) || value != 0 || operation != Enum.Operation.Call || gasPrice != 0 || data.length < 4) {
            return false;
        }
        // forge-lint: disable-next-line(unsafe-typecast)
        bytes4 selector = bytes4(data);
        return
            selector == SafenetGuard.announceTransaction.selector
                || selector == SafenetGuard.cancelAnnouncement.selector;
    }
```

**Q7** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:350-352` and `:659-663`

```text
#### Protocol boundary

- Calls to the Safenet Guard's `announceTransaction` and `cancelAnnouncement` functions are auto-allowed by the Guard without Sentinel review or Council arbitration.
```

```text
### R-4.6 — Known malicious or compromised target

#### Rule

- A transaction is insecure if it interacts with an address or contract where admissible evidence supports a reasonable finding that the target is malicious, compromised, exploited, or otherwise high-risk before the Council ruling.
```

## Trigger

Engine configured with a non-empty blocklist:

```toml
[engine]
blocklist = ["0x1111111111111111111111111111111111111111"]
address_poisoning_lookback_blocks = 50000
```

Request:

```json
{
  "block": "0x1500000",
  "transaction": {
    "chainId": "0x1",
    "safe": "0x5aFE3855358E112B5647B952709E6165e1c1eEEe",
    "to": "0x1111111111111111111111111111111111111111",
    "value": "0x0",
    "data": "<4-byte selector of announceTransaction((address,uint256,bytes,uint8,uint256,uint256,uint256,address,address))>00",
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

`data` is the bare selector plus one arbitrary trailing byte — the checker never decodes past the prefix, so it need not be a valid `announceTransaction` encoding. Expected per the Charter: `{"verdict":"insecure", "rule":"R-4.6"}`. Actual: `{"verdict":"secure"}` from position 2; `BlocklistChecker` at position 4 is never reached.

Corpus shape: this vector plus a control that keeps everything the same but changes the first four bytes of `data` (which must return `insecure R-4.6`) — the pair isolates the bypass. A second pair with `to` set to a non-blocklisted address and calldata that is _not_ a valid announcement (garbage suffix) documents the "affirmation without argument validation" half.

## Considered and rejected

- **The Guard would auto-allow this on-chain anyway, so the vote does not matter.** It would not: `_isAutoAllowed` returns `false` whenever `to != address(this)` (Q6), so this transaction goes through the normal attested path and the sentinel's `secure` vote is exactly what decides it.
- **A zero-value `CALL` cannot cause harm, as the module doc argues.** The doc's argument (Q3) is about the _Safe's own_ funds and storage, and it is correct for those. It does not address R-4.6, which forbids _interacting_ with a flagged target at all, nor the fact that an affirmation ends the chain. It also does not address the unvalidated calldata suffix: with `to` an attacker contract, the Safe is the `msg.sender` of an arbitrary call whose payload the engine never examined.
- **Selector collision is the only attack, and it is negligible.** A collision with a privileged function on an _existing_ contract is ~2^-32 and is not the claim here; the blocklist bypass needs no collision at all.
- **The blocklist is empty in practice.** It is a required config field that may be empty (`config.rs` `engine` table), but it is the only R-4.6 mechanism the engine has; an operator who populates it is entitled to have it consulted. Severity is stated on the assumption that it is populated.
- **`gas_price == 0` already limits the damage.** It does — this shape cannot carry a refund drain (F-ENG-031) and cannot move `value`. That is why this is High rather than Critical: the defect is a false `secure` on a transaction the Charter rules insecure, without a direct fund-loss primitive of its own.

## Remediation options

1. Move `EscapeHatchChecker` after `BlocklistChecker` in `main.rs:57-73`, as `nested.rs:10-11` already does for `NestedSafeChecker`. One-line change; keeps the affirmation but restores R-4.6.
2. Constrain `to`: affirm only when `to` is a configured/registered SafenetGuard deployment. Requires the engine to learn the guard address (config or an RPC `getStorageAt` of the Safe's guard slot); this is what makes the engine's rule match `_isAutoAllowed` exactly.
3. Return `Abstain` instead of `Secure`. The legitimate case (`to == guard`) is auto-allowed on-chain and never reaches the engine, so affirming buys nothing; abstaining costs nothing and removes an unevidenced affirmation.
4. ABI-decode the announcement argument rather than matching the selector prefix, so the engine at least knows the call is well-formed before affirming.

Tests to add: corpus vectors above; a unit test for `EscapeHatchChecker` (it currently has none — the module has no `#[cfg(test)]` block at all) pinning `to != guard` behaviour once a policy is chosen. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 82%. (Confirms lead ENG-H5, with the Charter §2.18 text now available: the carve-out is explicitly scoped to the Guard's own functions. All cited lines re-read in this checkout at commit 2893917. Note the analysis file's version of this lead predates the current code, which does now require `value == 0` and `gas_price == 0`; only the `to` gap remains.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts — the ordering claim checked in `main.rs` myself

| # | Verdict | Check |
| --- | --- | --- |
| 1 | **Supported** | `escape_hatch.rs:52-60`: the only tests are `tx.operation != Operation::Call |  | !tx.value.is_zero |  | !tx.gas_price.is_zero`followed by two`tx.data.starts_with(&…::SELECTOR)`tests. No`to`term; no`abi_decode`, so everything after byte 4 is unconstrained. |
| 2 | **Supported** | `escape_hatch.rs:41-47`. |
| 3 | **Supported** | The doc at `:16-21` says it "holds structurally for any `to`, not just a canonical, registered SafenetGuard deployment (none is tracked here)". The gap is deliberate, not accidental — which is worth stating in the remediation, because the fix is a policy decision, not a bug fix. |
| 4 | **Supported** | I read the registration list myself before reading R9's argument: `main.rs:57-73` is Cancellation, **EscapeHatch (2nd)**, Base (3rd), **Blocklist (4th)**, Nested, ExcessiveApproval, Cow, Staking, Refund, AddressPoisoning. `EscapeHatchChecker` is registered two positions ahead of `BlocklistChecker`, and `engine/mod.rs:66-68` breaks on its `Secure`. The ordering claim is correct. |
| 5 | **Supported** | `blocklist.rs:24-32` is the only use of the configured `blocklist` in the crate. |

### 2. The Solidity comparison, verified independently

This is the load-bearing citation and it holds exactly. `contracts/src/guard/SafenetGuard.sol:355-368`:

```solidity
    function _isAutoAllowed(address to, uint256 value, bytes calldata data, Enum.Operation operation, uint256 gasPrice)
        private
        view
        returns (bool allowed)
    {
        if (to != address(this) || value != 0 || operation != Enum.Operation.Call || gasPrice != 0 || data.length < 4) {
            return false;
        }
```

The Rust predicate reproduces four of the five conditions and drops `to != address(this)`. Under A7 the Solidity is the reference and a mismatch is a **Rust** finding, so this is correctly filed here. Charter §2.18:352 states the same boundary in words — "Calls to the Safenet Guard's `announceTransaction` and `cancelAnnouncement` functions are auto-allowed by the Guard without Sentinel review or Council arbitration" — i.e. the exemption is defined by _which contract is being called_, and the set of announcement-shaped calls that actually reach the sentinel is precisely the set the Guard would **not** auto-allow. Affirming that set is affirming the complement of the exemption.

I also confirm R9's stale-quote correction on ENG-H5: the current code **does** require `value == 0` and `gas_price == 0` (`escape_hatch.rs:53`), so the analysis file's quote is out of date and R9 was right to say so rather than repeat it. I re-read every other R9 finding's citations against the checkout for the same failure mode and found no other finding resting on a stale quote from that analysis file.

### 3. Severity — High confirmed, with the two variants separated

The finding mixes an `E2` variant and an `I` variant, and they should be reported as such:

- **`E2`, the blocklist bypass.** A zero-value, zero-`gasPrice` `Call` to a blocklisted address whose calldata begins with the `announceTransaction` selector is answered `secure` instead of `insecure R-4.6`, because the chain never reaches position 4. Fully code-traced. Caveat QA should record: `blocklist = []` in the shipped sample (`crates/sentinel-engine/sentinel-engine.sample.toml:22`), so this needs an operator who has actually populated the list — which is the only circumstance in which the finding matters at all.
- **`I`, the general case.** "Arbitrary zero-value call to an arbitrary `to` with arbitrary calldata after byte 4, rated `secure`" is the wider hole, but converting it into loss needs a victim contract that both dispatches on that selector and grants the Safe authority. I could not name one in this checkout, so it stays inference. I agree with R9's rejected-hypothesis #16 that a _chance_ selector collision is impractical, but note their reasoning is imprecise: an attacker does not brute-force a collision, they pick `to` from already-deployed contracts. That still requires a real victim ABI, so the conclusion survives.

Not Critical: the affirmed transaction cannot itself move native value or run code in the Safe's storage context (`value == 0`, `operation == Call`), so the malicious-transaction-rated-`secure` bullet applies only via the blocklist variant, which is operator-configuration-dependent. **High** is right.

### 4. Finding verdict

**Confirmed.** Certainty **85%**. Severity **High** (unchanged). This finding and F-ENG-035 are two distinct defects that happen to share a victim (the blocklist): this one is _the blocklist never runs_, F-ENG-035 is _the blocklist runs but looks in one place_. Both should ship; neither subsumes the other.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection.** `is_escape_hatch_call` (`escape_hatch.rs:52-61`) tests `operation`, `value`, `gas_price` and a `starts_with` on the two selectors — there is **no `to` term and no `abi_decode`** anywhere in the 61-line file; `:42-47` maps that to `Secure`; `main.rs:59` is the second `Box::new` and `main.rs:61` the fourth, so `BlocklistChecker` sits behind it; `engine/mod.rs:66-68` breaks on the affirmation. Not `E1`.

**Certainty unchanged at 85%. Severity unchanged at High.**

**PoC: `rust-audit/poc/F-ENG-034/`** — five tests appended to `escape_hatch.rs`, which today has **no test block at all**. Two are expected to fail on unfixed code, and they are deliberately separate because **options 1-3 close one and only option 4 closes the other**: the blocklist bypass and the "affirmation without argument validation" half are independent defects that happen to share a predicate.

The fixture uses the **bare selector plus one junk byte**, so the calldata decodes as nothing. That is the sharper form of the finding: the affirmation carries no information whatsoever about the call it affirms.

### Remediation check

- **Option 1 (move `EscapeHatchChecker` after `BlocklistChecker`) — sound for the tested case, unsound as a general fix.** One line, restores R-4.6, worth doing today. But it is exactly the per-pair patch F-ENG-044 identifies as a mitigation rather than a mechanism — and it is _the same reasoning that already succeeded for `NestedSafeChecker` (`nested.rs:10-11`) and was not applied here_. Shipping option 1 alone records the hazard in a second place instead of removing it.
- **Option 2 (affirm only when `to` is a registered SafenetGuard deployment) — sound, and it makes the engine's rule match `SafenetGuard._isAutoAllowed` exactly.** Cost: the engine must learn the guard address — from config (cheap, another per-chain list) or from an RPC `getStorageAt` of the Safe's guard slot (accurate, but makes the checker RPC-backed and forces it behind `main.rs:66`'s group, re-raising the very ordering question option 1 was meant to settle).
- **Option 3 (return `Abstain` instead of `Secure`) — sound, and the best value for the effort; my recommendation.** The legitimate case (`to == guard`) is auto-allowed **on-chain by the Guard**, per Charter §2.18 — "Calls to the Safenet Guard's `announceTransaction` and `cancelAnnouncement` functions are auto-allowed by the Guard without Sentinel review or Council arbitration" — so it never reaches the engine at all. The affirmation therefore buys nothing anyone consumes, while abstaining costs nothing and removes an unevidenced affirmation. Same argument as `refund.rs:60-67`.
- **Option 4 (ABI-decode the announcement argument) — sound, necessary, insufficient alone.** It closes the malformed-calldata half and nothing else: a well-formed announcement to a blocklisted address is still affirmed. Combine with 1, 2 or 3.
- **Do not combine options 1 and 3 and call it done.** If the checker abstains, its position is irrelevant and option 1 is redundant; if it still affirms, option 1 protects only against the blocklist and not against any other later denier. Take 3+4, or 2+4.
- **Test hook: none needed; the checker is pure.** This file having zero tests follows directly from `AGENTS.md`'s corpus-is-the-oracle rule with the corpus unavailable (A8). Note that unlike F-ENG-032 or F-ENG-044, a corpus vector **can** express this finding — one request, one wrong response — so the reason it was not caught is simply that no such vector was written.
- **Where the fix belongs: the checker,** with the combinator (F-ENG-044) removing the class. The `RuleId` mapping is fine.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 85% -> **97%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under `rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-034/append-to-src-checkers-escape_hatch.rs` appended to `crates/sentinel-engine/src/checkers/escape_hatch.rs`, then `cargo test -p sentinel-engine poc_f_eng_034`. Compiled on the first attempt. Full output: `rust-audit/poc/F-ENG-034/run-output.txt`.

### Verbatim result

```
running 5 tests
test checkers::escape_hatch::poc_f_eng_034::poc_f_eng_034_affirms_any_to_today ... ok
test checkers::escape_hatch::poc_f_eng_034::poc_f_eng_034_control_a_different_selector_is_denied ... ok
test checkers::escape_hatch::poc_f_eng_034::poc_f_eng_034_the_blocklist_never_runs_today ... ok
test checkers::escape_hatch::poc_f_eng_034::poc_f_eng_034_a_malformed_announcement_must_not_be_affirmed ... FAILED
test checkers::escape_hatch::poc_f_eng_034::poc_f_eng_034_an_r4_6_target_must_not_be_affirmed ... FAILED

---- ..._an_r4_6_target_must_not_be_affirmed stdout ----
assertion `left == right` failed
  left: Secure
 right: Insecure { rule: R4_6KnownMaliciousTarget }

---- ..._a_malformed_announcement_must_not_be_affirmed stdout ----
assertion `left != right` failed: the calldata is a bare selector plus a junk byte and decodes as nothing
  left: Secure
 right: Secure

test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 97 filtered out
```

Both expected-to-fail tests failed for the claimed reason, and they are independent:

1. **The blocklist bypass.** A `SentinelEngine` built as `[EscapeHatchChecker, BlocklistChecker([BLOCKLISTED])]` — `main.rs`'s own relative order (positions 2 and 4) — returns `Secure` for a transaction whose `to` is the operator's configured R-4.6 target. Closed by remediation option 1, 2 or 3.
2. **No argument validation.** The calldata is the bare `announceTransaction` selector plus one junk byte `0x00`; `is_escape_hatch_call` matches with `starts_with` and never ABI-decodes, so a payload that **decodes as nothing** is affirmed. Verified against an _unlisted_ `to` (`0x2222…`), so this half is entirely independent of the blocklist. Closed **only** by remediation option 4 — options 1-3 leave it open.

The control (test 3) passed and isolates the bypass to the selector alone: the identical transaction with `data` changed to `0xdeadbeef00` reaches the blocklist and is denied `R-4.6`. The only difference between the affirmed and the denied case is the first four bytes of calldata.

Residual uncertainty: the A7 reading of Charter §2.18 and of `SafenetGuard._isAutoAllowed`'s `to == address(this)` constraint (the Solidity was not executed this run). The engine-side behaviour is settled.
