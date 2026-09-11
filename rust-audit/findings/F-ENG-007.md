# F-ENG-007 Shutdown drops the serve future instead of draining it, so every in-flight security check is aborted mid-request and the sentinel loses those votes on every deploy

| Field                | Value                                                                                     |
| -------------------- | ----------------------------------------------------------------------------------------- |
| Status               | QA-done                                                                                         |
| Crate and module     | sentinel-engine, `main.rs`                                                                  |
| Location             | `crates/sentinel-engine/src/main.rs:79-84` (related: `crates/core/src/utils.rs:17-36`; `crates/sentinel/src/engine.rs:176-187`) |
| Severity             | Low / Low |
| Certainty            | 80% |
| Assumptions involved | A1, A3, A6                                                                                  |
| Tags                 | dos                                                                                         |

## Claim

The serve loop is a `tokio::select!` between `axum::serve(..)` and `utils::shutdown_signal`. When the signal
arm completes, `select!` drops the other branch — so the `axum::serve` future, and with it every connection
task and every handler future it owns, is cancelled at whatever await point it happens to be sitting on.
`axum::serve` has a `with_graceful_shutdown` combinator for exactly this and it is not used.

The consequence for a check in flight at that instant is that it never produces a verdict. The connection
closes without a response, and the sentinel maps that to `CheckOutcome::Unknown` with
`EngineCheckVerdict::Error` — its own comment says it is "dropping the request unanswered". So each rolling
restart, redeploy, config change or container reschedule silently converts some number of pending checks into
missing votes, and the votes lost are exactly the expensive ones: a check sitting in an `eth_getLogs` walk or
a CoW API lookup is far more likely to be mid-flight than a check that resolved locally in microseconds.
F-ENG-005 compounds this — with no deadline anywhere, a check can sit in flight for an unbounded time, which
widens the window in which a shutdown can catch it.

This is Low, not higher: an operator-initiated restart is a normal event under A1, the sentinel's fallback is
fail-safe (no vote, never a wrong vote), and the loss is bounded by the number of checks in flight. What makes
it worth fixing is that the fix is one combinator, and that the alternative — a `warn!`-free, metric-free
silent vote loss — is indistinguishable at the sentinel from an engine crash.

