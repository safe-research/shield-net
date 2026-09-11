# F-ENG-037 The CoW TWAP approval tolerance is sized by an attacker-chosen `n`, so a near-unlimited relayer approval is rated `secure`

| Field | Value |
| --- | --- |
| Status | Verified |
| Crate and module | sentinel-engine, checkers/cow.rs |
| Location | crates/sentinel-engine/src/checkers/cow.rs:544-554 (related: cow.rs:531-542, :351-383) |
| Severity | Medium / High |
| Certainty | 96% (V-ENG, Phase 5; E1 — PoC executed) |
| Assumptions involved | A2, A3, A15 |
| Tags | verdict-policy, charter, input-validation |

## Claim

`check_twap_batch` accepts an approval up to `max_approval_for_twap_total(total, n) = total + (n - 1)`, where both `total = partSellAmount * n` and `n` come from the `staticInput` of the same `createWithContext` call the attacker supplies. The headroom is therefore attacker-sized. Setting `partSellAmount = 0` makes `total = 0` (`checked_mul` does not overflow on a zero operand) while `n = U256::MAX` makes the ceiling `U256::MAX - 1`, so a batch of

- `approve(GPv2VaultRelayer, 2^256 - 2)` on any token, and
- `createWithContext` with the canonical TWAP handler and `CurrentBlockTimestampFactory`, `sellToken` equal to that token, `receiver = safe`, `partSellAmount = 0`, `n = U256::MAX`

is answered **`secure`** at `cow.rs:382`. `ExcessiveApprovalChecker`, which runs first, abstains because the amount is not literally `U256::MAX` (F-ENG-036).

The rounding argument the tolerance is built on holds only for the Safe app's own construction, where `partSellAmount = floor(desired_total / n)` and `n` is a small part count. Nothing constrains `n` to be small, or `partSellAmount` to be non-zero, or the pair to be internally consistent — the code's own comment states the assumption ("the Safe app computes `partSellAmount` as `floor(desired_total / n)`") without enforcing it.

Under Charter §2.5 / R-4.5 an approval of `2^256 - 2` on any real token materially exceeds what the stated interaction (an order selling zero tokens) plausibly needs, so the Charter rules it insecure; the engine affirms.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The tolerance is `total + (n - 1)`, computed from the order's own `n` | E2 | crates/sentinel-engine/src/checkers/cow.rs:544-554 | Q1 |
| 2 | `total` and `n` are decoded straight out of the attacker-supplied `staticInput`; only overflow of `partSellAmount * n` is rejected | E2 | crates/sentinel-engine/src/checkers/cow.rs:531-542 | Q2 |
| 3 | Passing the token and amount tests yields `Secure` | E2 | crates/sentinel-engine/src/checkers/cow.rs:370-383 | Q3 |
| 4 | Nothing else in the batch path constrains `n` or `partSellAmount`: only `handler` and `factory` are pinned | E2 | crates/sentinel-engine/src/checkers/cow.rs:492-499 | Q4 |
| 5 | The Charter rules a functionally unlimited approval insecure, and says non-max amounts can qualify | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:639, :654, :150-154 | Q5 |

**Q1** `crates/sentinel-engine/src/checkers/cow.rs:544-554`

```rust
/// The most a genuine `approve` may exceed a TWAP order's own
/// `partSellAmount * n` total by and still be considered an exact match:
/// the Safe app computes `partSellAmount` as `floor(desired_total / n)`, so
/// up to `n - 1` units of whatever total the user actually approved get
/// truncated away and never show up in `partSellAmount * n`. Sized off the
/// order's own `n` rather than a fixed amount or percentage, since neither
/// this checker nor the order itself knows the sell token's decimals or
/// value.
fn max_approval_for_twap_total(total_sell_amount: U256, n: U256) -> U256 {
    total_sell_amount.saturating_add(n.saturating_sub(U256::from(1)))
}
```

**Q2** `crates/sentinel-engine/src/checkers/cow.rs:531-542`

```rust
fn twap_order_terms(tx: &SafeTransaction) -> Option<(Address, Address, U256, U256)> {
    if tx.operation != Operation::Call || !tx.value.is_zero || tx.to != COMPOSABLE_COW {
        return None;
    };
    let create = createWithContextCall::abi_decode(&tx.data).ok?;
    if create.params.handler != TWAP_HANDLER || create.factory != CURRENT_BLOCK_TIMESTAMP_FACTORY {
        return None;
    };
    let order = TwapData::abi_decode(&create.params.staticInput).ok?;
    let total = order.partSellAmount.checked_mul(order.n)?;
    Some((order.sellToken, order.receiver, total, order.n))
}
```

