# F-ENG-038 CoW shape recognisers accept batches their paired decoders reject, turning a dangling relayer approval from `insecure` into `abstain`

| Field | Value |
| --- | --- |
| Status | QA-done |
| Crate and module | sentinel-engine, checkers/cow.rs |
| Location | crates/sentinel-engine/src/checkers/cow.rs:478-499 vs :531-542, :561-569 (related: cow.rs:388-402, :421-449) |
| Severity | Low / Low |
| Certainty | 78% (Critic C-ENG-B; E2 ceiling, read-only run) |
| Assumptions involved | A2, A3 |
| Tags | verdict-policy, input-validation |

## Claim

`check_dangling_approval` decides whether an approval to `GPv2VaultRelayer` has a co-batched "trigger" using `is_presignature` / `is_twap_create`. Those two predicates are strictly looser than the decoders that the subsequent checks use:

| Predicate (suppresses the denial) | Paired decoder (must succeed to reach a verdict) | Gap |
| --- | --- | --- |
| `is_presignature` — decodes `setPreSignature` and stops (`cow.rs:478-483`) | `presignature_order_uid` — additionally requires `signed == true` (`cow.rs:561-569`) | `setPreSignature(uid, false)` |
| `is_twap_create` — checks `handler` and `factory` only (`cow.rs:492-499`) | `twap_order_terms` — additionally requires `TwapData::abi_decode(staticInput)` to succeed and `partSellAmount * n` not to overflow (`cow.rs:531-542`) | malformed `staticInput`; `partSellAmount * n` overflow |

A proposer can therefore batch a relayer approval with a _decoy_ trigger that satisfies the loose predicate and fails the strict decoder. `check_dangling_approval` sees a trigger and abstains; `check_presignature_batch` and `check_twap_batch` both fail to decode and abstain; the whole `CowChecker` abstains. The standalone-approval denial the module's own docs describe ("**Denies, rather than exempts**") is silently skipped, and the approval — which does execute on-chain, since neither an unset presignature nor a registered-but-invalid conditional order reverts — stands unremarked.

This is a missed denial, not a false `secure`: the chain continues to `StakingChecker`, `RefundChecker` and `AddressPoisoningChecker`. The last of those can then affirm if the spender has prior history (F-ENG-036), so in combination the two defects do produce a `secure` on a dangling relayer approval.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | `is_presignature` does not look at `signed` | E2 | crates/sentinel-engine/src/checkers/cow.rs:478-483 | Q1 |
| 2 | `presignature_order_uid` requires `signed == true` | E2 | crates/sentinel-engine/src/checkers/cow.rs:561-569 | Q2 |
| 3 | `is_twap_create` does not decode `staticInput` | E2 | crates/sentinel-engine/src/checkers/cow.rs:492-499 | Q3 |
| 4 | `twap_order_terms` requires `staticInput` to decode and the product not to overflow | E2 | crates/sentinel-engine/src/checkers/cow.rs:535-542 | Q4 |
| 5 | The presence of either loose predicate suppresses the dangling-approval denial | E2 | crates/sentinel-engine/src/checkers/cow.rs:388-402 | Q5 |
| 6 | All three sub-checks abstain independently, so the whole checker abstains | E2 | crates/sentinel-engine/src/checkers/cow.rs:429-448 | Q6 |

**Q1** `crates/sentinel-engine/src/checkers/cow.rs:478-483`

```rust
fn is_presignature(tx: &SafeTransaction) -> bool {
    tx.operation == Operation::Call
        && tx.value.is_zero
        && tx.to == GP_V2_SETTLEMENT
        && setPreSignatureCall::abi_decode(&tx.data).is_ok
}
```

**Q2** `crates/sentinel-engine/src/checkers/cow.rs:561-569`

```rust
fn presignature_order_uid(tx: &SafeTransaction) -> Option<Bytes> {
    if tx.operation != Operation::Call || !tx.value.is_zero || tx.to != GP_V2_SETTLEMENT {
        return None;
    }
    setPreSignatureCall::abi_decode(&tx.data)
        .ok
        .filter(|call| call.signed)
        .map(|call| call.orderUid)
}
```

**Q3** `crates/sentinel-engine/src/checkers/cow.rs:492-499`

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

