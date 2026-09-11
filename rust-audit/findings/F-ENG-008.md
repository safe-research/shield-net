# F-ENG-008 `openapi.yaml`, the declared authoritative interface contract, documents only `200` while the engine provably returns `400`s, and it advertises `x-request-timeout` semantics the reference engine does not implement

| Field                | Value                                                                                     |
| -------------------- | ----------------------------------------------------------------------------------------- |
| Status               | QA-done                                                                                         |
| Crate and module     | sentinel-engine, `openapi.yaml`                                                             |
| Location             | `crates/sentinel-engine/openapi.yaml:39-48` and `:23-32` (related: `crates/sentinel-engine/src/api/extractors.rs:20-28`, `41-49`; `crates/sentinel-engine/src/api/mod.rs:33-38`, `48-50`; `crates/sentinel-engine/openapi.yaml:181-186`) |
| Severity             | Informational / Informational |
| Certainty            | 80% |
| Assumptions involved | A3, A6                                                                                      |
| Tags                 | input-validation, deps                                                                      |

## Claim

`openapi.yaml` is not incidental documentation — `docs/sentinel-engine.md:29` names it "the authoritative
interface contract" and tells operators writing their own engine to "validate against and remain compatible
with that document". Three things in it do not match the engine it describes.

**The response set is incomplete.** The `responses:` block documents exactly one status, `200`. The engine
provably returns `400 text/plain` for a malformed `x-request-id` or `x-request-timeout` — both extractors
declare `Rejection = (StatusCode, &'static str)` and return `StatusCode::BAD_REQUEST` with a fixed message,
before the body is read. Beyond that, the `Json` extractor, the router's method/path fallbacks and the default
body limit contribute the usual `415`, `422`, `405`, `404` and `413`, and, because `tower-http`'s
`catch-panic` feature is not enabled, a panic anywhere in the checker chain closes the connection with no
response at all. A third-party engine written strictly to this spec would emit none of these; a third-party
*sentinel* written strictly to it would have no defined handling for any of them.

**The timeout parameter is documented as functional and is not.** The parameter description says an engine
"can use this value to avoid starting work that cannot finish before the caller stops waiting". The reference
engine parses it and discards it (`api/mod.rs:48-50`, the `known` TODO — see F-ENG-005). The wording is
permissive enough ("can use") that it is not false, but the spec is the only place a reader learns the
parameter exists, and it gives no signal that the reference implementation ignores it.

**The `Quantity` schema admits values the engine must reject.** Its pattern constrains the *form* — minimal,
lower-case, `0x`-prefixed hex — but not the length, while the description says "A 256-bit unsigned integer".
A 100-digit hex string satisfies the published pattern and fails deserialisation into `U256`. The schema
should carry the bound its own description states.