A second, smaller point in the same lines: `shutdown_signal` `unwrap`s both signal registrations. That is a
startup-time panic in `main`'s task rather than a request-path one, so it is not the "panic is a missing vote"
class, but it is the only `unwrap` on the engine's shutdown path and it lives in another crate
(`crates/core`, R2's scope) — recorded here so it is attributed, not filed.

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| - | ----- | ----- | -------- | -------------- |
| 1 | Shutdown is a bare `select!` against `axum::serve`, with no `with_graceful_shutdown` and no drain step. | E2 | `crates/sentinel-engine/src/main.rs:75-86` | <pre>    let listener = TcpListener::bind(bind_address).await?;<br>    let local_address = listener.local_addr?;<br><br>    tracing::info!(%local_address, "starting sentinel engine");<br>    tokio::select! {<br>        result = axum::serve(listener, api::router(engine)) => result?,<br>        _ = utils::shutdown_signal => {<br>            tracing::info!("received shutdown signal; stopping sentinel engine");<br>        }<br>    }<br><br>    Ok()</pre> |
| 2 | The signal future completes on SIGTERM or SIGINT — the ordinary container-runtime and Ctrl-C paths, not an exceptional one. | E2 | `crates/core/src/utils.rs:13-23` | <pre>/// Creates a signal for supporting graceful shutdown in services.<br>///<br>/// Intercepts both `SIGTERM` (usually sent by container runtimes) and `SIGINT`<br>/// (sent by `Ctrl-C`).<br>pub async fn shutdown_signal {<br>    let sigterm = async {<br>        unix::signal(unix::SignalKind::terminate)<br>            .unwrap<br>            .recv<br>            .await<br>    };</pre> |
| 3 | The caller treats a failed request as no evidence and drops it unanswered — so an aborted check is a lost vote, not a retried one. | E2 | `crates/sentinel/src/engine.rs:176-187` | <pre>                // A failed request is *not* treated as approval or denial: an<br>                // unreachable or malfunctioning sentinel engine isn't evidence<br>                // about the transaction either way, so it resolves to<br>                // [`CheckOutcome::Unknown`], dropping the request rather than<br>                // voting on it.<br>                Err(err) => {<br>                    tracing::error!(<br>                        %err,<br>                        "sentinel engine request failed; dropping the request unanswered",<br>                    );<br>                    (CheckOutcome::Unknown, EngineCheckVerdict::Error)<br>                }</pre> |
| 4 | The engine emits no metric of its own, so the loss is visible only as the sentinel's `EngineCheckVerdict::Error` counter and an `error!` line on the other side of the connection. | E2 | `crates/sentinel/src/engine.rs:189` (and the absence of any `metrics::` call anywhere in `crates/sentinel-engine/src`) | <pre>            crate::metrics::engine_check_verdicts_total(verdict).increment(1);</pre> |
| 5 | Dropping a `tokio::select!` branch cancels that future, and `axum::serve` owns the per-connection tasks whose handler futures are therefore cancelled. | I | `axum` 0.8.9's and `tokio` 1.x's sources are not on this machine (A6, no registry, no network) and nothing was executed. `select!`'s drop-the-loser semantics and the existence of `axum::serve(..).with_graceful_shutdown(..)` are documented behaviours I am relying on but did not verify. | *(no verbatim quote available)* |

## Trigger

Send SIGTERM (`docker stop`, a Kubernetes pod eviction, a `systemctl restart`) while at least one
`POST /v1/security-check` is in flight. The most reliable way to have one in flight is the F-ENG-005 path: a
proposed transaction whose target steers the engine into `AddressPoisoningChecker`'s chunked `eth_getLogs`
walk or `CowChecker`'s HTTPS order lookup, neither of which has a timeout. The client observes a closed
connection with no response; the sentinel logs "sentinel engine request failed; dropping the request
unanswered" and increments `engine_check_verdicts_total{Error}`; no vote is cast for that proposal.

## Considered and rejected

- **"`axum::serve` might already drain on drop."** Rejected as an assumption I am not entitled to make, and
  the code's own shape argues against it: `with_graceful_shutdown` exists precisely because dropping the serve
  future does not drain. I have marked the mechanism class `I` (row 5) rather than assert it, but the
  remediation is correct either way — using the combinator makes the behaviour explicit instead of implicit.
- **"The sentinel will retry, so nothing is lost."** Rejected on the caller's own comment (row 3): the outcome
  is `CheckOutcome::Unknown` and the request is dropped, not requeued. Whether the sentinel's wider effect
  machinery re-drives the proposal later is R7's question, but at this boundary the vote is gone.
- **"This is the same finding as F-ENG-005."** Related but distinct: F-ENG-005 is about work never being
  bounded, this is about work being discarded at a known, controlled moment. They interact (no deadline means
  a wider in-flight window) and the fixes are independent — a graceful drain needs its own timeout to avoid
  hanging shutdown forever, which is worth calling out in remediation.
- **"An operator restart is not an attacker event, so this is not a security finding."** Conceded, and it is
  why this is Low and tagged only `dos`. It is filed because the failure is silent on the engine side: there
  is no drain log, no in-flight gauge and no engine metric at all, so an operator has no way to see how many
  votes a deploy cost them.
- **"The `unwrap`s at `utils.rs:20,27` are a reachable panic."** Rejected for this crate: they run once during
  `main`'s startup, before any request exists, and fail only if the process cannot register a Unix signal
  handler. Not request-reachable, and the file belongs to `crates/core` (R2).

## Remediation options

1. **Use the combinator.** Replace the `select!` with
   `axum::serve(listener, api::router(engine)).with_graceful_shutdown(utils::shutdown_signal).await?`.
   The listener stops accepting immediately, in-flight requests run to completion, and each returns a real
   verdict. One line, no new dependency. Tradeoff: shutdown now waits for the slowest in-flight check, which
   with no timeouts (F-ENG-005) is unbounded — so this should land together with, or after, an outbound-client
   timeout, or be paired with option 2.
2. **Bound the drain.** Wrap the graceful shutdown in a `tokio::time::timeout` so a stuck check cannot hold the
   process past the orchestrator's own termination grace period (commonly 30 s), falling back to the current
   abrupt drop when it expires. This is the version that is strictly better than today under all conditions.
3. **Make the loss observable whatever is chosen.** Log the number of connections still in flight when the
   signal arrives, and add an engine-side verdict counter so an operator can see votes lost to a deploy rather
   than inferring it from the sentinel's error counter. The engine currently records **no** metrics of its own
   despite initialising a Prometheus listener (`main.rs:45`), so this is a gap worth closing independently.

Tests to add: an integration test that starts the router on an ephemeral port, issues a request against a
handler that blocks on a channel, triggers shutdown, and asserts the response is a real verdict rather than a
connection reset. `main.rs` currently has no tests at all.

## Trail

- Reviewer R8: drafted, self-estimate 85%. Confirms ENG-H12. Rows 1-4 are direct reads of this
  checkout; row 5 is the library mechanism and is class `I` under A6 — I did not run anything and no dependency
  source is on disk. `crates/sentinel/src/engine.rs` is R7's scope and `crates/core/src/utils.rs` is R2's;
  both are cited only as the receiving end and the signal source.

## Critic (C-ENG-A)

I read `main.rs:79-84` and the shutdown helper before reading the argument. **No claim is `H`.**

### Per-claim verdicts

**Supported.** `crates/sentinel-engine/src/main.rs:79-84` is a `tokio::select!` between
`axum::serve(listener, api::router(engine))` and `utils::shutdown_signal`, with no
`.with_graceful_shutdown(..)` anywhere in the crate. When the signal arm completes, `select!` drops the
other future.

**Supported.** `crates/core/src/utils.rs:13-36` is the helper, and I note a detail that sharpens the
finding: its own doc comment is *"Creates a signal for supporting **graceful shutdown** in services."* The
helper is named and documented for graceful shutdown; the engine consumes it in a way that is not graceful.
That naming is why this is easy to miss in review, and it belongs in the Claim.

**Supported.** `crates/sentinel/src/engine.rs:181-187` maps the resulting transport error to
`(CheckOutcome::Unknown, EngineCheckVerdict::Error)` with the log line "sentinel engine request failed;
dropping the request unanswered", and `crates/sentinel/src/service.rs:176-179` then returns before
`commit_vote`. So an aborted check really does become a missing vote, and it is counted under the `Error`
label rather than anything that says "we were restarting".

### One mechanism the reviewer understates

The Claim says `select!` drops "every connection task and every handler future it owns". Worth being precise,
because a reader may object that `axum::serve` spawns connection tasks that are detached and would survive
the `Serve` future being dropped. The finding holds regardless, by a second route: `main` returns `Ok()`
immediately after the `select!` (`main.rs:86`), and under `#[tokio::main]` (`main.rs:33`) that drops the
runtime, which tears down every spawned task at its next await point. So in-flight checks die either way —
through the dropped serve future or through runtime teardown a moment later. Adding this makes the claim
robust against the detached-task objection rather than dependent on axum's internals (which are class `I`
here in any case: no dependency source on disk, A6).

