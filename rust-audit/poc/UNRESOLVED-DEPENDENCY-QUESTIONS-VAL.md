# Unresolved dependency questions — **validator crate** (QA-VAL)

> **Scoped file.** The shared, cross-crate list is
> [`UNRESOLVED-DEPENDENCY-QUESTIONS.md`](UNRESOLVED-DEPENDENCY-QUESTIONS.md) and uses plain
> numbers (`1`, `2`, …). This file uses `VAL-Qn` so the two numbering schemes cannot collide.
> Where a question here overlaps one there, it is cross-referenced.

Written by **QA-VAL**, per the QA brief §3.

Assumption **A6** holds for this run: **no dependency source is on disk** and there is no Rust
toolchain, so nothing about `frost-core`, `k256`, `sqlx`, `rayon`, `alloy` or `serde` could be
verified. Every finding below has a claim that cannot rise above class `I` until one of these is
answered.

Each entry gives **the exact check** — the file and item to open in the vendored source, or the
short program to run — so the team can close it in minutes rather than re-deriving the question.
Pinned versions are from `Cargo.lock` at commit `2893917`: `frost-core 3.0.0`,
`frost-secp256k1 3.0.0`, `k256 0.13.4`, `elliptic-curve 0.13.8`, `sqlx 0.9`.

To get the sources at all: `cargo fetch` then look under
`~/.cargo/registry/src/index.crates.io-*/<crate>-<version>/`, or `cargo vendor`.

---

## VAL-Q1 — `frost_core::Identifier`: how does one get the scalar out?

**Blocks:** `poc/F-VAL-001/poc.rs` (`identifier_scalar`), and any future Lagrange work.

**Question.** `poc.rs` converts an `Identifier` to a `k256::Scalar` via `Identifier::serialize`,
written against `AsRef<[u8]>` so it compiles whether that returns `Vec<u8>` or the ciphersuite
`Serialization` type. Which is it in 3.0.0, and is `Identifier::to_scalar` public (the validator
enables `frost-core`'s `internals` feature, `crates/validator/Cargo.toml:11`)?

**Exact check.** `frost-core-3.0.0/src/identifier.rs` — the signature of `pub fn serialize`, and
whether `to_scalar` is `pub` or `pub(crate)`. Thirty seconds.

**If `to_scalar` is public**, replace `identifier_scalar`'s body with
`frost_core::Identifier::to_scalar(id)` and delete the length assertion.

---

## VAL-Q2 — the FROST(secp256k1, SHA-256) challenge `H2`

**Blocks:** `poc/F-VAL-033/nonce_reuse.rs` (`challenge`), i.e. the key-recovery arithmetic.

**Question.** The PoC recomputes `c = H2(SerializeElement(R) ‖ SerializeElement(PK) ‖ msg)` as
`hash_to_field::<ExpandMsgXmd<Sha256>, Scalar>` with DST
`"FROST-secp256k1-SHA256-v1" ‖ "chal"`, mirroring `frost::ecdh::hash_to_scalar`'s use of the same
split DST with `"enc"` (`crates/validator/src/frost/ecdh.rs:123-132`) and `participants.rs`'s
documented `"id"`. Is that the ciphersuite's actual `H2`?

**Exact check.** `frost-secp256k1-3.0.0/src/lib.rs` — the `H2` implementation on the
`Secp256K1Sha256` ciphersuite; confirm the context string and the discriminant. Alternatively check
whether `frost_core::challenge` and `Challenge::to_scalar` are public under `internals`, in which
case use them directly.

**Self-answering.** `signing_round` asserts `z_aggregate·G == R + c·PK` on public data before using
`c`, with a failure message naming this question. So the PoC tells you the answer when you run it —
this entry only tells you where to look if it says no.

---

## VAL-Q3 — does `sqlx` 0.9 enable `PRAGMA foreign_keys` by default?

**Blocks:** **F-VAL-035** claim (c) (class `I` there), and
`poc/F-VAL-005-066/secrets_reconciliation.rs::reconciliation_cascades_away_a_committed_nonce_chunk`.

**Question.** `nonces` rows are removed only by the `ON DELETE CASCADE` from `nonces_chunks`
(`crates/validator/src/secrets/store.rs:80-87`). SQLite honours that only with
`PRAGMA foreign_keys = ON`, which is **off** in SQLite's own default.
`safenet_core::utils::connect_sqlite` sets only pool timeouts (`crates/core/src/utils.rs:56-62`) and
the options come from a TOML URL (`crates/validator/src/config.rs:29`). No pragma is set anywhere in
the workspace.

**Exact check, no source needed** — this is the one C-VAL-B already identified as answerable by a
test, and it is now written:

```rust
// crates/validator/src/secrets/store.rs, tests module
#[tokio::test]
async fn retain_nonces_cascades {
    let store = store.await;
    let root = store.register_nonces_chunk(GROUP, ME, nonce_chunk(4)).await.unwrap();
    store.retain_nonces([]).await.unwrap();
    assert_eq!(count_root_nonces(&store, root).await, 0);
}
```

**If it fails**, retired groups leave their complete nonce inventory on disk with no chunk row
pointing at it, so no later `retain_nonces` can ever find it — F-VAL-035 (c) becomes a live defect
rather than an upgrade hazard, and F-VAL-066's nonce-cascade consequence changes shape (orphaned
rather than deleted).

