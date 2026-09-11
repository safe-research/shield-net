# Unresolved questions — QA-CORE-SEN's entries, for merge into the shared list

Written by **QA-CORE-SEN** (scope: `safenet-core`, `sentinel`). Commit `2893917`.

**Why this file exists.** These entries were written into
`rust-audit/poc/UNRESOLVED-DEPENDENCY-QUESTIONS.md` and were destroyed when QA-VAL overwrote that
file rather than appending. The shared file is untracked, so there was no git object and no copy to
recover from; this is a restoration from my own working context, not from disk. QA-XC is restoring
the shared file in parallel, so **I have deliberately not written to it again** — the Manager merges
these entries in afterwards.

**Everything below is labelled by where it belongs in the shared file**, so the merge is
unambiguous. Nothing here duplicates QA-XC's own text: entry 1 is strictly an *addition* to their
Q2, which was already stronger than my draft of the same question and which I did not touch.

**Standing disclaimer, restated so this file is self-contained.** No Rust toolchain and no
dependency source exist on this host (`rust-audit/state/baseline.md` §1-2; A9 is FALSE, A6 applies).
Nothing below was executed. Every entry is an instruction for someone who has a toolchain, not a
result.

---

## 1. ADDITION TO **Q2** — `alloy-sol-types` 1.6.0 and non-UTF-8 `string` (settles `F-SEN-013`)

> **Merge instruction:** append verbatim to the end of QA-XC's Q2, after their
> *"Do the same check for the other three `string` fields…"* paragraph. Do **not** replace any of
> their text — their runnable test against the crate's real `SentinelOracleEvents` bindings, with
> the event data segment hand-encoded, is better than the scratch-crate version I had drafted and is
> the artefact that settles the question.

### Addendum from QA-CORE-SEN (two points that version does not cover)

**1. Do the source read as well as the run.** C-SEN asked for it by name in F-SEN-013's
`### Exactly what QA must read to settle basis 8`, and it earns its five minutes for a reason the
assertion alone does not give you: the read explains *why* the answer is what it is, so it keeps
being informative after a dependency bump that a bare assertion would only flag after the fact. Two
items:

1. `~/.cargo/registry/src/*/alloy-sol-types-1.6.0/src/types/data_type.rs` — the
   `impl SolType for sol_data::String` block. Read `detokenize`, and `valid_token` / `type_check` if
   present. The whole question is one line: does it go through `String::from_utf8` (**checked** →
   returns `Err`), or `String::from_utf8_lossy` / `from_utf8_unchecked` (**lossy** → returns `Ok`)?
2. `~/.cargo/registry/src/*/alloy-sol-types-1.6.0/src/types/event.rs` — `SolEvent::decode_raw_log`
   and `SolEventInterface::decode_raw_log`. Confirm a `detokenize` error propagates as `Err` rather
   than being swallowed.

Alloy has historically used the lossy path, which would refute the finding. **Do not assume it.**
C-SEN explicitly refused to upgrade basis 8 on that recollection and so do I — under PROMPT.md §2 an
assertion about a pinned dependency's internals with no source on disk is class `I` at best, and no
amount of protocol reasoning about what a decoder "should" do substitutes for reading it.

**2. An `Ok` answer does not close `F-CORE-004`.** F-CORE-004 is canonical for the *mechanism* — a
deterministic, content-dependent decode failure retried every 100 ms forever with no terminal state
(`crates/core/src/index/events.rs:495-516`, `crates/core/src/driver.rs:206-225`) — and that mechanism
survives a lossy decoder completely untouched. Only the cheap, attacker-chosen path into it goes
away. A green run on Q2 must **not** be read as "the batch-poisoning design is fine".

Relatedly: **F-SEN-013's remediation option 2** (skip undecodable logs in `decode_and_sort` with a
`warn` and a counter) must not be taken as a blanket `core` change, whatever Q2 answers.
`decode_and_sort` is shared by every service, and silently dropping a log a **validator** needed — a
`Sign`, a `KeyGenSecretShared`, a `Preprocess` — is precisely the F-CORE-002 failure mode of
committing an incomplete batch as complete, which this audit rates High. F-CORE-004 option 2 and
F-SEN-013 option 2 propose the same skip and will look like consensus; F-CORE-002 option 1 points the
opposite way on the same code path. The sound pair is **option 3** (make `DecodeLog` terminal rather
than transient — which depends on F-CORE-030, since `Driver::run` currently discards its outcome and
the process exits with status 0) plus **option 5** (alert on a flat
`safenet_core_block_number{status="processed"}`, `driver.rs:313-317`). If skipping is adopted at all,
the boundary belongs in F-CORE-004: only for a log that *cannot* be a valid protocol message, only
paired with F-CORE-006's per-address topic sets, and never in a way that lets an empty or short
result pass a completeness gate.

---

## 2. NEW SECTION — questions that are **not** answerable by reading a dependency

> **Merge instruction:** add as its own section near the end of the shared file, below the
> *"diminishing returns"* line. These are recorded here specifically so they are not mistaken for
> crate questions and handed to whoever is doing the registry reads — they need a **node** or a
> **deployed engine**, and no amount of `~/.cargo/registry` will settle either.
>
> Two finding QA sections cite this section by name; after the merge they should point at wherever it
> lands. Until then they cite
> `rust-audit/poc/UNRESOLVED-DEPENDENCY-QUESTIONS-CORE-SEN.md` §2.