**Q4** `crates/sentinel-engine/src/checkers/cow.rs:535-542`

```rust
    let create = createWithContextCall::abi_decode(&tx.data).ok?;
    if create.params.handler != TWAP_HANDLER || create.factory != CURRENT_BLOCK_TIMESTAMP_FACTORY {
        return None;
    };
    let order = TwapData::abi_decode(&create.params.staticInput).ok?;
    let total = order.partSellAmount.checked_mul(order.n)?;
    Some((order.sellToken, order.receiver, total, order.n))
}
```

**Q5** `crates/sentinel-engine/src/checkers/cow.rs:388-402`

```rust
    fn check_dangling_approval(&self, calls: &[SafeTransaction]) -> Verdict {
        if !calls.iter.any(approves_vault_relayer) {
            return Verdict::Abstain;
        }
        if calls
            .iter
            .any(|c| is_presignature(c) || is_twap_create(c))
        {
            Verdict::Abstain
        } else {
            Verdict::Insecure {
                rule: RuleId::R4_4AuthorizationTarget,
            }
        }
    }
```

**Q6** `crates/sentinel-engine/src/checkers/cow.rs:429-441` (`:443-448` repeats the same pattern for `check_twap_batch`, and `:448` is the final `Verdict::Abstain`)

```rust
        let calls = sub_transactions(transaction);

        let dangling_check = self.check_dangling_approval(&calls);
        if dangling_check != Verdict::Abstain {
            return dangling_check;
        }

        let presig_check = self
            .check_presignature_batch(transaction.safe, transaction.chain_id, &calls)
            .await;
        if presig_check != Verdict::Abstain {
            return presig_check;
        }
```

## Trigger

**Trigger A — unset presignature.** MultiSend delegatecall to a canonical deployment, `chainId` 1, two packed `Call` entries:

1. `to = <token>`, `data = approve(0xC92E8bdf79f0507f65a392b0ab4667716BFE0110, 1000000000000000000000)`;
2. `to = 0x9008D19f58AAbD9eD0D60971565AA8510560ab41` (GPv2Settlement), `data = setPreSignature(<any 56-byte uid>, false)`.

Expected: `insecure R-4.4` (the same verdict `cow.rs:1054-1064`'s `denies_a_batched_approval_without_a_recognized_trigger` asserts for the approval on its own). Actual: `abstain`.

**Trigger B — malformed `staticInput`.** Entry 2 replaced with `to = 0xfdaFc9d1902f4e0b84f65F49f244b32b31013b74` (ComposableCoW), `data = createWithContext({handler: 0x6cF1e9cA41f7611dEf408122793c358a3d11E5a5, salt: 0x00…00, staticInput: 0x00}, 0x52eD56Da04309Aca4c3FECC595298d80C2f16BAc, 0x, true)` — a one-byte `staticInput` that `TwapData::abi_decode` rejects. Same expected/actual as A. (`cow.rs:1149-1173` already builds this exact shape with an _empty_ `staticInput`, but pairs it with a delegatecall so a different branch denies it.)

**Trigger C — overflow.** As B but `staticInput` a well-formed `TwapData` with `partSellAmount = 2^255` and `n = 3`, so `checked_mul` returns `None`. Same expected/actual.

**Trigger D — combined false `secure`.** Trigger A with the approval's spender replaced by an address with prior `Transfer`/`Approval` history from the Safe on that token, and the batch flattened to a single non-batched `approve` so `AddressPoisoningChecker::decode_target` accepts it — then the chain reaches `address_poisoning.rs:332` and answers `secure`. (This is the F-ENG-036 interaction; listed here because the CoW abstention is what lets the chain get that far.)

Corpus shape: A, B and C as request/response pairs, each with a control that fixes the decoy (`signed: true`, a valid `TwapData`) and therefore reaches a real verdict.

## Considered and rejected

- **`ExcessiveApprovalChecker` denies the approval first.** Only if the amount is literally `U256::MAX` (`excessive_approval.rs:22`).
- **The on-chain batch reverts, so the approval never lands.** `setPreSignature(uid, false)` is a valid call that clears (rather than sets) a presignature and does not revert; `createWithContext` registers a conditional order without validating `staticInput` against the handler. Neither aborts the MultiSend, so entry 1's approval persists. (Class I for the CoW contracts' revert behaviour — they are not in this checkout — but the engine-side claim does not depend on it.)
- **This is the same defect as F-ENG-037.** No: F-ENG-037 is about the _tolerance_ on a well-formed TWAP batch producing a false `secure`; this is about _shape recognition_ suppressing a denial the checker would otherwise make.
- **Abstain is the safe answer for an unrecognised shape.** It is, in general. The specific problem is that `check_dangling_approval`'s job is to deny exactly the unrecognised shapes, and the attacker can move a batch from "unrecognised, therefore denied" to "unrecognised, therefore abstained" by adding a decoy call.

