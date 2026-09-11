# Plan: Scheduled secret pruning

Component: the validator's secret store, effect handler and state machine (`crates/validator/src/secrets/store.rs`, `crates/validator/src/service/effect.rs`, `crates/validator/src/state/`), plus a small housekeeping hook in the shared effect-handler interface and driver (`crates/core/src/effects.rs`, `crates/core/src/driver.rs`).

---

## Overview

The validator's [secret store](../crates/validator/src/secrets/store.rs) persists DKG polynomial secrets and FROST signing nonces independently of reorg-aware snapshots. Today, [`Effect::ReconcileGroupSecrets`](../crates/validator/src/service/effect.rs) immediately deletes secrets that the current state no longer needs. A rollback or restart replay can still need those secrets.

This epic replaces immediate deletion with a block-based schedule while preserving the existing effect-driven mechanism:

- **Effects schedule deletion.** `ReconcileGroupSecrets` carries the block and retained groups computed by the state transition. The handler writes the schedule to the database.
- **The store orders reconciliation by block.** Persist the last accepted reconciliation block. Ignore requests at lower blocks; apply requests at equal or higher blocks. Update this marker and both tables' schedules in one transaction.
- **Housekeeping receives only block status.** After applying state transitions and dispatching their commands, the driver forwards the state machine's latest committed block and safe snapshot boundary. The handler collects eligible rows without reading protocol state or receiving retained groups.

This is a retention improvement over immediate deletion, with the existing missing-secret and protocol recovery behavior as the fallback. It does not guarantee that every secret needed by a replay or overlapping effect survives. Block numbers order requests but do not identify reorg branches; this limitation is an accepted tradeoff for the simpler mechanism.

Explicitly **out of scope**: [`SecretStore::take_nonce`](../crates/validator/src/secrets/store.rs) still deletes a nonce immediately and permanently when handing it out. Scheduling applies only to retirement of unused secret material, including DKG secrets after a key share is available. Consumed nonces must never be restored.

---

## Architecture Decision

### Reconciliation remains a state-machine effect

Keep `Effect::ReconcileGroupSecrets` and its retained-group computation in the existing `Message::NewBlock` transition. Add the originating block to the effect. The handler schedules absent groups, unschedules retained groups, and reconciles the process-local nonce generators as it does today.

The effect handler has no state-machine reference or inspection capability. All protocol-state information arrives through effects. There is no service housekeeping projection, extra generic payload, or `StateMachine::inspect` accessor.

### Persist the last accepted reconciliation block

Store one reconciliation marker alongside the secret tables. A reconciliation transaction applies only if its block is greater than or equal to the stored block, or no block has been stored yet. A lower-block request is a successful no-op. Equality must remain accepted so another reconciliation at the same height can change the retained sets, including after replay or a reorg.

The block check, marker write and schedule updates for both secret tables must be atomic. An application-side read followed by independent writes would allow concurrent effects to pass the check and commit in the wrong order. Use a conditional write inside the same database transaction instead.

For each accepted request, retained rows get `delete_after = NULL`; absent rows get `COALESCE(delete_after, block)`. This preserves the first scheduled absence while the group remains absent. A group retained and later dropped gets a new schedule. Equal-block requests execute normally, with the last committed request determining retention.

Preserve the marker across database reopening, restart, rollback and collection, even when all secret rows have been deleted. Reconciliations below it remain ignored until processing reaches that height again. There is no startup reset or branch identifier.

### Housekeeping receives block status only

Add a default no-op `EffectHandler::housekeeping(BlockStatus)` method and an awaited delegate on `EffectManager`. The driver calls it once per indexer update after applying state transitions, pruning snapshots, dispatching commands and queueing transactions, passing `state.block_status()`. Its only input is a block-status update: latest committed block and earliest retained snapshot. Repeated identical statuses are harmless and allow collection retries.

Use the state machine's safe block rather than the watcher's safe block. The snapshot boundary reflects how far the state machine has actually committed and avoids using a chain cutoff ahead of its replay position. Housekeeping deletes rows with `delete_after <= status.safe`.