**Q3** `crates/sentinel-engine/src/checkers/cow.rs:370-383`

```rust
        if receiver != safe && !receiver.is_zero {
            return Verdict::Insecure {
                rule: RuleId::R4_4AuthorizationTarget,
            };
        }
        if approved_token != sell_token
            || approved_amount > max_approval_for_twap_total(total_sell_amount, n)
        {
            return Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            };
        }
        Verdict::Secure
    }
```

**Q4** `crates/sentinel-engine/src/checkers/cow.rs:492-499`

```rust
fn is_twap_create(tx: &SafeTransaction) -> bool {
    tx.operation == Operation::Call
        && tx.value.is_zero
        && tx.to == COMPOSABLE_COW
        && createWithContextCall::abi_decode(&tx.data).is_ok_and(|call| {
            call.params.handler == TWAP_HANDLER && call.factory == CURRENT_BLOCK_TIMESTAMP_FACTORY
        })
}
```

**Q5** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:639`, `:654`, `:150-154`

```text
- A transaction is insecure if it grants a functionally unlimited approval (§ 2.5).
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

A MultiSend delegatecall (to any canonical deployment, e.g. `0x218543288004CD07832472D464648173c77D7eB7`) with exactly two packed `Call` entries, `chainId` one of 1 / 100 / 42161:

1. `to = <token>`, `value = 0`, `data = approve(0xC92E8bdf79f0507f65a392b0ab4667716BFE0110, 0xffff…fffe)` (`2^256 - 2`);
2. `to = 0xfdaFc9d1902f4e0b84f65F49f244b32b31013b74` (ComposableCoW), `value = 0`, `data = createWithContext(params, factory, data, dispatch)` with `params.handler = 0x6cF1e9cA41f7611dEf408122793c358a3d11E5a5`, `params.salt = 0x00…00`, `params.staticInput = abi.encode(TwapData{ sellToken: <token>, buyToken: <any>, receiver: <the Safe>, partSellAmount: 0, minPartLimit: 0, t0: 0, n: 2^256-1, t: 0, span: 0, appData: 0x00…00 })`, `factory = 0x52eD56Da04309Aca4c3FECC595298d80C2f16BAc`, `data = 0x`, `dispatch = true`.

Chain: Base allows (both sub-calls target other contracts, `base.rs:210-212`), Blocklist abstains, Nested abstains (outer is a delegatecall), ExcessiveApproval abstains (`2^256-2 != U256::MAX`), **Cow returns `Secure`**: `check_dangling_approval` abstains because a TWAP create is present (`cow.rs:392-396`), `check_presignature_batch` abstains, `check_twap_batch` computes `total = 0`, `max = 2^256 - 2`, and `approved_amount == max`, so the `>` comparison is false.

Intermediate variant, for a corpus that wants a plausible-looking order rather than a degenerate one: `partSellAmount = 1`, `n = 2^128` gives `total = 2^128` and a ceiling of `2^129 - 1` — functionally unlimited for any token with 18 decimals, while every field still looks like a real TWAP.