## Remediation options

1. Define the trigger predicates in terms of the decoders: `is_presignature(tx)` becomes `presignature_order_uid(tx).is_some` and `is_twap_create(tx)` becomes `twap_order_terms(tx).is_some`. Two-line change; makes the loose/strict pairs impossible to drift apart again.
2. Invert the control flow: run `check_presignature_batch` and `check_twap_batch` first, and let `check_dangling_approval` deny whenever a relayer approval is present and neither reached a verdict. Same effect, and removes the duplicate recognition logic entirely.
3. If some shapes must stay abstaining, log a distinct `warn!` when a relayer approval is present but no check could conclude, so the case is at least observable.

Tests to add: `setPreSignature(uid, false)` co-batched with a relayer approval; a canonical-handler `createWithContext` with malformed `staticInput` co-batched with one (the existing suite pairs both only with a delegatecall or a value-carrying entry, which take a different branch — `cow.rs:1129-1173`). No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 84%. (Confirms lead ENG-H9; cited lines re-read at commit 2893917; not executed — A9 is FALSE this run.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts — the recogniser/decoder divergence is real

**Claims 1 and 2 — Supported.** `is_presignature` (`cow.rs:478-483`) is `tx.operation == Operation::Call && tx.value.is_zero && tx.to == GP_V2_SETTLEMENT && setPreSignatureCall::abi_decode(&tx.data).is_ok` — it never reads `signed`. `presignature_order_uid` (`cow.rs:561-569`) adds `.filter(|call| call.signed)`. The gap is exactly `setPreSignature(uid, false)`.

**The TWAP row — Supported.** `is_twap_create` (`cow.rs:492-499`) stops at `handler` and `factory`; `twap_order_terms` (`cow.rs:531-542`) additionally requires `TwapData::abi_decode(&create.params.staticInput)` to succeed and `partSellAmount.checked_mul(order.n)` not to overflow. A `createWithContext` with the canonical handler and factory but a malformed `staticInput` satisfies the first and fails the second.

**The consequence — Supported.** `check_dangling_approval` (`cow.rs:388-402`) abstains as soon as any call satisfies `is_presignature(c) || is_twap_create(c)`; `check_presignature_batch` and `check_twap_batch` then both abstain on the decode failure (`cow.rs:288-293`, `:360-365`); `CowChecker::check` falls through to `Verdict::Abstain` (`cow.rs:448`). The `Insecure { R4_4AuthorizationTarget }` at `cow.rs:398-400` is skipped.

### 2. One sub-claim marked `H` — the combination paragraph is contradicted by the code

R9's Claim closes with:

> The last of those can then affirm if the spender has prior history (F-ENG-036), so in combination the two defects do produce a `secure` on a dangling relayer approval.

**This is false, and the counter-evidence is two lines.** The decoy scenario requires a two-call batch, and a two-call batch only exists when `decode_multi_send_call` succeeds, which requires `tx.operation == Operation::DelegateCall` (`contracts/multi_send.rs:143-151`):

```rust
    if tx.operation != Operation::DelegateCall {
        …
        return None;
    }
```

`AddressPoisoningChecker::decode_target` then refuses the transaction outright (`address_poisoning.rs:116-121`):

```rust
    // A `DelegateCall`'s `to` isn't necessarily even a token contract, so
    // events queried against it would be meaningless.
    if tx.operation != Operation::Call {
        return None;
    }
```

so `AddressPoisoningChecker` returns `Verdict::Abstain` at `:309-311` for _every_ batched transaction and can never affirm one. The engine's answer to the decoy vector is `Abstain`, full stop. Per the brief this sub-claim is class **`H`** — it contradicts the code. The finding does **not** depend on it (R9's own title is "turning a dangling relayer approval from `insecure` into `abstain`"), so the verdict is unaffected; the paragraph should be struck when the report is compiled.

