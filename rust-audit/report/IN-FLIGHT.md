# In-flight impact assessment — the "Batched Execution" (batex) PR stack

**Assessor:** FWD. **Date:** 2026-09-11. **Audit HEAD:** `a7f3915` (audit ran at `2893917`, re-validated against `origin/main` = `49d7e39`, which has not moved).

This document assesses **unmerged** code. Nothing here is a finding against `main`. No branch was merged, checked out, rebased or modified; everything below was read with `git show` / `git diff` against `origin/...` refs.

## The stack

| PR | Branch → base | Title | Assessed at |
| --- | --- | --- | --- |
| #899 | `feat/batex_0` → `main` | [Phase 0] Add Epic for Batched Execution | — |
| #900 | `fix/batex_1` → `feat/batex_0` | [Phase 1] Adjust 7702 batching contract | contract |
| #902 | `feat/batex_2` → `fix/batex_1` | [Phase 2] Adjust config | config |
| #903 | `feat/batex_3` → `feat/batex_2` | [Phase 3] Add authorisation signing support | signer/types |
| #904 | `feat/batex_4` → `feat/batex_3` | [Phase 4] Adjust nonce handling | storage/mod |

The epic (`epics/2026_09_09_safenet_7702_executor_tx_batching.md`, on `feat/batex_0`) plans **ten** phases. **The stack stops at Phase 4.** Phases 5 (enqueue the delegation on start), 6 (the batch builder `tx/executor.rs`), 7 (wire batching into `queue()`), 8–10 are **not on any pushed branch**.

The single most important consequence of that for this assessment: **on `feat/batex_4` nothing ever sets `Transaction::authorization`, so Phases 3 and 4 are behaviourally inert.** Every new code path in the diff — 7702 signing, the two-nonce reservation, `pending_delegation`, the authorization self-check — is dead code today. The risks below are therefore _forward-looking_: they are what the stack will do once Phase 5 arms it, and they are unreachable as the branches stand.

## Summary table

Effect key: **fix** / **worsen** / **unchanged** / **reshape** (impact changes character without a clear net improvement).