Housekeeping is awaited inline, outside the effect task queue. It may overlap already spawned effects; database transactions serialize store mutations. Command dispatch does not guarantee effect completion: housekeeping may acquire a shared mutex or database write lock before a spawned reconciliation does. It does not drain effects, wait for unconsumed resumes, require catch-up to the observed tip, or reconcile retained groups before deleting. It is not called directly for `Input::Resume`.

### Accepted recovery tradeoff

The persisted block rejects an older reconciliation once a newer one has committed. It does not prove that the accepted retained sets belong to the current branch, or that a newer retaining effect has completed before collection. During restart replay or rollback, an old schedule can become eligible before a current reconciliation clears it. Equal-height effects also have no branch ordering.

These cases can still delete material needed by an in-flight ceremony. The existing nonce lookup effects return `Resume::Noop` when material is absent, and signing timeouts can retry or drop stalled ceremonies. Deleted secrets themselves are not recovered: fresh DKG secrets cannot reproduce an existing commitment, and nonce generation cannot recreate a deleted committed nonce. Recovery may therefore cost a ceremony or participation; this epic preserves the existing recovery paths rather than promising uninterrupted progress.

Periodic reconciliation repairs schedules for rows still present or subsequently written once an equal-or-higher-block effect succeeds. It cannot repair a deletion that already happened. Tests and documentation must describe this distinction instead of asserting lossless replay.

### Alternatives Considered

- **Project retained groups into housekeeping and wait for idle effects.** This adds state access and driver coordination beyond the chosen scope. Keep protocol-state decisions in effects.
- **Reset schedules on startup and rollback.** This requires a reset protocol and coordination with outstanding effects. Keep a persistent block marker and accept the recovery tradeoff above.
- **Reject equal-block reconciliation.** This would discard legitimate reconciliation updates at the same height. Reject strictly lower blocks only.
- **Separate ordering markers for each secret table.** Both retained sets come from the same effect, so one transaction and one marker keep their ordering consistent.
- **Keep retirement metadata in snapshot state.** This needs a separate protocol for applying it to the external store; row-based schedules reuse the current store boundary.
- **Use time-based retention or immediate group deletion.** Block-based scheduling follows the existing snapshot retention boundary and improves on immediate deletion without adding a clock dependency.

---

## Tech Specs

### Shared housekeeping interface (`crates/core/src/effects.rs`)

Keep the existing effect/resume generic parameters:

```rust
pub trait EffectHandler<Effect, Resume>: Send + Sync + 'static {
    fn perform_effect(&self, effect: Effect) -> impl Future<Output = Resume> + Send;

    /// Performs maintenance using committed snapshot block bounds only.
    /// Called after state transitions and command dispatch for an update.
    /// Handlers log and record failures; subsequent updates retry.
    fn housekeeping(&self, status: BlockStatus) -> impl Future<Output = ()> + Send {
        let _ = status;
        async {}
    }
}
```

Add `EffectManager::housekeeping(&self, status: BlockStatus)` to await the handler method directly. There is no housekeeping resume or result acknowledgement. `Pure` and the sentinel handler inherit the default no-op; `Service` and state-machine interfaces are unchanged.

### Driver wiring (`crates/core/src/driver.rs`)

At the start of `Driver::update`, record whether the input is an indexer update before moving it into the existing match. Keep state transitions and snapshot pruning in their current locations, then invoke housekeeping after the command dispatch loop and transaction queue handling, immediately before returning `Ok(())`:

```rust
let run_housekeeping = matches!(&input, Input::Update(_));

// Existing input handling and snapshot pruning.
// Existing command dispatch and transaction queueing.

if run_housekeeping
    && let Some(status) = self.state.block_status().await?
{
    self.effects.housekeeping(status).await;
}

Ok(())
```

The local boolean preserves housekeeping only for `Input::Update`; processing a resume does not trigger it. Effects emitted by the update have been spawned when housekeeping starts, but are not awaited. The handler receives no log update, group sets, reset flag or state reference. No effect-idle check, tip comparison or additional driver fields are needed. State-status errors follow the driver's existing state-error policy.

### Secret store schema (`crates/validator/src/secrets/store.rs`)

Add nullable `delete_after INTEGER` columns to `keygen_secrets` and `nonces_chunks`. `NULL` means no pending deletion. `nonces` is unchanged; deleting its chunk cascades to its rows.