None of this is exploitable and none of it is inflated by A3: the caller is trusted, and the drift is the
implementation being *more* permissive or *differently* shaped than the contract, never stricter. It is filed
because this file is the interoperability surface the project has chosen to publish, and an interface contract
that omits every error response is the kind of gap that only surfaces when a second implementation appears.

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| - | ----- | ----- | -------- | -------------- |
| 1 | The spec documents exactly one response status for the only endpoint. | E2 | `crates/sentinel-engine/openapi.yaml:39-48` | <pre>      responses:<br>        "200":<br>          description: >-<br>            The verdict. Note that `abstain` is a successful response: the<br>            engine is directing the sentinel not to vote because it has no<br>            definitive answer.<br>          content:<br>            application/json:<br>              schema:<br>                $ref: "#/components/schemas/SecurityCheckResponse"</pre> |
| 2 | The `x-request-id` extractor returns an undocumented `400` with a plain-text message. | E2 | `crates/sentinel-engine/src/api/extractors.rs:18-28` | <pre>    type Rejection = (StatusCode, &'static str);<br><br>    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {<br>        let request_id = parse_header(parts, "x-request-id", \|\| {<br>            (<br>                StatusCode::BAD_REQUEST,<br>                "x-request-id must be a 0x-prefixed 32-byte digest",<br>            )<br>        })?;<br>        Ok(Self(request_id))<br>    }</pre> |
| 3 | So does the `x-request-timeout` extractor, with its own message. | E2 | `crates/sentinel-engine/src/api/extractors.rs:39-49` | <pre>    type Rejection = (StatusCode, &'static str);<br><br>    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, Self::Rejection> {<br>        let timeout = parse_header(parts, "x-request-timeout", \|\| {<br>            (<br>                StatusCode::BAD_REQUEST,<br>                "x-request-timeout must be an unsigned integer number of milliseconds",<br>            )<br>        })?;<br>        Ok(Self(timeout.map(Duration::from_millis)))<br>    }</pre> |
| 4 | Both extractors run before the body extractor, so those rejections happen on requests whose bodies are perfectly valid. | E2 | `crates/sentinel-engine/src/api/mod.rs:33-38` | <pre>async fn security_check(<br>    State(engine): State<Arc<SentinelEngine>>,<br>    RequestId(request_id): RequestId,<br>    RequestTimeout(timeout): RequestTimeout,<br>    Json(request): Json<CheckRequest>,<br>) -> Json<Verdict> {</pre> |
| 5 | The spec describes `x-request-timeout` as something an engine uses to avoid starting unfinishable work. | E2 | `crates/sentinel-engine/openapi.yaml:23-32` | <pre>        - name: x-request-timeout<br>          in: header<br>          required: false<br>          description: >-<br>            The caller's timeout budget for the request, in milliseconds. An<br>            engine can use this value to avoid starting work that cannot finish<br>            before the caller stops waiting.<br>          schema:<br>            type: integer<br>            minimum: 0</pre> |
| 6 | The reference engine discards it. | E2 | `crates/sentinel-engine/src/api/mod.rs:48-50` | <pre>    // TODO: Pass this parameter to the engine once it supports request<br>    // lifecycle context.<br>    let _ = timeout;</pre> |
| 7 | The `Quantity` schema states a 256-bit bound in prose but its pattern imposes no length limit. | E2 | `crates/sentinel-engine/openapi.yaml:181-186` | <pre>    Quantity:<br>      type: string<br>      description: >-<br>        A 256-bit unsigned integer as minimal (no leading zeros), lower-case,<br>        `0x`-prefixed hex string. Zero is "0x0".<br>      pattern: "^0x([1-9a-f][0-9a-f]*\|0)$"</pre> |
| 8 | Every quantity field in the request is a `U256` or a `u64`, so an over-long but pattern-valid value fails deserialisation. | E2 | `crates/sentinel-engine/src/engine/transaction.rs:57-68` | <pre>pub struct SafeTransaction {<br>    /// The chain the transaction is to execute on.<br>    pub chain_id: U256,<br>    /// The Safe executing the transaction.<br>    #[serde(serialize_with = "checksummed_address::serialize")]<br>    pub safe: Address,<br>    #[serde(serialize_with = "checksummed_address::serialize")]<br>    pub to: Address,<br>    pub value: U256,<br>    pub data: Bytes,<br>    pub operation: Operation,<br>    pub safe_tx_gas: U256,</pre> |
| 9 | The document is the project's declared authoritative contract for third-party engines, which is what makes the omissions matter. | E2 | `docs/sentinel-engine.md:29` (reference-only material — cited as context, not as the subject of this finding) | <pre>[`crates/sentinel-engine/openapi.yaml`](../crates/sentinel-engine/openapi.yaml) is the authoritative interface contract. It defines the `POST /v1/security-check` request and response bodies, wire formats, and the optional `x-request-id` and `x-request-timeout` headers. Operators implementing their own engine should validate against and remain compatible with that document.</pre> |
| 10 | The remaining undocumented statuses (`415`, `422`, `413`, `405`, `404`) and the no-response-on-panic case come from axum defaults and from `tower-http`'s `catch-panic` feature being absent. | I | axum 0.8.9's rejection-to-status mapping, its `DefaultBodyLimit`, and its method/path fallbacks are library behaviour whose source is not on this machine (A6, no registry, no network); nothing was executed. The `catch-panic` half is E2 from `Cargo.toml:24` taking `tower-http` with only `["trace"]`. | *(no verbatim quote available for the axum half)* |

## Trigger

`curl -X POST http://127.0.0.1:5473/v1/security-check -H 'content-type: application/json'
-H 'x-request-id: not-a-digest' -d '<any valid CheckRequest>'` returns
`400` with the body `x-request-id must be a 0x-prefixed 32-byte digest` — a status and media type the spec
does not mention. Same with `x-request-timeout: -1` or `x-request-timeout: abc`.

For the `Quantity` case: a `value` of `"0x1"` followed by 100 more hex digits matches
`^0x([1-9a-f][0-9a-f]*|0)$` and is therefore a spec-valid request, and fails to deserialise into `U256`.

## Considered and rejected

- **"A3 makes this Informational."** Agreed, and that is the severity filed — but note A3 is not the load
  bearing reason. Even with a fully trusted caller the contract is what a *second implementation* is written
  against, and `docs/sentinel-engine.md:29` invites exactly that. The harm is interoperability, not security.
- **"The sentinel already handles non-2xx, so nothing breaks."** True for *this* sentinel — it maps every
  request failure to `CheckOutcome::Unknown` (`crates/sentinel/src/engine.rs:176-187`) — and that is why this
  is not higher. It is not true for an arbitrary client written to the published contract.
- **"The `RuleId` schema being an open set while `RuleId::from_code` accepts only six codes is a matching
  defect."** Checked and rejected. `openapi.yaml:161-168` deliberately documents the rule set as open
  ("This is not a closed set as the security Charter is an evolving document"), the engine only ever *emits*
  codes on this endpoint, and the consuming side is permissive: the sentinel parses any `R-<u32>.<u32>` rather
  than using the engine's enum. The strict `Deserialize` at `engine/rule.rs:92-101` is never applied to a
  response the engine receives.
- **"The `Digest` and `Address` patterns are stricter than the parsers, so requests could be rejected."**
  Rejected — the drift is the other way and therefore harmless. `openapi.yaml:169-174` requires lower-case
  for `x-request-id` while `B256::from_str` accepts either case; `openapi.yaml:175-180` allows mixed-case
  addresses and `transaction.rs:78-91` documents accepting any case deliberately, pinned by
  `transaction.rs:143-157`. A conforming caller is always accepted.
- **"The missing `x-request-timeout` implementation is already F-ENG-005."** Yes, and that is where the
  behavioural finding lives; here it is one of three spec-level defects and is cross-referenced rather than
  re-argued. If F-ENG-005 option 2 is taken, this half of F-ENG-008 disappears on its own.
- **"Row 10's statuses should be dropped since they are unverified."** Rejected: the finding stands on rows
  1-4 alone, which are pure repository facts — the spec says one status, the code demonstrably returns
  another. Row 10 only broadens the list, and I have marked it `I` rather than assert axum's mapping.

## Remediation options

1. **Document the real response set.** Add `400`, `413`, `415`, `422` (and `405`/`404` if the spec is meant to
   describe the whole surface) with a `text/plain` content type, and note that error bodies are unstructured.
   Cheapest and immediately useful to anyone writing a second engine or a second client. Tradeoff: it pins
   axum's current mapping into the contract, so a framework upgrade that changes a status becomes a
   contract change — arguably a feature.
2. **Define a structured error body and make the engine emit it.** Give the extractors and a `Json` rejection
   handler a small `{"error": "..."}` shape and document it. More work, but it turns "the request was bad" into
   something a caller can log usefully, and it removes the dependency on framework defaults. Pairs naturally
   with enabling `tower-http`'s `catch-panic` so a panic becomes a documented `500` rather than a dropped
   connection.
3. **Tighten the schemas to what the implementation accepts.** Add `maxLength: 66` to `Quantity` (the widest
   valid `U256`, `0x` + 64 digits) and `maxLength: 42` to `Address`, so the published schema cannot describe a
   request the engine must reject. Consider also documenting the request-body size limit, which today is an
   undocumented framework default (see F-ENG-005).
4. **Add `security: []` explicitly.** The API is deliberately unauthenticated (A3,
   `docs/sentinel-engine.md`), and saying so in the spec is clearer than omitting the field.

Tests to add: a spec-conformance test that starts the router and asserts each documented status and body
shape, which would also close the gap that `api/mod.rs` and `api/extractors.rs` have **zero** tests today —
neither the `400` paths nor the happy path is exercised anywhere in the crate.

## Trail

- Reviewer R8: drafted, self-estimate 90%. Rows 1-9 are direct reads of this checkout; row 10 is
  class `I` under A6 for the axum half and E2 for the `catch-panic` half. `docs/sentinel-engine.md` is
  reference-only per the brief §4 and is cited as context only — the finding is against `openapi.yaml`, which
  is in my assigned scope. Overlaps F-ENG-005 on the timeout parameter by design.

## Critic (C-ENG-A)

I read `openapi.yaml` against the handler before reading the argument. **No claim is `H`.**

### Per-claim verdicts

**Supported — the response set.** `crates/sentinel-engine/openapi.yaml:39-48` has a `responses:` block
containing exactly one entry, `"200"`. The engine provably returns `400` before the body is read:
`api/extractors.rs:18` / `:22-25` and `:39` / `:43-46` both declare
`type Rejection = (StatusCode, &'static str)` returning `StatusCode::BAD_REQUEST`. Confirmed as the strongest
half of the finding, and the one a third-party implementer would actually be hurt by.

**Supported — the `Quantity` schema.** `openapi.yaml:181-186`: `pattern: "^0x([1-9a-f][0-9a-f]*|0)$"` with
the description "A 256-bit unsigned integer". The pattern bounds the form, not the length; a 100-digit hex
string satisfies it and fails `U256` deserialisation. A `maxLength` would state the bound the description
already claims.

**Supported but weaker than the title — the timeout parameter.** The title says the file "advertises
`x-request-timeout` semantics the reference engine does not implement". `openapi.yaml:26-29` actually reads
"An engine **can** use this value to avoid starting work that cannot finish before the caller stops waiting"
— permissive, not mandatory, so the reference engine ignoring it is not a contract violation. The Claim
concedes this in its own body ("The wording is permissive enough ('can use') that it is not false"), which
is honest, but the **title does not**, and the title is what reaches the report's summary table. I flag this
as a title/body mismatch for the Documentation agent: the sharp, defensible half of this finding is the
undocumented error responses, and the title should lead with that. I have not edited the reviewer's text.

**Supported — the catch-panic consequence.** Root `Cargo.toml:24` is
`tower-http = { version = "0.7", features = ["trace"] }`, so `CatchPanicLayer` is not compiled in and a
panic in the checker chain closes the connection with no status at all. Correctly stated, and correctly not
inflated: I found no reachable panic to go with it (see below).

### Severity and certainty

**Informational / Informational — confirmed.** Correct band and correctly justified: the drift is the
implementation being *more* permissive than the published contract in every instance, never stricter, so no
conforming caller is rejected. A3 does not change this either way. What makes it worth filing at all is that
`docs/sentinel-engine.md:29` names this file "the authoritative interface contract" for third-party engines
— an interoperability surface that omits every error response is a gap that only surfaces when a second
implementation appears.

**Confirmed — 80%.** R8's 90% self-estimate exceeds this run's ceiling: `state/baseline.md` § 2 fixes 89% as
the maximum absent `E1`, and nothing here was executed. 80 rather than 89 because the timeout sub-claim is
Plausible rather than Confirmed on the reading above, and because the full set of statuses axum contributes
(`415`, `422`, `405`, `404`, `413`) is class `I` under A6 — no axum source on disk. The `400`s from the two
extractors are `E2` and settle the finding on their own.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the
finding is a contract-vs-implementation mismatch, and demonstrating it needs the router running, which the
crate has no harness for. The one library-behaviour question it rests on (does axum's `JsonRejection` echo
a fragment of a malformed body?) is already **Q10** in
`rust-audit/poc/UNRESOLVED-DEPENDENCY-QUESTIONS.md`.

**Certainty unchanged at 80%. Severity unchanged at Informational.** Confirmed by inspection that the
engine can return `400` from paths the spec does not document: `api/extractors.rs:22-26` and `:43-47`
return `(StatusCode::BAD_REQUEST, …)` for a malformed `x-request-id` or `x-request-timeout`, and
`Json<CheckRequest>` rejects on `deny_unknown_fields` (`api/mod.rs:15`), on an unknown `RuleId` code
(`engine/rule.rs:97-99`) and on an operation byte outside `{0,1}` (`engine/transaction.rs:41-49`).

### Remediation check

- **Option 1 (document the real response set) — sound, cheapest, immediately useful.** The finding's own
  caveat is the interesting part and I agree with its resolution: pinning axum's current status mapping
  into the contract means a framework upgrade that changes a status becomes a contract change. That **is**
  a feature for a document declared authoritative for a second implementation, and it is the strongest
  argument for option 2 rather than against option 1.
- **Option 2 (define a structured error body and emit it) — sound, and it is the option that makes the
  contract independent of the framework** rather than a transcript of it. Pair it with `catch-panic` as the
  finding suggests: today a panic anywhere in the checker chain drops the connection rather than returning
  a documented `500`, which is a worse contract violation than any of the undocumented `400`s. One
  addition: whatever shape is chosen must **not** echo request content back, or Q10's question stops being
  hypothetical and becomes a log-injection surface.
- **Option 3 (tighten the schemas) — sound and free.** `maxLength: 66` on `Quantity` and `42` on `Address`
  stop the published schema describing requests the engine must reject. Documenting the body-size limit
  matters more than it looks, because today that limit is an undocumented framework default (F-ENG-005), so
  a second implementation has no way to match it.
- **Option 4 (`security: []`) — sound and trivially correct.** The API is deliberately unauthenticated
  under A3; saying so beats omitting the field.
- **The finding's real value is the test gap it exposes, not the spec text.** `api/mod.rs` and
  `api/extractors.rs` have **zero** tests — neither the `400` paths nor the happy path is exercised
  anywhere in the crate — so nothing detects a divergence between the spec and the handler in either
  direction. A spec-conformance test needs a router harness, which is the same infrastructure F-ENG-005
  and F-ENG-007 need. **Build it once; it closes three findings' test gaps.** Note this is entirely outside
  `AGENTS.md`'s checker/corpus rule, which is about verdicts — there is no policy reason the API layer is
  untested.
- **Where the fix belongs: the interface contract (`openapi.yaml`) and the API layer.** Not the checkers,
  combinator or `RuleId` mapping.