| Finding | Sev / Cert | Effect of the batex stack | Note |
| --- | --- | --- | --- |
| `F-CORE-060` (fee ratchet unbounded) | High / 98% | **unchanged**, with a new trigger | `fees.rs` is not in the diff; `fees::bump` is byte-identical. `submit_transaction`'s rearrangement (`bumped_fees` before `build`) is a pure refactor producing the same `Submission`. **But** `F-CORE-069`'s permanent nonce gap strands transactions in flight where the ratchet runs on them without bound, and manually repairing the gap releases the whole backlog at the ratcheted fee. |
| `F-CORE-061` (first-submission rejection retries forever) | Medium / 58% | **unchanged** | `is_transaction_underpriced` and its regexes are untouched; the generic branch still records no fee floor. Batching would reduce the _number_ of transactions exposed to it but raise the cost of each one wedging (a batch, not an action). |
| `F-CORE-062` (nonce never released, queue wedges) | High / 60% | **worsen** | Phase 4 keeps `MAX(chain, high_water + span)` — it still never allocates into a gap — and now _creates_ gaps deliberately. The epic's own acknowledged failure path (authorization does not apply) is a new, first-party way to reach exactly this defect. Filed as `F-CORE-069`. |
| `F-CORE-063` (execution inferred from account nonce) | Medium / 55% | **worsen** (blast radius) | `mark_executed` is untouched, still a pure nonce comparison. Under batching the inference covers a transaction carrying 6–8 actions, and Phase 1's `InsufficientGas` guard adds a **whole-batch revert** that is indistinguishable from success to the queue. Filed as `F-CORE-068`. The delegation transaction itself is also declared executed purely from a nonce advance. |
| `F-CORE-064` (`expires_at` void once a nonce is allocated) | Medium / 72% | **reshape** | The delegation is specified with `expires_at: None` _by design_, so the epic institutionalises the never-expiring row this finding is about — and makes it load-bearing ("never dropped or pruned while unexecuted"). Batches split on any `expires_at` change, so no batch inherits a wrong deadline, but a late-landing batch now lands _all_ of its stale actions at one nonce. |
| `F-CORE-065` (no chain-id / signer binding) | Low / 55% | **worsen** | The EIP-7702 authorization's `chain_id` comes from the same connect-time `Provider::chain_id()` cache (`signer.rs`, `types.rs::build`). The cache is now load-bearing for a _code delegation_, not just one transaction: a stale cached id produces an authorization the chain rejects, which is precisely `F-CORE-069`'s gap. The stored `request` JSON gains an `authorization` field naming an executor **address with no chain binding**, so a database moved between chains now carries a delegation target too. (No cross-chain _replay_ risk: the epic correctly uses the real chain id rather than `0`.) |
| `F-CORE-066` (`tx::Config` unvalidated) | Medium / 78% | **unchanged, surface extended** | Two new keys, neither validated: `max_batch_gas: u64` accepts `0` (and anything below the ~26 000 base, which silently disables batching via the "cannot fit in an empty batch" pass-through), and `executor: Option<Address>` accepts the **zero address** — which the sample TOML ships as its literal example value. Phase 5's planned `eth_getCode(executor)` check would catch that, but Phase 5 is not in this stack, so today the field is accepted with no check whatsoever. `deny_unknown_fields` and `#[serde(default)]` are preserved. |
| `F-CORE-067` (no idempotency key on `enqueue`) | Critic / 98% | **unchanged in kind, worsen in blast radius** | `enqueue` is still the identical unconditional `INSERT INTO transactions (request, expires_at)` with no idempotency key, no content hash, no `ON CONFLICT`, no unique constraint; the table DDL is unchanged apart from a new **`transactions_nonce_idx` index on `nonce` only** — not a uniqueness constraint and not an idempotency key. `SnapshotStore::reorg` still never touches `transactions`. The +186 lines in `storage.rs` are the nonce span, two delegation helpers, an index and ~120 lines of tests. Once Phase 7 lands, a replay produces **two duplicate batches** — the same defect multiplied by the batch size. Phase 5's `queue_delegation` idempotency also rests on `has_delegation`, a read-then-write with no constraint behind it, so the delegation is itself duplicable. |
| `F-VAL-063` (unvalidated consensus config, unsafe defaults) | Medium / 72% | **unchanged** | `validator.sample.toml` on `feat/batex_4` still ships `genesis_salt = "0x00…00"` uncommented and `oracles` commented out; `crates/validator/src/config.rs` gains only two assertions in an existing test. |
| `F-XC-009` (samples teach dangerous values) | Low / 72% | **unchanged, one new instance** | Both samples on `feat/batex_4` still carry `rpc = "https://rpc.gnosischain.com"` (live mainnet) and `signer = "0x00…01"` (the well-known key), and `metrics_address = "0.0.0.0:3555"`. The new block adds a **third** dangerous placeholder: `# executor = "0x0000000000000000000000000000000000000000"`. It is commented out, but under EIP-7702 the zero address is the _delegation-clearing_ target and a codeless address re-opens the epic's own "every batched action silently no-ops" mode. |
| `F-VAL-065` (validator actions not deduplicated) | Low / 70% | **unchanged, worsen downstream** | `action.rs`'s +12 lines are twelve `authorization: None` initialisers — a mechanical struct-field addition, no encoder logic changes, no expiry changes. `SetValidatorStaker` and `Preprocess` still carry `None` expiry. The finding's compounding note ("a duplicate that reverts … is recorded as executed and never retried") is exactly what `F-CORE-068` amplifies to batch scale. A duplicate `Sign` batched with other actions still burns a nonce sequence for the whole group. |
| `F-SEN-006` (sentinel actions not idempotent under replay) | Low / 85% | **unchanged, worsen downstream** | `service.rs`'s +5 lines are five `authorization: None` initialisers in `SentinelEncoder`. All five duplicate-producing paths are untouched. Batching makes each replay produce a duplicate _batch_; and because `execute` swallows the `AlreadyCommitted` / `AlreadyRevealed` reverts as `CallFailed`, the duplicates' reverts stop being visible even in principle. |
| `F-SEN-007` (no balance/allowance/registration pre-check) | Medium / 80% | **worsen** | Still no `eth_call` or estimation before broadcast. With batching, the per-request burn becomes a batch that _succeeds_ while every call in it reverts into a swallowed `CallFailed` on the sentinel's own EOA — an address nothing indexes. The finding's stated symptom ("no error visible beyond the absence of `Committed` events") gets strictly harder to diagnose. Note also that `execute` does **not** stop on a failed `ApproveToken`: the following `Commit` runs anyway and reverts for want of allowance. |

**Net: nothing in the batex stack fixes any existing finding.**

## New defects filed

Both are marked **UNMERGED** in their title and Location field, naming the branch and PR.

