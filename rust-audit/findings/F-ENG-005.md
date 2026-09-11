# F-ENG-005 The engine has no deadline anywhere: `x-request-timeout` is parsed and discarded, there is no server timeout or concurrency limit, and neither outbound client has a timeout

| Field                | Value                                                                                     |
| -------------------- | ----------------------------------------------------------------------------------------- |
| Status               | QA-done                                                                                         |
| Crate and module     | sentinel-engine, `api/mod.rs` (with `main.rs`, `Cargo.toml`, and the outbound clients in `checkers/cow.rs` and `core/provider`) |
| Location             | `crates/sentinel-engine/src/api/mod.rs:48-50` and `:25-31` (related: `crates/sentinel-engine/Cargo.toml:7-21`; `Cargo.toml:23-24`; `crates/sentinel-engine/src/main.rs:75-84`; `crates/sentinel-engine/src/checkers/cow.rs:228-231`, `196-204`; `crates/core/src/provider/mod.rs:129-137`; `crates/sentinel-engine/openapi.yaml:23-32`) |
| Severity             | Medium / **Low** |
| Certainty            | 80% |
| Assumptions involved | A2, A3, A4, A6                                                                              |
| Tags                 | dos, config, known                                                                          |

## Claim

There is no point in the request path where wall-clock time is bounded. The caller states its budget in
`x-request-timeout`; the extractor parses it into a `Duration` and the handler throws it away with
`let _ = timeout;` behind a TODO — the item listed in codebase-map § 4 and reported here tagged `known`
per A12. Nothing replaces it: the router carries one layer, `TraceLayer`, so there is no `TimeoutLayer`,
no `ConcurrencyLimitLayer` and no explicit body limit. The absence is structural rather than an oversight of
wiring — the crate does not take the workspace's `tower` dependency, and it takes `tower-http` with
only the `trace` feature, so none of those middlewares is compiled into the binary.

The outbound side is the same. `CowChecker::new` builds a bare `reqwest::Client::new`, which applies no
request timeout, and issues `GET {base}/api/v1/orders/{order_uid}`. `Provider::connect` builds the alloy
client with an observability layer and no timeout configuration, and every `eth_getLogs` the
address-poisoning check issues inherits that. So each in-flight security check can hold one RPC connection
and/or one HTTPS connection open indefinitely, and each such check occupies a tokio task with nothing to
cancel it.

What makes this more than housekeeping is who chooses the path. Under A2 the Safe transaction contents are
fully attacker-controlled, and the transaction's own fields decide which outbound call the engine makes: a
`setPreSignature` batch steers it into the CoW HTTPS lookup with a proposer-chosen `order_uid` interpolated
into the URL, and an ERC-20 transfer target steers it into up to `ceil((lookback+1)/(max_range+1))`
sequential `eth_getLogs` calls (see F-ENG-009 — that count is set by configuration with no validation). A
proposer therefore selects, for their own transaction, the slowest and most externally-dependent path the
engine has. If the third party is slow, rate-limiting, or simply unresponsive, the check does not fail — it
hangs, and the sentinel's own client timeout fires first and turns the whole thing into `Unknown`, i.e. no
vote on the attacker's transaction. Under A4 a stale or rate-limited RPC is explicitly in scope, and the
CoW API needs no assumption at all: it is a third party on the public internet.