Add a singleton table for the last accepted reconciliation block:

```sql
CREATE TABLE IF NOT EXISTS group_secret_reconciliation (
    id    INTEGER PRIMARY KEY CHECK (id = 0),
    block INTEGER NOT NULL
);
```

An empty marker table means no reconciliation has been accepted. The marker remains even when reconciliation finds no secret rows or collection deletes every row.

Extend the `CREATE TABLE IF NOT EXISTS` definitions only. No deployed databases require an application-managed upgrade, so assume database recreation. Do not add schema probes, `ALTER TABLE` calls or migration execution to `SecretStore::new`. Reopening a database with the new schema preserves its secrets, schedules and reconciliation block.

For the existing dev network, add a temporary `migrations/2026_09_08_scheduled_secret_pruning.sql` file to run manually against the old database before starting the updated validator:

```sql
BEGIN;
ALTER TABLE keygen_secrets ADD COLUMN delete_after INTEGER;
ALTER TABLE nonces_chunks ADD COLUMN delete_after INTEGER;
CREATE TABLE group_secret_reconciliation (
    id    INTEGER PRIMARY KEY CHECK (id = 0),
    block INTEGER NOT NULL
);
COMMIT;
```

Document in the SQL file that this is a one-time manual migration for the pre-change schema, not for a recreated or already migrated database. It preserves existing secret rows, initializes schedules to `NULL`, and leaves the marker empty. The application never discovers or runs this file. Remove it in Phase 5.

### Secret store API (`crates/validator/src/secrets/store.rs`)

Replace immediate group deletion with one scheduling operation covering both retained sets. Collection is a separate operation used by housekeeping:

```rust
pub struct RetainedGroups {
    pub keygen: BTreeSet<B256>,
    pub nonces: BTreeSet<B256>,
}

pub struct Pruned {
    /// DKG secret rows removed.
    pub keygen: u64,
    /// Nonce chunk rows removed; excludes cascaded nonce rows.
    pub nonces: u64,
}

/// Returns false for an ignored lower-block request, true after an
/// accepted reconciliation commits. Does not delete secrets.
pub async fn schedule_group_secrets_deletion(
    &self,
    block: u64,
    retained: &RetainedGroups,
) -> Result<bool, Error>;

pub async fn prune_scheduled_secrets(&self, safe: u64) -> Result<Pruned, Error>;
```

Begin the scheduling transaction with a conditional marker write so the ordering decision participates in database write serialization:

```sql
INSERT INTO group_secret_reconciliation (id, block) VALUES (0, ?)
ON CONFLICT (id) DO UPDATE SET block = excluded.block
WHERE excluded.block >= group_secret_reconciliation.block
RETURNING block;
```

If no row is returned, return `Ok(false)` without changing schedules. Otherwise, update both secret tables in the same transaction using the existing `QueryBuilder` binding pattern:

```sql
UPDATE keygen_secrets
   SET delete_after = CASE WHEN group_id IN (?, ...) THEN NULL
                           ELSE COALESCE(delete_after, ?) END;

-- Empty retained set:
UPDATE keygen_secrets SET delete_after = COALESCE(delete_after, ?);
```

Apply the same operation to `nonces_chunks` with its retained set. Commit before returning `Ok(true)`. A failed update or commit rolls back the marker and both schedule updates, allowing a retry at the same block.

Collection uses its own transaction across both tables:

```sql
DELETE FROM keygen_secrets WHERE delete_after <= ?;
DELETE FROM nonces_chunks  WHERE delete_after <= ?;
```

Commit before returning counts. Collection does not change the reconciliation marker or inspect retained groups. It can run before or after an effect's scheduling transaction, but cannot observe half of one. Report deletion metrics only for committed deletes.

`store_keygen_secrets` continues returning the originally stored secrets on conflict and additionally clears `delete_after`. Newly registered nonce chunks start with `NULL`. These writes do not change the reconciliation marker. Later accepted reconciliations schedule newly inserted rows if their groups remain absent. `take_nonce` remains consumptive, with no schedule-based restoration.

Nonce chunks can accumulate over an epoch and deletion cascades to their nonce rows. Start with existing indexes and measure housekeeping latency on representative data; do not assume constant cost.

