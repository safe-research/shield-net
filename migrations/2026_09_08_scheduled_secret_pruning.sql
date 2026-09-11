-- Temporary, one-time manual migration: scheduled secret pruning.
--
-- Applies ONLY to a validator database created before scheduled secret pruning
-- was introduced, i.e. one whose `keygen_secrets` and `nonces_chunks` tables
-- have no `delete_after` column and which has no `group_secret_reconciliation`
-- table. It exists for the long-running dev network, whose database predates
-- the change and holds secrets worth keeping.
--
-- Do NOT run it against a recreated or already migrated database: those get the
-- new schema from `SecretStore::new` (crates/validator/src/secrets/store.rs),
-- and re-running this fails on the duplicate column. The validator never
-- discovers or runs this file.
--
-- Stop the validator, then apply it to the database file directly:
--
--     sqlite3 <validator-database> < migrations/2026_09_08_scheduled_secret_pruning.sql
--
-- Existing secret rows are preserved, their deletion schedules start as NULL
-- (nothing scheduled for deletion), and the reconciliation marker starts empty
-- (no reconciliation accepted yet), so the updated validator schedules and
-- collects from its next accepted reconciliation onwards.

BEGIN;

ALTER TABLE keygen_secrets ADD COLUMN delete_after INTEGER;
ALTER TABLE nonces_chunks ADD COLUMN delete_after INTEGER;

CREATE TABLE group_secret_reconciliation (
    id    INTEGER PRIMARY KEY CHECK (id = 0),
    block INTEGER NOT NULL
);

COMMIT;