Impact is liveness, not safety — a hung check yields no vote rather than a wrong one — which is why this is
Medium and not the High band's "engine denial of service from a single request". A single request does not
take the engine down: it pins one task, and the caller's disconnect is expected to drop the handler future.
But nothing in the engine enforces that, and nothing bounds how many such tasks accumulate; the bound lives
entirely in the caller, in another crate, and would be gone the moment a caller retried without closing or a
second sentinel shared the engine.

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| - | ----- | ----- | -------- | -------------- |
| 1 | The caller's timeout budget is parsed and then explicitly discarded; this is the `known` TODO from codebase-map § 4. | E2 | `crates/sentinel-engine/src/api/mod.rs:44-54` | <pre>    if let Some(request_id) = request_id {<br>        span.record("request_id", field::display(request_id));<br>    }<br><br>    // TODO: Pass this parameter to the engine once it supports request<br>    // lifecycle context.<br>    let _ = timeout;<br><br>    let context = CheckContext {<br>        block: request.block,<br>    };</pre> |
| 2 | The budget really is a parsed, usable `Duration` by the time it is dropped — the information is available and thrown away, not missing. | E2 | `crates/sentinel-engine/src/api/extractors.rs:41-49` | <pre>    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {<br>        let timeout = parse_header(parts, "x-request-timeout", \|\| {<br>            (<br>                StatusCode::BAD_REQUEST,<br>                "x-request-timeout must be an unsigned integer number of milliseconds",<br>            )<br>        })?;<br>        Ok(Self(timeout.map(Duration::from_millis)))<br>    }</pre> |
| 3 | The router applies exactly one layer, `TraceLayer`: no timeout, no concurrency limit, no body-limit override. | E2 | `crates/sentinel-engine/src/api/mod.rs:24-31` | <pre>/// Constructs the transaction-checking API.<br>pub fn router(engine: SentinelEngine) -> Router {<br>    let engine = Arc::new(engine);<br>    Router::new<br>        .route("/v1/security-check", post(security_check))<br>        .with_state(engine)<br>        .layer(TraceLayer::new_for_http)<br>}</pre> |
| 4 | The crate's dependency list does not take `tower` (the workspace declares it at `Cargo.toml:23`, but this crate does not use it), so `tower::timeout::TimeoutLayer` and `tower::limit::ConcurrencyLimitLayer` are not available to it. | E2 | `crates/sentinel-engine/Cargo.toml:7-21` | <pre>[dependencies]<br>alloy.workspace = true<br>argh.workspace = true<br>async-trait.workspace = true<br>axum.workspace = true<br>reqwest.workspace = true<br>safenet-core.workspace = true<br>serde.workspace = true<br>serde_json.workspace = true<br>thiserror.workspace = true<br>tokio.workspace = true<br>toml.workspace = true<br>tower-http.workspace = true<br>tracing.workspace = true<br>url.workspace = true</pre> |
| 5 | `tower-http` is taken with only the `trace` feature, so its `timeout`, `limit` and `catch-panic` middlewares are not compiled in either. | E2 | `Cargo.toml:24` | <pre>tower-http = { version = "0.7", features = ["trace"] }</pre> |
| 6 | The CoW client is a bare `reqwest::Client::new` — no `.timeout(..)`, no connect timeout. | E2 | `crates/sentinel-engine/src/checkers/cow.rs:228-231` | <pre>impl CowChecker {<br>    pub fn new -> Self {<br>        Self::with_client(reqwest::Client::new)<br>    }</pre> |
| 7 | The outbound request to the third party interpolates the proposer-supplied `order_uid` and awaits it with no deadline. | E2 | `crates/sentinel-engine/src/checkers/cow.rs:196-204` | <pre>        Ok(self<br>            .client<br>            .get(format!("{base_url}/api/v1/orders/{order_uid}"))<br>            .send<br>            .await?<br>            .error_for_status?<br>            .json<br>            .await?)</pre> |
| 8 | The RPC provider is built with an observability layer and no timeout configuration; every `eth_getLogs` inherits it. | E2 | `crates/core/src/provider/mod.rs:128-137` | <pre>    /// Connects to `url`.<br>    pub async fn connect(url: &Url) -> Result<Self, TransportError> {<br>        let client = ClientBuilder::default<br>            .layer(ObservabilityLayer)<br>            .connect(url.as_str)<br>            .await?;<br>        let root = RootProvider::new(client);<br>        let chain_id = root.get_chain_id.await?;<br>        Ok(Self { root, chain_id })<br>    }</pre> |
| 9 | The serve loop adds no deadline of its own either — `tokio::select!` between `axum::serve` and the shutdown signal, nothing more. | E2 | `crates/sentinel-engine/src/main.rs:75-84` | <pre>    let listener = TcpListener::bind(bind_address).await?;<br>    let local_address = listener.local_addr?;<br><br>    tracing::info!(%local_address, "starting sentinel engine");<br>    tokio::select! {<br>        result = axum::serve(listener, api::router(engine)) => result?,<br>        _ = utils::shutdown_signal => {<br>            tracing::info!("received shutdown signal; stopping sentinel engine");<br>        }<br>    }</pre> |
| 10 | The published interface contract advertises the header as something an engine uses to avoid starting work it cannot finish. | E2 | `crates/sentinel-engine/openapi.yaml:23-32` | <pre>        - name: x-request-timeout<br>          in: header<br>          required: false<br>          description: >-<br>            The caller's timeout budget for the request, in milliseconds. An<br>            engine can use this value to avoid starting work that cannot finish<br>            before the caller stops waiting.<br>          schema:<br>            type: integer<br>            minimum: 0</pre> |
| 11 | `reqwest::Client::new` applies no request timeout by default, and hyper drops an in-flight handler future when the client closes the connection. | I | Neither `reqwest` 0.13 nor `hyper` 1.10.1 source is on this machine (A6, no registry, no network) and nothing was executed. Both are documented library behaviours, but I did not verify either. | *(no verbatim quote available)* |

## Trigger

Under A2 the proposer picks the path:

1. **CoW lookup.** Propose a two-call batch of `approve(GPv2VaultRelayer, n)` plus
   `setPreSignature(<orderUid>, true)` on a supported chain (1, 100 or 42161). The engine reaches
   `CowChecker` (checker #7, `main.rs:64`) and issues `GET https://api.cow.fi/api/v1/orders/0x<uid>` with no
   deadline. `orderUid` is the proposer's `bytes`, so its length is bounded only by the request body, and a
   multi-kilobyte UID is rendered straight into the URL (`cow.rs:198`). If `api.cow.fi` is slow, rate-limiting
   the engine, or blackholing, the handler awaits until the sentinel gives up.
2. **RPC fan-out.** Propose `transfer(<recipient>, amount)` to any ERC-20-shaped target with a matching
   `chainId`. `AddressPoisoningChecker` (checker #10, `main.rs:72`) issues its chunked `eth_getLogs` walk
   sequentially with no per-call deadline; a provider that stalls on the first chunk stalls the request.
3. **The header is inert.** Send the same request with `x-request-timeout: 1` (one millisecond). The response
   still takes as long as the checkers take; the value is parsed at `extractors.rs:42-48` and dropped at
   `api/mod.rs:50`.

## Considered and rejected

- **"A3 makes this Informational: only the co-deployed sentinel can call the engine."** Rejected, and the
  distinction matters. A3 makes *missing authentication and rate limiting* Informational, and I have not filed
  those. This finding is not about who may call — it is about work the engine performs on behalf of a legitimate
  caller, whose duration is chosen by the **transaction proposer**, who is adversarial under A2 and is not the
  caller. A3 does not constrain the proposer at all.
- **"The sentinel's own client timeout already bounds it."** Partly true and explicitly accounted for in the
  Claim — it is why the severity is Medium. But it bounds the *caller's* wait, not the engine's work; it lives
  in `crates/sentinel/src/engine.rs`, i.e. in a different crate that an operator running a third-party
  sentinel against this reference engine may not have; and it depends on basis row 11's unverified claim that
  a client disconnect actually cancels the handler future. An engine that is documented as a replaceable
  reference implementation should not rely on its caller for its own resource bounds.
- **"An unbounded `x-request-timeout` value would itself be a defect."** Rejected: `u64::from_str` then
  `Duration::from_millis` (`extractors.rs:42-48`) cannot panic or overflow for any `u64`, and the value is
  discarded anyway. If it is honoured later, an upper clamp becomes worth adding — noted in remediation.
- **"There is no body-size limit either, so a huge body is the real DoS."** Rejected as a separate defect:
  `api/mod.rs:25-31` sets no `DefaultBodyLimit`, but axum 0.8.9 applies its documented 2 MiB default to the
  `Json` extractor. I could not verify that from source (no registry, A6), so I record it as class `I` and do
  not build a finding on it. Note the direction: if that default did *not* apply, the exposure would be worse,
  not better — so declining to claim it is the conservative choice.
- **"A hung checker produces a wrong verdict."** Rejected. `Checker::check` returns a `Verdict`, not a
  `Result`, and both externally-backed checkers map a failed lookup to `Verdict::Abstain`, so the failure mode
  is a missing vote, never `Secure` or `Insecure`. That is what keeps this out of the Critical band.
- **"Adding a timeout is trivially safe."** Not quite, and remediation says so: a `TimeoutLayer` that fires
  mid-chain would turn a completed `Insecure` into a 408, which the sentinel maps to `Unknown` — trading a
  denial for a missing vote. The deadline has to be threaded into the chain to be strictly better, which is
  precisely what the TODO at `api/mod.rs:48-49` describes ("once it supports request lifecycle context").

## Remediation options

1. **Bound the outbound calls first — the highest value for the least risk.** Give `CowChecker::new` a
   `reqwest::ClientBuilder::new.timeout(..).connect_timeout(..)` client, and configure a per-call timeout on
   the alloy transport in `Provider::connect`. This turns an indefinite hang into a prompt `Abstain` with the
   `warn!` the checkers already emit, changes no verdict semantics, and needs no new dependency. It also caps
   the RPC fan-out in F-ENG-009 in wall-clock terms even while the call count stays unbounded.
2. **Honour `x-request-timeout` by threading a deadline through `CheckContext`.** `CheckContext`
   (`engine/mod.rs:20-33`) is already the per-request carrier for "caller-supplied hints that aren't part of
   the transaction itself" and already carries `block`; adding a deadline is the change the TODO anticipates.
   Each checker can then decide between abstaining early and finishing. Clamp the accepted value to a sane
   maximum on the way in. Tradeoff: every checker needs a policy for a deadline that expires mid-check.
3. **Add a server-side backstop.** Enable `tower-http`'s `timeout` feature (and take `tower` for
   `ConcurrencyLimitLayer`), and set an explicit `DefaultBodyLimit` rather than relying on axum's default.
   Tradeoff as above: a layer-level timeout can only return a status code, converting a would-be denial into a
   missing vote, so it should be set well above the in-chain deadline as a last resort rather than as the
   primary mechanism. Enabling `catch-panic` at the same time is worth considering — today a panic anywhere in
   the chain drops the connection rather than returning a 500 (see the R8 coverage log).

Tests to add: an `api` test asserting the handler observes the parsed deadline once option 2 lands; a
`CowChecker` test with a fake `OrderApi` that never resolves, asserting the check abstains within the
deadline; and an `AddressPoisoningChecker` test using `Provider::mocked_with_chain` with a stalling asserter.
`api/mod.rs` and `api/extractors.rs` currently have **zero** tests.

## Trail

- Reviewer R8: drafted, self-estimate 90%. Rows 1-10 are direct reads of this checkout; row 11 is
  class `I` and marked so — no dependency source is on disk (A6) and nothing was executed. Confirms ENG-H10,
  and adds the dependency-level evidence (rows 4-5) that the middlewares are absent from the build rather than
  merely unused, which the prior analysis did not have. Tagged `known` for the `api/mod.rs:48` TODO per A12,
  and filed at reduced priority accordingly; the outbound-client half is not covered by that TODO.

## Critic (C-ENG-A)

I traced the request path and both outbound clients before reading the argument. **No claim is `H`**; all
eleven `Basis` rows were re-opened and the two that depend on library defaults (row 11: `reqwest`'s default
and hyper's cancel-on-disconnect) are correctly marked `I`, which is the right call under A6 with no
dependency source on disk.

### Per-claim verdicts

| Claim | Verdict | Re-opened |
| ----- | ------- | --------- |
| `x-request-timeout` parsed then discarded | **Supported** | `api/mod.rs:48-50`, `// TODO: Pass this parameter to the engine once it supports request lifecycle context.` / `let _ = timeout;`, with `extractors.rs:42-48` producing a real `Duration`. |
| Router carries only `TraceLayer` | **Supported** | `api/mod.rs:25-31`. |
| The middlewares are **not compiled in** | **Supported, and this is the reviewer's best original contribution** | `crates/sentinel-engine/Cargo.toml:7-21` lists no `tower`; root `Cargo.toml:23-24` is `tower = "0.5"` (workspace only) and `tower-http = { version = "0.7", features = ["trace"] }`. So `TimeoutLayer`, `ConcurrencyLimitLayer`, `RequestBodyLimitLayer` and `CatchPanicLayer` are unavailable, not merely unused. The prior analysis missed this. |
| CoW client has no timeout | **Supported** | `cow.rs:228-231` `Self::with_client(reqwest::Client::new)`; `cow.rs:196-204` `.get(..).send.await?` with no `.timeout(..)`. |
| Provider has no timeout | **Supported** | `crates/core/src/provider/mod.rs:129-137`, `ClientBuilder::default.layer(ObservabilityLayer).connect(url.as_str)` — no timeout configuration anywhere in the builder chain. |
| The proposer picks which outbound path runs | **Supported** | `cow.rs:421-437` gates on `transaction.chain_id` then dispatches on the batch shape; `address_poisoning.rs:192-213` on the ERC-20 shape. Both are decided by the transaction's own fields. |

### Does A3 cap this at Informational? No — and it is worth stating why precisely

A3 caps a specific class: *"Missing authentication or rate limiting on it is Informational unless a bypass
exists within that deployment."* It is a statement about **who can reach the API**. R8 correctly declined to
file the missing-auth finding at all under it, and I looked for a bypass independently and found none — the
engine opens exactly one listener (`main.rs:75`, `TcpListener::bind(bind_address)`) defaulting to
`127.0.0.1:5473` (`config.rs:57-59`).

This finding is not in that class. The attacker never touches the API. They propose a Safe transaction
onchain; the **honest** sentinel reads it and forwards it to its own engine over the trusted loopback
channel; and the transaction's own fields — attacker-controlled under **A2** — then select which outbound
call the engine makes and how long it takes. A3 is silent about the *content* of a legitimate request from
the co-deployed sentinel. So the proposer-controlled path does escape A3, and the finding is properly filed.

### Severity: Medium → **Low**

I part company with the reviewer here, on evidence R8 did not cite. The blast radius is bounded by the
caller after all, and the caller is in this repository:

```
crates/sentinel/src/engine.rs:154-161
    /// Configure the timeout for the security check.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.request = self.request.timeout(timeout).header(
            "x-request-timeout",
            u64::try_from(timeout.as_millis).unwrap_or(u64::MAX),
        );
```

The sentinel sets a real `reqwest` client-side timeout on every check, not just the advisory header. So the
concrete outcome of a hung path is: one check exceeds the sentinel's budget, the sentinel gives up, the
outcome is `CheckOutcome::Unknown` and the request is dropped unanswered (`service.rs:176-179`) — the
attacker suppresses a vote **on their own transaction**, which is not an approval and carries no slashing
exposure (no commitment was made). There is no demonstrated engine-side amplification: no path where one
request degrades service for other requests, which is what PROMPT § 8's High band ("engine denial of service
from a single request") and its Medium band both need. Combined with A12 — the headline item is the `known`
TODO in codebase-map § 4, to be filed "at reduced priority" — **Low** is the honest band.

R8's real point survives the downgrade and should be kept in the report's wording: *the bound lives entirely
in the caller, in another crate*. Nothing in the engine enforces it, the engine advertises a header it
ignores, and a second sentinel, a retry without closing, or any caller that omits `.timeout(..)` removes the
bound with no engine-side change. That is a design weakness worth fixing; it is not today an availability
defect.

### Finding verdict and certainty

**Confirmed — 80%.** Mechanism fully verified; trigger concrete for all three Trigger items. R8's
self-estimate of 90% is above the ceiling this run allows: `state/baseline.md` § 2 fixes 89% as the maximum
absent `E1`, and no test, PoC or tool output exists for this finding. 80 rather than 89 because row 11's
`I` premises (whether hyper actually cancels the handler on client disconnect) materially affect how bad
this is, and neither can be settled without running something.

### Relationship to F-ENG-043 (R9)

**Overlapping, not duplicate.** F-ENG-043 is the CoW-specific version — no client timeout *plus* an
unbounded attacker-controlled `orderUid` interpolated into the URL, which is a separate defect this file
does not make. **Canonical: F-ENG-005** for the engine-wide absence of any deadline (server layer, the
discarded header, the RPC provider); **F-ENG-043** for the CoW client and its URL construction. The report
must not count the CoW timeout twice.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: every
trigger is a *hang*, which needs a running server and a stalling peer, and the pivotal question (does an
un-timed `reqwest` request hang indefinitely?) is a dependency-behaviour question, not a code-reading one.
It is already filed as **Q4** in `rust-audit/poc/UNRESOLVED-DEPENDENCY-QUESTIONS.md` with the exact check.

**Certainty unchanged at 80%. Severity unchanged.** I confirmed the code-side facts by inspection —
`api/mod.rs:48-50` parses `x-request-timeout` and discards it with `let _ = timeout;`; `main.rs:79-84` has
no `TimeoutLayer`, no `ConcurrencyLimitLayer` and no explicit body limit; `CowChecker::new`
(`cow.rs:229-231`) builds a bare `reqwest::Client::new`; `Provider::connect`
(`crates/core/src/provider/mod.rs:123-134`) sets no per-call transport timeout — but the *consequence*
depends on library defaults I cannot read (A6).

### Remediation check

- **Option 1 (bound the outbound calls first) — sound, and correctly ordered as the highest value for the
  least risk.** It changes no verdict semantics: a timeout becomes the `Abstain` the checkers already emit
  on a failed lookup, with the `warn!` they already log. `cow.rs:234` (`CowChecker::with_client`) exists as
  the seam, so the CoW half is a one-line change at the call site in `main.rs`. **Take this first.**
- **Option 2 (thread a deadline through `CheckContext`) — sound, and it is the right long-term home**;
  `CheckContext` is documented as the carrier for "caller-supplied hints that aren't part of the
  transaction itself" and already carries `block`. Two things to get right that the finding notes only in
  passing: **clamp the accepted value** (an attacker-supplied `x-request-timeout` of `u64::MAX` is an
  amplifier, and a value of `0` must not mean "no deadline"), and give each checker an explicit policy for
  a deadline that expires mid-check — the default should be `Abstain`, never a guessed verdict.
- **Option 3 (a server-side backstop) — sound, with the caveat the finding already states and which is the
  important one:** a layer-level timeout can only return a status code, converting a would-be *denial* into
  a missing vote. Set it well above the in-chain deadline as a last resort. The `catch-panic` suggestion is
  worth taking on its own merits and is nearly free.
- **An interaction the finding does not raise, and the report should.** If F-ENG-044 option 1 (conjunctive
  affirmation) ships, **every** transaction that any checker affirms will additionally run the two
  RPC-backed checkers and possibly the CoW lookup, so the per-request latency ceiling rises sharply.
  Option 1 here is therefore not merely worthwhile — it is a **precondition** for the F-ENG-044 fix.
  Sequence: F-ENG-005 option 1, then F-ENG-009 option 1, then F-ENG-044.
- **Test hook: mostly missing, and it is a real gap.** `api/mod.rs` and `api/extractors.rs` have **zero**
  tests, so there is nothing to extend for the handler-side assertions. The checker-side hooks exist
  (`CowChecker::with_order_api` for a never-resolving `OrderApi`; `Provider::mocked_with_chain` for a
  stalling asserter). Add `axum`'s test support (or `tower::ServiceExt::oneshot`) to exercise the router —
  that is infrastructure the crate does not have today and which F-ENG-008 also needs.
- **Where the fix belongs: the transport/config layer and the API handler**, not the checkers and not the
  `RuleId` mapping.