### Effect and state machine (`crates/validator/src/service/effect.rs`, `state/`)

Keep the existing effect name and metric label, adding its originating block:

```rust
ReconcileGroupSecrets {
    block: u64,
    groups: BTreeMap<B256, Option<Arc<KeyShare>>>,
},
```

Thread `block` from `Message::NewBlock(block)` through `handle_group_reconciliation` into the effect. Retained-group computation and epoch reaping stay in the transition. Preserve the current rules:

- DKG secrets are retained for participating groups still building their key share.
- Persisted nonces are retained for every tracked group, including groups whose key share is `None`.
- Generators run only for groups with a key share.

The handler builds the two retained sets from the effect and calls `schedule_group_secrets_deletion`. An ignored request returns `Resume::Noop` without changing generators. For an accepted request, apply the existing generator retain/start logic immediately. Reuse the existing generator mutex across this reconciliation arm, acquiring it before scheduling, so accepted scheduling and generator updates follow the same order within the handler. Other effects and housekeeping retain their existing concurrency; no driver barrier is introduced.

Implement the validator's housekeeping method by calling `prune_scheduled_secrets(status.safe)`. Log and count errors locally. A later block-status notification retries collection; a failed reconciliation is retried through subsequent new-block effects. Housekeeping does not synthesize a reconciliation or sample state.

### Metrics (`crates/validator/src/metrics.rs`)

Add `safenet_validator_secrets_pruned_total{kind}` with `keygen` counting DKG rows and `nonces` counting nonce chunks. Add `safenet_validator_housekeeping_total{result}` using the existing success/failure labels. Increment deletion counters only after commit and materialize all label combinations at zero.

Keep the `ReconcileGroupSecrets` effect label. An ignored lower-block request is a successful no-op. No shared driver deferral metrics are needed because collection is not gated on catch-up or effect completion.

### Test cases

Store tests extend `crates/validator/src/secrets/store.rs`:

- A fresh database has both scheduling columns and an empty reconciliation marker. Reopening preserves rows, schedules, the marker and nonce consumption.
- Validate the temporary manual SQL against a pre-change devnet fixture: existing rows survive, schedules start as `NULL`, and the marker is empty. Remove fixtures used solely for the temporary migration in Phase 5.
- The first reconciliation is accepted, including block zero. A higher block applies; a lower block changes neither retained nor absent rows in either table, nor the marker.
- An equal-block request applies changed retained sets. Repeating an identical request preserves deadlines; retaining then dropping a group at a later block starts a new deadline.
- Ordering survives reopening and collection of every secret row. Lower-block replay remains ignored; an equal-or-higher-block reconciliation applies again.
- Concurrent reconciliations cannot let a lower block overwrite a higher one. Use controlled transaction ordering rather than timing assumptions.
- Inject a failure after the marker or first table update: the marker and both schedules roll back together, and a subsequent retry succeeds.
- An empty retained set schedules everything. Collection deletes only deadlines at or before `safe`, including equality, and leaves unscheduled rows untouched.
- Deleting chunks cascades to nonce rows and reports chunk counts; failed collection rolls back both tables and produces no successful counts.
- Scheduled but uncollected nonces remain revealable and consumable. Conflicting keygen writes preserve the original secrets and clear the schedule. Maintenance never restores consumed nonces.

Core and validator wiring tests:

- The default housekeeping handler is a no-op; manager delegation forwards block status.
- The driver calls housekeeping after state transitions, snapshot pruning, effect dispatch and transaction queue handling, forwarding the state machine's status even when it differs from the watcher's status.
- Housekeeping is not called directly on resumes and can run while a dispatched effect is still pending; use a controlled task barrier to verify that dispatch does not imply completion.
- Reconciliation carries the new-block number and preserves the existing retained-group rules. Lower-block requests skip both scheduling and generator changes; equal-block requests are applied.
- Collection uses `status.safe` and records committed deletion counts or a failure; a subsequent notification retries.
- Keep the existing validator reorg/nonce regression. For a schedule that survives from an earlier branch, cover the accepted behavior: lower-block reconciliation is ignored, collection can remove the secret, and a missing nonce produces `Resume::Noop` without nonce reuse. Existing protocol timeout/retry behavior remains the fallback; do not assert that deleted secrets are recreated.