**Either way, set the pragma explicitly.** The value of the answer is bounded; the value of not
depending on it is not.

---

## VAL-Q4 — does `sqlx`'s SQLite pool have a busy timeout, and what is it?

**Blocks:** **F-VAL-038** (the step from "contention" to "the validator exits" is explicitly an
inference), **F-VAL-004** trigger A (how likely is an `SQLITE_BUSY` from `store_keygen_secrets`),
**F-VAL-066** (whether the delete/insert interleaving is a retry race or a queue).

**Exact check.** `sqlx-sqlite-0.9.*/src/options/mod.rs` — the `Default` impl for
`SqliteConnectOptions`: the `busy_timeout` field's default, and whether `journal_mode` defaults to
WAL. Then `SqlitePoolOptions`' default `max_connections`.

**Or measure it.** With a toolchain, the honest answer is a benchmark, not a source read:

```rust
// time `register_nonces_chunk` with a real 1024-nonce chunk while a second
// task commits snapshots in a loop on the same pool
```

F-VAL-038's whole claim is about duration; a source read gives the timeout but not the transaction
length, and it is the ratio that matters.

---

## VAL-Q5 — does `frost-core`'s `Debug` redact `SigningShare`, `SecretPackage` and `KeyPackage`?

**Blocks:** **F-VAL-062**'s central claim (class `I` today).

**Exact check — no source needed, and it is the cheapest check in the audit.** Run
`poc/F-VAL-062/debug_redaction.rs`. `KeyShare::dummy`'s signing share is `k256::Scalar::ONE`
(`crates/validator/src/frost/keygen.rs:446-452`), so a `format!("{:?}", …)` answers it directly.
See that directory's README for how to read the result — **a pass partly refutes the finding**, and
that is a result worth recording.

If you want the source anyway: `frost-core-3.0.0/src/keys.rs`, the `Debug` impls for `SigningShare`
and `SecretShare`, and `frost-core-3.0.0/src/keys/dkg.rs` for `round1::SecretPackage`.

---

## VAL-Q6 — does `round2::sign` verify that the supplied `SigningNonces` match the signing package?

**Blocks:** **F-VAL-034** (the whole *outcome*; the missing local check is `E2`, the consequence is
`I`).

**Question.** `handle_nonces` applies a nonce resume to whatever session currently holds the message,
without checking the signature id (`crates/validator/src/state/sign.rs:359-404`). Because
`core::state` states that resume ordering is undefined and effects may run more than once
(`crates/core/src/state/mod.rs:44-64`), a resume from a ceremony that has since restarted can land on
the restarted one. The only thing preventing a share computed from a stale nonce is `frost-core`'s
own commitment check.

**Exact check.** `frost-core-3.0.0/src/round2.rs` — does `sign` (or `SigningPackage`'s
accessors it calls) compare `signer_nonces.commitments` against the package's entry for this
signer, and what error does it return? If it does, F-VAL-034's outcome is "a warning and no share",
which is the benign branch and the reason C-VAL-B floored it at 40.

**Do not leave it as a dependency.** Whatever the answer, `handle_nonces` should check the signature
id itself; that is F-VAL-034's remediation and it removes the question.

---

## VAL-Q7 — `k256::Scalar` inherent items

**Blocks:** nothing substantive; a possible mechanical fix in three PoC files.

**Question.** The PoCs use `Scalar::ZERO`, `Scalar::ONE`, `Scalar::invert` and
`Scalar::from_repr`, importing `elliptic_curve::{Field, PrimeField}` under
`#[allow(unused_imports)]` in case they are inherent. `crates/validator/src/frost/ecdh.rs:124` uses
`Scalar::ZERO` with no `Field` import, so at least `ZERO` is inherent.

**Exact check.** `k256-0.13.4/src/arithmetic/scalar.rs` — the inherent `impl Scalar` block. If the
imports are redundant, drop the `allow` and the `use`; if `invert` is not inherent, keep them.

---

## VAL-Q8 — is the x-coordinate of an ECDH point close to uniform?

**Blocks:** **F-VAL-002** basis row 9 (class `I`).

**Question.** The pad is `x(sk · Q)` used directly as a one-time pad
(`crates/validator/src/frost/ecdh.rs:110-121`). About half of all 256-bit values are valid
secp256k1 x-coordinates, so the pad is measurably non-uniform.