- **`F-CORE-068`** (High / Medium, 85%) — _A batch's execution status is unobservable, so a whole-batch revert or a mid-batch `CallFailed` silently drops up to a full batch of actions._ Three outcomes (success, swallowed `CallFailed`, whole-batch `InsufficientGas` revert) all advance the nonce and are therefore identical to `mark_executed`. `prune` then destroys the record within `max_reorg_depth` blocks. Phase 1's new gas guard is genuinely louder onchain and **no louder offchain**, while enlarging the unit of loss from one action to a whole batch. Applies to `origin/feat/batex_4` (PR #904) plus the planned Phases 6–7.
- **`F-CORE-069`** (High / Medium, 80%) — _The two-nonce delegation reservation writes a permanent nonce gap into durable state on the epic's own acknowledged failure path, and the only alarm fires exactly once._ The `error!` in `update_block_status` is immediately self-clearing, because the very next statement is `mark_executed`, which sets `executed_at` on the delegation and makes `pending_delegation()` return `None` from the next block on. One log line is the entire signal for a permanently wedged queue. Includes a secondary defect: `pending_delegation`'s query has no `ORDER BY` and no `WHERE nonce IS NOT NULL` despite promising the in-flight nonce, so with two unexecuted delegation rows it can return the `NULL`-nonce one and silently disable the self-check; all three of its tests create exactly one delegation row. Applies to `origin/feat/batex_4` (PR #904).

### Checked and found sound (no finding)

Recorded so the negative results are not re-derived:

- **Authorization replay.** `Authorization { chain_id: U256::from(tx.chain_id), address: delegate, nonce: tx.nonce + 1 }` uses the real chain id rather than the wildcard `0`, so it cannot be replayed on another chain. Within the chain the signed authorization is a standalone object that a mempool observer can lift into their own type-4 transaction, but it only applies when the authority's nonce is exactly `N + 1`, which is reachable only _after_ the service's own transaction at `N` has already applied it — and the delegate is the same either way. Not exploitable.
- **The batch gas formula against the new guard.** Worst case: every earlier call consumes its full reservation. The formula budgets `gas_j + gas_j/63 + 5_000` per call while the guard needs `gas_i * 64/63 = gas_i + gas_i/63`, so the remaining budget at index `i` still satisfies the guard with the `5_000` terms to spare. The formula is sound _provided_ the `5_000` and `26_000` constants cover real overhead — which the epic itself flags as unmeasured. An **under-estimated per-call `gas`** does **not** revert the batch; the callee OOGs inside its own reservation and is swallowed as a `CallFailed`, i.e. case 2 of `F-CORE-068`.
- **Nonce high-water lookahead vs. `prune`.** The Phase 4 query reads a single row (`ORDER BY nonce DESC LIMIT 1`) and relies on the max-nonce row being the most recently allocated. Pruning cannot break this: any row with a nonce above the delegation's is allocated later, so deleting an executed lower row cannot lower the high-water mark below a live reservation.
- **EOA-ness assumptions in the contracts.** `grep` over `contracts/src/` finds no `extcodesize`, `code.length`, `isContract` or `tx.origin == msg.sender` check, so delegating the validator's and sentinel's signer EOAs does not trip any "must be an EOA" gate in Safenet's own contracts.
- **`Safenet7702Executor`'s access control.** `require(msg.sender == address(this), OnlySelf())` is the only authorisation, which is correct for a 7702 self-call target; there is no ERC-1271, no 4337 entry point, no sponsored path, and the `receive()` is plain payable.
- **`submit_transaction`'s rearrangement.** Computing `bumped_fees` before `build` and recording the `Submission` from those fees produces values identical to reading them back off the built `TxEip1559`. Behaviour-preserving, as the epic claims.

## What to re-run when this merges

The audit's 21 end-to-end-validated PoCs were run on local Anvil. These are the ones the batex stack puts at risk, plus the new coverage it demands. Anvil must be on **Prague or later** for any 7702 path (`--hardfork prague`); the epic flags this as unconfirmed in its own Phase 8.

**Re-run unchanged, to confirm no regression (Phases 2–4 are inert, so all should still pass):**

1. `F-CORE-067`'s duplicate-action PoC — reorg replay produces two onchain transactions at two nonces. Re-run **twice**: once with `executor` unset (must be byte-identical to the `main` result), once with it set, where the expected new result is **two duplicate batches**.
2. `F-CORE-060`'s underpriced-ratchet PoC — confirm `fees::bump` still ratchets at 1.1× per block and that the `bumped_fees`/`build` split records the same fee floor in `record_submission`.
3. `F-CORE-061`'s first-submission-rejection PoC — confirm the regexes still miss it and the fee is still unchanged on retry.
4. `F-CORE-062`'s nonce-wedge PoC — then extend it: allocate a delegation, let the authorization fail, and confirm the queue wedges with exactly one `error` line (this is `F-CORE-069`).
5. `F-CORE-063`'s mark-executed-without-a-receipt PoC — extend to a batch: submit a batch whose calls all revert, confirm `executed_at` is set and the row is pruned (this is `F-CORE-068`).
6. `F-CORE-064`'s post-expiry-resubmission PoC — and add the delegation, which is `expires_at: None` by design.
7. `F-CORE-066`'s config PoC — extend with `max_batch_gas = 0` and `executor = "0x00…00"`.
8. `F-VAL-065` and `F-SEN-006` replay PoCs — re-run with `executor` set; the expected observable changes from N duplicate transactions to one duplicate batch whose reverts are `CallFailed` events on the service's own EOA rather than failed transactions.

**New coverage required before Phase 7 ships:**

9. Phase 5 landing: assert `queue_delegation` is idempotent across a restart **while the delegation is still queued but unallocated**, and across a restart **after it executed but before `prune`** — the second case enqueues a second delegation by design and must not produce two reservations.
10. `pending_delegation` with **two** unexecuted delegation rows (one allocated, one not) — currently order-undefined; add the test before the query is relied on.
11. Phase 6 landing: measure the `5_000` per-call and `26_000` base constants against real action calldata on a Prague Anvil, per the epic's own open question, and assert the `InsufficientGas` guard does not fire for a full `max_batch_gas` batch of real validator actions.
12. Phase 7 landing: a batch containing one reverting call — assert the remaining calls still take effect and that the service can tell (it currently cannot; that is `F-CORE-068`).