### 3. Severity — Low confirmed

`Insecure → Abstain` is a fail-closed degradation: `Abstain` becomes `CheckOutcome::Unknown` and the sentinel drops the request unvoted (`crates/sentinel/src/engine.rs:175`, `crates/sentinel/src/service.rs:176-179`), so no attestation is produced and the Guard still refuses the transaction. The attacker gains no execution. What is lost is the _on-chain denial record_ — the bonded `insecure R-4.4` vote and its reason string — which degrades the evidence trail and any later analysis grouped by rule, the same class of harm as F-ENG-040. **Low** is right; I considered Medium and rejected it because nothing reachable turns this into an approval.

### 4. Trigger quality — the strongest in R9's set

`setPreSignature(<any 56 bytes>, false)` batched with the approval is a one-line decoy with no state dependencies, no RPC and no chain requirements beyond `chainId ∈ {1, 100, 42161}` (`cow.rs:422-427`) and a known MultiSend wrapper (`contracts/multi_send.rs:27-68`). Directly encodable as a corpus vector today. QA should encode both rows of R9's table; the `signed == false` one is cheaper and needs no malformed-ABI construction.

### 5. Finding verdict

**Confirmed**, with the combination sub-claim struck as `H`. Certainty **78%**. Severity **Low** (unchanged).

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the finding's effect is a downgrade from `insecure` to `abstain`, i.e. a _missing_ denial rather than a wrong affirmation, and `cow.rs` already has the crate's largest test suite with all the helpers needed — the two cases the finding names are ~10 lines each in the existing `mod tests` and are better added by whoever takes the fix than transplanted from a separate directory. `rust-audit/poc/F-ENG-037/` shows the pattern for appending CoW fixtures if a standalone module is wanted.

**Certainty unchanged at 78%. Severity unchanged at Low.**

### Remediation check

- **Option 1 (define the trigger predicates in terms of the decoders) — sound, two lines, and it is the fix.** `is_presignature(tx)` becomes `presignature_order_uid(tx).is_some` and `is_twap_create(tx)` becomes `twap_order_terms(tx).is_some`. The value is not that it fixes the two known shapes but that it makes the loose and strict recognisers **the same code**, so they cannot drift apart again — which is the actual defect. Take this.
- **Option 2 (invert the control flow) — sound and cleaner still**, since it removes the duplicate recognition logic rather than reconciling it: run `check_presignature_batch` and `check_twap_batch` first, and let `check_dangling_approval` deny whenever a relayer approval is present and neither concluded. **One consequence the finding does not name:** `check_presignature_batch` performs an **HTTP lookup** (`cow.rs:294`), so running it before the purely-local `check_dangling_approval` moves a network call earlier in the request for batches that would have been decided locally. With no client timeout today (F-ENG-005, F-ENG-043) that is a real latency regression. **Option 2 is right, but it should land after F-ENG-005 option 1.** Until then, option 1 is the safe choice.
- **Option 3 (a distinct `warn!` when a relayer approval is present but no check concluded) — sound and worth taking regardless of 1 or 2**, because it makes the abstain-ambiguity visible: today "no relayer approval" and "a relayer approval nobody could reason about" are the same silent `Abstain`. This is the cheapest instance of the general abstain-ambiguity point that also appears in F-ENG-041.
- **Test hook: excellent, and unused for these shapes.** `cow.rs:641-1407` has every helper needed. The finding is precise about why the existing suite misses it: the loose/strict pairs are only ever exercised with a delegatecall or a value-carrying entry (`cow.rs:1129-1173`), which take a different branch. Add `setPreSignature(uid, false)` co-batched with a relayer approval, and a canonical-handler `createWithContext` with malformed `staticInput` likewise.
- **Where the fix belongs: the checker** (`cow.rs`). Not the combinator or the `RuleId` mapping.