### Severity and certainty

**Low / Low — confirmed.** The reviewer's own reasoning is the correct one and I did not find a way to move
it: an operator-initiated restart is normal under A1, the failure is fail-safe (a missing vote, never a
wrong one), and the loss is bounded by in-flight count. It is not Informational because it is a real
behavioural defect with a one-combinator fix, and because F-ENG-005 widens the window it applies to — with
no deadline anywhere, a check can be in flight for an unbounded time, so "some number of pending checks"
is not necessarily small.

**Confirmed — 80%.** Mechanism and trigger both verified; the trigger (SIGTERM during an in-flight check) is
concrete and reachable by any deploy. Not higher because the exact drop semantics of `axum::serve` are
class `I` this run, and because "how many checks are typically in flight" is unmeasurable without running
the system. R8's 85% self-estimate was reasonable; I hold it at 80 for the `I` premise, and the 89% ceiling
applies regardless.

The secondary note in the Claim — `shutdown_signal`'s two `unwrap`s at `crates/core/src/utils.rs:20,26` — is
Supported and correctly **not** filed here: it is startup-time, in `crates/core` (R2's scope), and under A1
carries no untrusted input. Recording it with attribution rather than filing it is the right call.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the
trigger is a process-level signal race, which needs a running server, a blocked handler and a real
shutdown — none of which is expressible as a unit test in a crate whose `main.rs` has no tests at all.

**Certainty unchanged at 80%. Severity unchanged at Low.** Confirmed by inspection that `main.rs:79-84`
uses `tokio::select!` over `axum::serve(...)` and `utils::shutdown_signal`, so the serve future is
**dropped**, not drained, when the signal arrives.

### Remediation check

- **Option 1 (`with_graceful_shutdown`) — sound, one line, no new dependency — but it must not ship
  alone, and the finding says why.** Shutdown then waits for the slowest in-flight check, and with no
  timeouts anywhere (F-ENG-005) that wait is **unbounded**. Shipping option 1 by itself converts a
  guaranteed-fast, lossy shutdown into a possibly-indefinite hang, which an orchestrator resolves with
  `SIGKILL` — i.e. the same loss, later, plus a failed deploy. **Ship option 1 only with option 2, or only
  after F-ENG-005 option 1.**
- **Option 2 (bound the drain with `tokio::time::timeout`) — sound, and it is the version that is strictly
  better than today under all conditions.** Set the budget below the orchestrator's termination grace
  period (commonly 30 s) rather than equal to it, so the fallback path has room to run.
- **Option 3 (make the loss observable) — sound and independently worthwhile.** The engine initialises a
  Prometheus listener at `main.rs:45` and then records **no metrics of its own**, so today an operator can
  only infer lost votes from the *sentinel's* error counter. That is a gap worth closing regardless of
  which of 1/2 is taken, and it is what turns "we think deploys lose votes" into a number.
- **A consequence for the sentinel side worth stating in the report:** an aborted request is not a wrong
  verdict, it is a *missing* one — `crates/sentinel/src/service.rs:173-179` maps `Unknown` to "drop the
  request unanswered". So the cost is participation and reward, not a slashable act. That is why Low is
  right.
- **Test hook: missing.** `main.rs` has no tests, and there is no harness that starts the router on an
  ephemeral port. The test the finding describes needs exactly that, and it is the same infrastructure
  F-ENG-005 and F-ENG-008 need — worth building once.
- **Where the fix belongs: the service entry point** (`main.rs`). Not the checkers, combinator or `RuleId`
  mapping.