---

## Implementation Phases

```text
Phase 1 (core hook) ─────────────────┐
                                    ├─→ Phase 4 (collection) ─→ Phase 5 (docs)
Phase 2 (schema) ─→ Phase 3 (schedule)┘
```

Each phase must compile independently. For affected Rust packages, run `cargo fmt --all`, `cargo clippy --package <package>` and `cargo test --package <package>`; run the repository-required `just check` before committing. Validate the sentinel's compatibility with the shared trait's default hook.

### Phase 1 — Block-status housekeeping hook in `safenet-core`

- Add the default `EffectHandler::housekeeping(BlockStatus)` and manager delegate.
- Call it at the end of `Driver::update`, after state transitions, snapshot pruning, command dispatch and transaction queue handling, using `state.block_status()` and a local flag to restrict it to indexer updates.
- Test forwarding, driver timing and the default no-op. Leave the service and state-machine interfaces unchanged.

No validator secret-retention behavior changes yet.

### Phase 2 — Schema and temporary manual devnet migration

- Add `delete_after` to both secret tables and create the reconciliation marker table; add no in-app migration code.
- Add `migrations/2026_09_08_scheduled_secret_pruning.sql` for a one-time manual devnet upgrade, with usage instructions in SQL comments.
- Test fresh-schema creation and reopening; validate the manual SQL against the pre-change schema.

Independent of Phase 1; no scheduling or collection yet.

### Phase 3 — Schedule through block-ordered reconciliation effects

- Add `block` to `ReconcileGroupSecrets` and thread the new-block number through the transition.
- Replace immediate group deletion APIs with `schedule_group_secrets_deletion`, atomically updating the block marker and both schedules.
- Wire the handler to ignore lower-block requests and apply equal-or-higher-block requests; reuse the generator mutex for consistent local reconciliation order.
- Preserve immediate nonce consumption and original DKG secrets on conflicting writes; add scheduling, ordering and rollback tests.

Requires Phase 2. Secrets are scheduled but accumulate until collection lands in Phase 4.

### Phase 4 — Collect scheduled secrets on housekeeping

- Add `prune_scheduled_secrets` and committed deletion counts.
- Implement validator housekeeping, failure logging and metrics.
- Test collection boundaries, retries, nonce consumption and the documented replay tradeoff. Run the existing validator reorg/nonce regression and measure housekeeping latency with representative nonce-chunk data.

Requires Phases 1 and 3. Phases 3 and 4 may be combined to avoid the interim accumulation window.

### Phase 5 — Documentation and temporary migration removal

- Update secret-store module docs with effect-driven scheduling, block ordering, block-status collection and immediate nonce consumption.
- Update `docs/validator-handbook.md` with retention timing, metric units and the accepted recovery tradeoff across rollback, replay and concurrent effects.
- Remove the temporary migration and any fixtures or instructions used solely for it.
- Delete this epic after implementation and validation are complete.

---

## Decisions and Remaining Assumptions

1. **State boundary:** only effects carry retained groups. Housekeeping receives `BlockStatus`; the handler cannot sample the state machine.
2. **Ordering:** persist one reconciliation block; ignore strictly lower requests and apply equal or higher ones. Commit the marker and both schedules atomically.
3. **Rollback and restart:** preserve the marker and schedules. Block ordering does not identify branches, and recovery from missing material remains an accepted fallback.
4. **Cadence and cutoff:** call housekeeping on indexer updates after state transitions, snapshot pruning, command dispatch and transaction queue handling, with the state machine's block status. Dispatch does not imply effect completion. Collect at `delete_after <= status.safe` without a catch-up or effect-idle barrier.
5. **Migration:** assume database recreation; provide only a temporary manual SQL upgrade for the existing dev network, removed in Phase 5.
6. **Failure policy:** log failures and retry on subsequent effects or block-status notifications. A scheduling failure must not advance the marker; a collection failure must not report committed deletions.
7. **Nonce consumption and foreign keys:** preserve immediate nonce burning and SQLite cascade behavior, verified in store tests.
8. **Ownership and lifecycle:** retain the current single-driver ownership and group-lifecycle assumptions. This epic adds neither cross-process lifecycle coordination nor a new recovery mechanism.