**Not a dependency question at all** — it is a property of the curve, and the fix (F-VAL-002's KDF)
removes it regardless. Listed here only so it is not mistaken for one. The check, if anyone wants a
number, is a histogram over `hash_to_scalar`-derived keys, but it will not change the remediation:
option 1 of F-VAL-001 (`HKDF-SHA256` over the shared secret, bound to `(gid, sender, recipient)`)
fixes the bias, the two-time pad and the possession gap in one change.

---

## VAL-Q9 — `rayon`'s global pool sizing under the shipped deployment

**Blocks:** **F-VAL-038** basis row 2's escalation.

**Question.** `NonceChunk::with_size` fans 1024 nonce generations across
`rayon`'s global pool (`crates/validator/src/frost/preprocess.rs:112-131`), from inside a dedicated
worker thread that is itself one per group. How many cores does that occupy, and does it starve the
tokio runtime?

**Exact check.** Not a source read — a measurement, on the single-core configuration
`docs/validator-handbook.md` describes. Run the validator's nonce generation under
`taskset -c 0` and watch block-processing latency. `RAYON_NUM_THREADS` is the mitigation to test
against.

---

# Answers — V-VAL, Phase 5 (executed)

Dependency sources are now on disk under `~/.cargo/registry/src/index.crates.io-*/` and a toolchain
exists (cargo/rustc 1.98.1). **A6 no longer holds for the validator crate's questions.** Seven of
the nine are closed below; the two that remain need a machine this is not, not a source read.

Test sources and full output: `poc/V-VAL-dependency-questions/`.

| | Question | Answer | Effect |
| --- | --- | --- | --- |
| **VAL-Q1** | `Identifier` → scalar | **Closed.** `pub fn serialize(&self) -> Vec<u8>` (`frost-core-3.0.0/src/identifier.rs:65`). QA's `AsRef<[u8]>` helper compiled unchanged. | none — PoC ran as written |
| **VAL-Q2** | the FROST `H2` challenge | **Closed, correct.** QA's DST `"FROST-secp256k1-SHA256-v1" ‖ "chal"` verified by the PoC's own `z·G == R + c·PK` self-check on every run. | F-VAL-033 key recovery is sound |
| **VAL-Q3** | `PRAGMA foreign_keys` | **Closed: ON.** `sqlx-sqlite-0.9.0/src/options/mod.rs:185-187` sets it in `new`; executed `PRAGMA foreign_keys` returns `1`; the cascade was observed firing. | **F-VAL-035 leg (c) REFUTED**; F-VAL-066's nonce cascade keeps its shape |
| **VAL-Q4** | busy timeout, WAL, pool | **Half closed.** `busy_timeout = 5000` ms, `journal_mode = delete` (**not** WAL), `synchronous = 2` (FULL), `max_connections = 10`. The *duration* half is not measurable here. | F-VAL-038 → 55%; sharpens F-VAL-004 trigger A |
| **VAL-Q5** | `frost-core` `Debug` redaction | **Closed: it redacts.** `SigningShare` (`keys.rs:126-133`), `round1::SecretPackage` (`keys/dkg.rs:191-204`), `round2::SecretPackage` (`:337-350`) all hand-write `"<redacted>"`. Confirmed by executed `format!("{:?}", …)`. | **F-VAL-062, F-XC-002, F-CORE-036 leak claims REFUTED**; all reduced |
| **VAL-Q6** | does `round2::sign` bind the nonce? | **Closed: yes.** `round2.rs:140-143` returns `Error::IncorrectCommitment`; executed, giving `Err(Unexpected(IncorrectCommitment))`. | F-VAL-034 outcome is benign and cannot escalate |
| **VAL-Q7** | `k256::Scalar` inherent items | **Closed.** `ZERO` (`scalar.rs:80`), `ONE` (`:83`) and `invert` (`:128`) are inherent; `from_repr` needs the `PrimeField` import. The `Field` import in the PoCs is redundant. | cosmetic |
| **VAL-Q8** | ECDH x-coordinate uniformity | **Not a dependency question**, as the entry itself says. Unchanged, class `I`, and the remediation removes it regardless. | none |
| **VAL-Q9** | `rayon` global pool sizing | **Open.** Needs the single-core deployment under `taskset -c 0`; this host is not it. | F-VAL-038's escalation stays unproven |

## A correction that applies to every PoC README in this audit

Every README gives its command as `cargo test -p validator --lib …`. That fails:

```
error: no library targets found in package `validator`
```

`validator` is a binary-only crate. The working form is **`--bins`**:

```sh
cargo test -p validator --bins <module path> -- --nocapture
```

## One trap worth carrying forward

`KeyShare::dummy` (`crates/validator/src/frost/keygen.rs:443-453`) seeds the signing share with
`k256::Scalar::ONE` **and** the identifier with `Identifier::try_from(1)`. Both serialize to
`0000…0001`, and the identifier is printed in the clear because it is public. Any test that asks
"does this rendering contain the secret scalar?" by substring match will answer **yes** when the
answer is no. It did, on the first run of `poc/F-VAL-062/debug_redaction.rs`. Isolate the
`signing_share` field before asserting — see that file's
`signing_share_field_alone_is_redacted`.