Control: the same batch with `partSellAmount = 10`, `n = 3`, approval `33` must return `insecure R-4.5` (this is `cow.rs:927-943`'s existing unit expectation), so the corpus pair isolates the `n`-scaling.

## Considered and rejected

- **`ExcessiveApprovalChecker` denies first.** It runs 6th, ahead of Cow, but only on a literal `U256::MAX` (`excessive_approval.rs:22`); `2^256 - 2` abstains.
- **`checked_mul` blocks the degenerate order.** It blocks only genuine overflow. `partSellAmount = 0` times any `n` is `0`, and `partSellAmount = 1` times `2^128` is exactly representable.
- **ComposableCoW rejects `n = 2^256-1` on-chain, so the order is not real.** `createWithContext` registers the conditional order; validation of the TWAP parameters happens later, when `getTradeableOrder` is called. Even if the order were unusable, the **approval in entry 1 executes and persists** — that is the standing authority R-4.5 exists to prevent, and the engine has affirmed it.
- **The relayer can only spend against valid CoW orders, so the allowance is harmless.** That is a mitigation argument about CoW's design, not about the Charter rule; R-4.5 is written about the grant, not about whether the grantee is currently trustworthy. It is why this is Medium rather than High.
- **The tolerance is needed for real orders.** It is, for small `n`. The defect is that it scales linearly with a field the proposer chooses; a cap on `n` (or on the absolute headroom) keeps the rounding allowance without the scaling.

## Remediation options

1. Bound the headroom independently of `n`: `min(n - 1, SOME_ABSOLUTE_CAP)`, or reject orders whose `n` exceeds a plausible part count (the Safe app's UI caps parts far below 2^64).
2. Reject degenerate orders outright: require `partSellAmount > 0` and `n > 0` in `twap_order_terms` before computing a total, so a zero-total order abstains rather than affirming an arbitrary approval.
3. Compare against the approval instead: require `approved_amount >= total` _and_ `approved_amount - total < n` _and_ `n <= MAX_PARTS`, which preserves the rounding argument exactly and fails closed on everything else.
4. Fix F-ENG-036's `U256::MAX`-only test as well, so an earlier checker catches the extreme cases regardless of the CoW path.

Tests to add: unit tests for `partSellAmount = 0, n = U256::MAX` and for `partSellAmount = 1, n = 2^128` (the existing suite covers only `n = 3` — `cow.rs:879-943`); a corpus vector for the degenerate order. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 83%. (Confirms lead ENG-H8. Cited lines re-read at commit 2893917. Impact on funds depends on CoW's settlement design, which is not in this checkout — that part is class I and severity is set at Medium accordingly; the false `secure` itself is E2.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts — Supported, and I re-derived the arithmetic before reading R9's

`cow.rs:552-554`:

```rust
fn max_approval_for_twap_total(total_sell_amount: U256, n: U256) -> U256 {
    total_sell_amount.saturating_add(n.saturating_sub(U256::from(1)))
}
```

and `twap_order_terms` (`cow.rs:531-542`) takes **both** `total` and `n` from the same attacker-supplied `staticInput`: `let total = order.partSellAmount.checked_mul(order.n)?; Some((order.sellToken, order.receiver, total, order.n))`. So the tolerance is sized by a value the proposer chooses, exactly as claimed. With `partSellAmount = 0` the `checked_mul` cannot overflow, `total = 0`, and `max = 0 + (U256::MAX - 1) = U256::MAX - 1`. The gate at `cow.rs:375-381` is `approved_amount > max_approval_for_twap_total(...)`, so an `approve` of `2^256 - 2` passes and `cow.rs:382` returns `Verdict::Secure`.

The tolerance's stated justification is `floor(desired_total / n)` truncation, and the code's own comment (`cow.rs:544-551`) states that assumption while enforcing none of it: nothing requires `n` to be small, `partSellAmount` to be non-zero, or the two to be mutually consistent.

### 2. The three preconditions I checked because the finding fails without them

1. **`ExcessiveApprovalChecker` must not fire first.** It is 6th, `CowChecker` 7th (`main.rs:63-64`), and it denies only `amount == U256::MAX` (`excessive_approval.rs:22`). `2^256-2` is not `U256::MAX`, so it abstains. Confirmed — and note this makes the finding _dependent on_ F-ENG-036: pick the largest value below `U256::MAX` and both checks are evaded simultaneously.
2. **`BaseChecker` must not deny the MultiSend wrapper.** A 2-call batch is necessarily an `Operation:: DelegateCall` to a known MultiSend (`contracts/multi_send.rs:143-155`), so `check_delegatecall_integrity` runs; `check_multi_send` (`base.rs:205-213`) passes because both sub-calls are plain `Call`s to addresses other than the Safe and `check_calls` returns `true` at `base.rs:91-93`. Confirmed — BaseChecker abstains.
3. **`check_dangling_approval` must not deny first.** It runs ahead of `check_twap_batch` inside `CowChecker` (`cow.rs:431-446`), but abstains because `is_twap_create` matches the decoy (`cow.rs:392-396`). Confirmed.

All three hold. The vector is genuinely constructible.

### 3. Severity — raised from Medium to High

R9 rates this Medium; I raise it to **High**, and I want to be explicit that a Critic raising a severity is a considered judgement, not deference. The outcome is not a missed denial — it is `Verdict::Secure` on an `approve(GPv2VaultRelayer, 2^256-2)`, i.e. an affirmative, bonded attestation that a functionally unlimited allowance to a live third-party spender is safe. Charter §2.5:154 and R-4.5:639 make that transaction insecure; §3.7:439 forbids calling it secure. Once the allowance exists, extracting the funds needs no further Safe transaction and therefore never passes the guard again — the approval _is_ the breach. Under PROMPT.md §8 that is the "malicious Safe transaction rated `secure`" bullet, which nominally reads Critical; I settle on High rather than Critical only because the spender is CoW's genuine `GPv2VaultRelayer` rather than an attacker address, so realising the loss still requires the attacker to drive a settlement through CoW's own protocol. Medium understates it either way.

### 4. Trigger quality

The best-specified trigger in R9's set: it names the exact `staticInput` fields, the required `CurrentBlockTimestampFactory` and TWAP handler, `receiver = safe`, and the two amounts. Two additions QA needs that R9 omitted, both of which will otherwise make the vector abstain rather than affirm:

- `chainId` must be 1, 100 or 42161 (`cow.rs:106`, `:422-427`); the whole checker abstains otherwise.
- The batch must be wrapped in a **known** MultiSend deployment — e.g. `0x218543288004CD07832472D464648173c77D7eB7` (`contracts/multi_send.rs:29`) — with `operation = 1`; any other `to` makes `sub_transactions` return the single outer transaction (`multi_send.rs:168-172`) and the 2-call match at `cow.rs:352-354` fails.
- `receiver` may be `safe` **or** zero; `cow.rs:370` accepts both.

### 5. Finding verdict

**Confirmed.** Certainty **84%**. Severity **High** (raised from Medium). Depends on F-ENG-036; the two should be read and remediated together.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection**, including the arithmetic. `twap_order_terms` (`cow.rs:531-542`) computes `total = order.partSellAmount.checked_mul(order.n)?`, which for `partSellAmount = 0` is `Some(0)` — `checked_mul` does not overflow on a zero operand, so the degenerate order is _not_ rejected by the overflow guard the comment at `:528-530` describes. `max_approval_for_twap_total(0, U256::MAX)` (`cow.rs:552-554`) is `0.saturating_add(MAX.saturating_sub(1))` = `2^256 - 2`, and `cow.rs:375-383` denies only on `approved_amount > max`, so an approval of exactly `2^256 - 2` is affirmed at `:382`. `handler` and `factory` are both enforced (`:536`), so those cannot be substituted — the attacker's freedom is entirely in `partSellAmount` and `n`. Not `E1`.

**Certainty unchanged at 84%. Severity unchanged at High.**

**PoC: `rust-audit/poc/F-ENG-037/`** — five tests appended to `cow.rs` as a sibling of its own suite (so it re-declares the `pack`/`multisend`/`twap_create_data` helpers, which are private to that module; the file-level constants `GP_V2_VAULT_RELAYER`, `TWAP_HANDLER`, `CURRENT_BLOCK_TIMESTAMP_FACTORY`, `COMPOSABLE_COW` do come through `use super::*`). **No HTTP is issued** — the presignature path requires a `setPreSignature` call in the batch (`cow.rs:283-293`) and this batch has none — so `CowChecker::new` is safe offline. Two tests are expected to fail on unfixed code.

**The PoC carries both variants and keeps them separate on purpose**: the degenerate order (`partSellAmount = 0`, `n = 2^256-1`) and the plausible one (`partSellAmount = 1`, `n = 2^128`, ceiling `2^129 - 1`). **Option 2 closes only the first.** The plausible variant is the one worth putting in a corpus, because every field still reads like a real TWAP order.

### Remediation check

- **Option 1 (`min(n - 1, SOME_ABSOLUTE_CAP)`, or reject an implausible part count) — sound, and the smallest change that closes both variants.** Worth framing correctly in the fix: the tolerance's justification (`cow.rs:544-551` — the Safe app computes `partSellAmount = floor(desired_total / n)`, so up to `n - 1` units are truncated away) is valid _only_ for `n` in the range the Safe app produces. Capping `n` restores the argument's own precondition rather than working around it. Derive the cap from the Safe app's UI limit and say so in the comment.
- **Option 2 (require `partSellAmount > 0` and `n > 0`) — sound but insufficient**, and this PoC proves it: the plausible variant has both non-zero and internally consistent. Do not close the finding on it.
- **Option 3 (`approved >= total` and `approved - total < n` and `n <= MAX_PARTS`) — sound and most faithful, but it contains a policy change the finding does not flag.** Adding `approved >= total` reverses `cow.rs:348-350`'s deliberate position that "An approval _smaller_ than that total is a trade-soundness concern (the order may not fully fill), not a security one, so it doesn't affect this verdict either way". Under option 3 an under-approving but otherwise genuine batch stops being affirmed — a change in the **denying** direction against honest traffic, which this audit has now established exposes the sentinel's bond (`contracts/src/libraries/SentinelOracleRequests.sol:301-304`; see F-ENG-042's `## QA`). **Recommend adopting `approved - total < n` together with `n <= MAX_PARTS` and omitting the `approved >= total` clause**, keeping under-approval unopinionated as today.
- **Option 4 (fix F-ENG-036's `U256::MAX`-only equality) — defence in depth, not a fix here.** It catches the degenerate variant at position 6 before `CowChecker` runs, but not the plausible one.
- **Test hook: exists, and is unusually good.** `CowChecker::with_order_api` (`cow.rs:242`) covers the network path and the TWAP path needs nothing at all; `cow.rs:641-1407` is the crate's largest suite and simply covers only `n = 3`. This is the one engine finding where "the corpus is the oracle" is a defensible position — a corpus vector _can_ express it — and where the in-process test is nevertheless plainly cheaper.
- **Where the fix belongs: the checker** (`check_twap_batch` / `max_approval_for_twap_total`). The `RuleId` mapping is fine.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 84% -> **96%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under `rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-037/append-to-src-checkers-cow.rs` appended to `crates/sentinel-engine/src/checkers/cow.rs`, then `cargo test -p sentinel-engine poc_f_eng_037`. Compiled on the first attempt. No HTTP is issued: the presignature path requires a `setPreSignature` call in the batch and these batches have none, so `CowChecker::new` runs offline. Full output: `rust-audit/poc/F-ENG-037/run-output.txt`.

### Verbatim result

```
running 5 tests
test checkers::cow::poc_f_eng_037::poc_f_eng_037_a_degenerate_order_must_not_size_the_tolerance ... FAILED
test checkers::cow::poc_f_eng_037::poc_f_eng_037_a_large_part_count_must_not_size_the_tolerance ... FAILED
test checkers::cow::poc_f_eng_037::poc_f_eng_037_the_tolerance_is_sized_by_n ... ok
test checkers::cow::poc_f_eng_037::poc_f_eng_037_control_a_real_order_behaves_as_documented ... ok
test checkers::cow::poc_f_eng_037::poc_f_eng_037_affirms_a_near_unlimited_relayer_approval_today ... ok

---- ..._a_degenerate_order_must_not_size_the_tolerance stdout ----
assertion `left == right` failed
  left: Secure
 right: Insecure { rule: R4_5ExcessiveApproval }

---- ..._a_large_part_count_must_not_size_the_tolerance stdout ----
assertion `left != right` failed
  left: Secure
 right: Secure

test result: FAILED. 3 passed; 2 failed; 0 ignored; 0 measured; 98 filtered out
```

Both expected-to-fail tests failed for the claimed reason.

### What this establishes

- **The arithmetic** (test 1, passed): `max_approval_for_twap_total(U256::ZERO, U256::MAX)` evaluates to `U256::MAX - 1`. An order selling **zero** tokens tolerates an approval of `2^256 - 2`.
- **It is reachable through the real checker** (test 2, passed, then test 3 FAILED): a genuine 2-call MultiSend batch — `approve(GPv2VaultRelayer, 2^256 - 2)` plus a TWAP `createWithContext` using the **canonical** `TWAP_HANDLER` and `CURRENT_BLOCK_TIMESTAMP_FACTORY` (both enforced at `cow.rs:536`, so neither can be substituted) with `partSellAmount = 0`, `n = U256::MAX`, `receiver = safe` — is rated `Secure`.
- **It does not need a degenerate-looking order** (test 4, FAILED): `partSellAmount = 1`, `n = 2^128` gives `total = 2^128` and a ceiling of `2^129 - 1`, and an approval of `2^129 - 1` is affirmed. Every field reads like a real TWAP. This matters for remediation: option 2 alone (reject `partSellAmount == 0`) closes test (3) and **leaves test (4) open** — only a bound on `n` itself (option 1 or 3) closes both.
- **The documented behaviour is intact** (control, test 0, passed): a real 3-part order selling 30 units is affirmed when approved for exactly 30 and denied `R-4.5` when approved for 1000. A fix must not disturb this.

Residual uncertainty: the Charter §2.5/R-4.5 mapping under A7/A15, and A2 for the attacker's control of the `staticInput`. The checker's behaviour is settled.