### 2a. `F-CORE-061` — what does a node say when it rejects a **first** submission on fee grounds?

`is_transaction_underpriced` (`crates/core/src/tx/mod.rs:359-365`) matches two patterns, both of
which require the rejection to be about a *replacement*: `"replacement transaction"` **and**
`"underpriced"` together, or the single vendor sentence
`"INTERNAL_ERROR: could not replace existing tx"`. A node that rejects a **first** submission because
its fee is below the pool's own floor produces neither, so the rejection falls into the generic
branch, which deliberately records no fee floor — and the row is then rebuilt with
`bump(fresh, None)`, i.e. the same fee, forever, holding an allocated nonce that blocks every later
one.

The pivotal fact is a **string**, and it is a client's string, not a crate's. Settle it against:

- geth's `core/txpool` error set (`ErrUnderpriced`, `ErrReplaceUnderpriced`, `ErrTxGasPriceTooLow`)
  and how `internal/ethapi` renders each over JSON-RPC;
- Nethermind's `AcceptTxResult` / `TxPool` rejection reasons;
- Erigon's equivalents;
- or, fastest and most trustworthy: a testnet node with a deliberately underpriced first submission,
  capturing the literal `error.message` and `error.code`.

**Record which client and version produced each string**, because the answer is version-specific and
the current two regexes are already vendor-shaped.

C-CORE's section on F-CORE-061 explicitly declined to upgrade the finding on a recollection of geth's
`ErrUnderpriced` wording, which was the right call and which this entry preserves.

**What each answer implies.** If a common client's first-submission rejection is unmatched, the
finding's trigger becomes `E2` and its certainty rises from 58% toward the `E2` band. If every
client in scope happens to include the word "replacement", the finding weakens sharply. **Either
way, F-CORE-061's remediation options 1 and 3 must not ship before F-CORE-060's ceiling** — both
widen the set of rejections that feed an unbounded fee ratchet.

### 2b. `F-SEN-015` variant 1 — does the sentinel engine re-decide a proposal the same way?

Variant 1 of F-SEN-015 (a replayed engine check returns a *different* verdict, so the stored `reason`
no longer matches the onchain `commitHash` and the reveal reverts `InvalidReveal`) rests on the
engine being non-deterministic across a restart — a rule list updated during a deploy, a checker
reading chain state at head rather than at `Effect::EngineCheck.block`, or simply a different rule id
for the same violation. That is a property of the **deployed engine**, not of any crate, and it
cannot be assessed from the `sentinel` crate at all.

**Do not spend registry time on this.** Instead run **variant 2** of the PoC at
`rust-audit/poc/F-SEN-015/poc_service.rs`
(`poc_f_sen_015_unknown_verdict_on_replay_drops_an_already_committed_request`), which needs no
determinism assumption whatsoever: it turns on `handle_engine_check_result` removing the entry at
`crates/sentinel/src/service.rs:156` *before* inspecting the outcome and never re-inserting it on
`CheckOutcome::Unknown` (`:176-179`) — the overwhelmingly likely case on a restart under A3, where
the engine is co-deployed and still booting. Variant 2 alone establishes the loss.

If someone does want variant 1 settled, the check is: run the same proposal through the deployed
engine twice across a restart and diff `(approve, reason)`. That is an engine-team question and
belongs on their list, not on the dependency list.

---

## 3. Cross-references from my QA sections into the shared file

> **Merge instruction:** nothing to merge — this is a map, recorded so the Manager can verify the
> citations still resolve after QA-XC's restoration lands. Four of my QA sections cite QA-XC's
> numbering; if their restored file renumbers, these need updating.

| Finding | Cites | Question |
| --- | --- | --- |
| `F-CORE-036` | shared file, **question 1** | Does `frost-core` 3.0.0 redact secrets in `Debug`? — **the highest-value five minutes in the run for my scope**: if `round1::SecretPackage`'s `Debug` prints the scalar, F-CORE-036 goes from Low/50% to Critical, because core requires `Debug` on every `Effect`/`Resume` and prints them at `trace` in five places (`driver.rs:78-79`, `effects.rs:41-42`, `effects.rs:55`/`:59`/`:78`, `driver.rs:238`/`:261`) and a service cannot opt out. |
| `F-SEN-013` | shared file, **question 2** | The `alloy-sol-types` question above. |
| `F-CORE-037` | shared file, **question 3** | Does `#[serde(deny_unknown_fields)]` do anything alongside `#[serde(flatten)]`? Bears on F-CORE-037 remediation option 2, whose downgrade-safety half relies on that attribute actually binding. |
| `F-CORE-011`, `F-CORE-039` | shared file, **question 4** | Can an un-timed `reqwest` request hang indefinitely? Both findings' "indefinitely" rests on it; answering it is what moves them out of 60% and 55%. One answer, two findings, and one fix (a `tower` timeout layer in `Provider::connect`) closes both. |
| `F-CORE-061`, `F-SEN-015` | **§2 of this file** | The two non-dependency questions above. |
