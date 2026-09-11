# F-ENG-043 The CoW order lookup has no client timeout and puts an unbounded, unvalidated attacker-controlled `orderUid` into the request URL

| Field                | Value                                                                          |
| -------------------- | ------------------------------------------------------------------------------ |
| Status               | QA-done                                                                             |
| Crate and module     | sentinel-engine, checkers/cow.rs                                                |
| Location             | crates/sentinel-engine/src/checkers/cow.rs:189-205, :228-239, :561-569          |
| Severity             | Low / Low                                                                   |
| Certainty            | 74% (Critic C-ENG-B; E2 ceiling, read-only run)                                          |
| Assumptions involved | A2, A3                                                                          |
| Tags                 | dos, input-validation, deps                                                     |

## Claim

`CowChecker::new` builds a bare `reqwest::Client::new` — no connect timeout, no total-request timeout — and
`ReqwestOrderApi::fetch_order` interpolates the presignature's `orderUid` straight into the request path. Two
consequences, both reachable from a single proposed Safe transaction:

1. **No deadline.** If `api.cow.fi` accepts the connection and then stalls, the handler future is pinned until
   the sentinel's own client disconnects. There is no server-side timeout either — the `x-request-timeout` header
   is parsed and discarded (`api/mod.rs`, item `checkers` cross-reference ENG-H10, R8's scope) — and no
   concurrency limit, so a stalled third party turns into unbounded in-flight handler tasks.
2. **Unbounded URL.** `orderUid` is the `bytes` argument of `setPreSignature`, i.e. attacker-chosen and bounded
   only by the JSON body limit. `presignature_order_uid` performs no length check — a genuine CoW order UID is
   exactly 56 bytes (`orderDigest(32) ‖ owner(20) ‖ validTo(4)`, which is what `compute_order_uid` builds), yet
   any length is accepted and rendered as `0x`-hex into `GET {base}/api/v1/orders/{order_uid}`. A ~2 MiB
   `orderUid` becomes a ~4 MB request line sent to a third party, once per proposal, for a lookup that cannot
   possibly succeed.

`Bytes`'s `Display` emits `0x`-prefixed hex only, so there is no path- or query-injection component to this; the
issue is amplification and liveness, not request smuggling. Under A3 the engine is reachable only by its
co-deployed sentinel, which bounds the request rate to on-chain proposal throughput — hence Low.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| - | ----- | ----------------- | -------- | -------------- |
| 1 | The request is issued with no timeout configured and the UID goes into the path | E2 | crates/sentinel-engine/src/checkers/cow.rs:189-205 | Q1 |
| 2 | The client is a default `reqwest::Client` (reqwest applies no default request timeout) | E2 (code) / I (reqwest default, registry not on disk) | crates/sentinel-engine/src/checkers/cow.rs:228-239 | Q2 |
| 3 | `orderUid` is taken from attacker calldata with no length or shape validation | E2 | crates/sentinel-engine/src/checkers/cow.rs:561-569 | Q3 |
| 4 | A genuine UID is exactly 56 bytes, as the checker's own recomputation shows | E2 | crates/sentinel-engine/src/checkers/cow.rs:143-169 | Q4 |

**Q1** `crates/sentinel-engine/src/checkers/cow.rs:191-204`

```rust
    async fn fetch_order(
        &self,
        base_url: &str,
        order_uid: &Bytes,
    ) -> Result<CowOrder, OrderLookupError> {
        Ok(self
            .client
            .get(format!("{base_url}/api/v1/orders/{order_uid}"))
            .send
            .await?
            .error_for_status?
            .json
            .await?)
    }
```

**Q2** `crates/sentinel-engine/src/checkers/cow.rs:228-239`

```rust
impl CowChecker {
    pub fn new -> Self {
        Self::with_client(reqwest::Client::new())
    }

    /// Backed by `client` rather than a freshly-constructed one, so a caller
    /// can share a single `reqwest::Client` across checkers.
    pub fn with_client(client: reqwest::Client) -> Self {
        Self {
            order_api: Box::new(ReqwestOrderApi { client }),
        }
    }
```

**Q3** `crates/sentinel-engine/src/checkers/cow.rs:561-569`

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

**Q4** `crates/sentinel-engine/src/checkers/cow.rs:164-169`

```rust
    let mut uid = [0u8; 56];
    uid[..32].copy_from_slice(digest.as_slice);
    uid[32..52].copy_from_slice(order.owner.as_slice);
    uid[52..].copy_from_slice(&order.valid_to.to_be_bytes);
    uid
}
```

## Trigger

A MultiSend delegatecall on chain 1 / 100 / 42161 with two packed `Call` entries:

1. `to = <any token>`, `data = approve(0xC92E8bdf79f0507f65a392b0ab4667716BFE0110, 1)`;
2. `to = 0x9008D19f58AAbD9eD0D60971565AA8510560ab41`,
   `data = setPreSignature(<orderUid of length L>, true)`.

`L = 1_000_000` yields an outbound `GET https://api.cow.fi/mainnet/api/v1/orders/0x<2,000,000 hex chars>` from
one 1 MB request body. The engine keeps that request open with no deadline; the response (a 414 or a rejected
connection) maps to `Verdict::Abstain` via `cow.rs:318-325`.

The liveness half of the trigger needs a stalling endpoint rather than a real one, so it is best encoded as a
unit test against a `tokio::net::TcpListener` that accepts and never responds, asserting that `fetch_order`
returns within a bounded time once a timeout is configured. A corpus vector can cover only the length half:
assert that an over-long `orderUid` produces `abstain` *without* an outbound request once a length check exists.

## Considered and rejected

- **`order_uid` could inject into the URL.** It cannot: `Bytes`'s `Display` is `0x` + lowercase hex, so no `/`,
  `?`, `#` or whitespace can appear. Amplification only.
- **The engine is exposed, so this is a remote DoS.** Under A3 only the co-deployed sentinel can reach it, and
  the sentinel issues one check per proposal, so the rate is bounded by on-chain proposal throughput. This is why
  the severity is Low rather than High despite there being no timeout anywhere on the path.
- **`error_for_status` bounds the work.** It is applied after the response headers arrive; it does nothing for a
  connection that never responds.
- **The presignature batch is rare, so the path is cold.** It is reachable from any two-call batch whose first
  entry is a relayer approval, which the proposer chooses freely; nothing about the shape is expensive to
  produce.
- **This is ENG-H10 and therefore R8's finding.** ENG-H10 covers the server-side deadline and the RPC client,
  which are in `api/mod.rs` and `core/provider`. The two facts cited here are inside `checkers/cow.rs` and are
  filed from this scope; the Manager should merge them if R8 files the same ground.

## Remediation options

1. Build the client with `reqwest::Client::builder.timeout(...).connect_timeout(...).build`, with the budget
   derived from the caller's `x-request-timeout` once that is threaded through (`api/mod.rs`'s TODO), or from a
   config value.
2. Reject a malformed UID before the lookup: `presignature_order_uid` returns `None` unless
   `call.orderUid.len == 56`. Removes the amplification entirely and costs nothing — a UID of any other length
   can never match `compute_order_uid`'s 56-byte output, so the request is always wasted.
3. Wrap the whole checker chain in a `tower::timeout::TimeoutLayer` so no single request can pin a handler
   indefinitely regardless of which external dependency stalls (overlaps with R8's ENG-H10).

Tests to add: a `presignature_order_uid` unit test for a 55- and a 57-byte UID; a `fetch_order` test against a
never-responding listener. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 88% (facts) / Low severity. Overlaps ENG-H10, which is R8's
  scope; filed here for the two `checkers/cow.rs` facts. reqwest's default-no-timeout behaviour is class I (the
  registry is not on disk); the missing length check is E2.

## Critic (C-ENG-B)

### 1. Per-claim verdicts

| # | Verdict | Re-opened |
| - | ------- | --------- |
| 1 | **Supported** | `cow.rs:191-204`: `self.client.get(format!("{base_url}/api/v1/orders/{order_uid}")).send.await?.error_for_status?.json.await`. No `.timeout(..)` on the request builder; the UID is interpolated straight into the path. |
| 2 | **Supported (code) / `I` (library default)** | `cow.rs:228-231`, `Self::with_client(reqwest::Client::new)`. That the resulting client applies no default request timeout is a `reqwest` behaviour, and no dependency source is on disk this run, so per the brief it cannot rise above `I`. R9 labels it exactly that way, which is the right call rather than a defect. |
| 3 | **Supported** | `cow.rs:561-569`: `setPreSignatureCall::abi_decode(&tx.data).ok.filter(|call| call.signed).map(|call| call.orderUid)`. `orderUid` is a `bytes` argument returned whole, with no length or shape check anywhere on the path to the URL. A genuine UID is 56 bytes — `compute_order_uid` (`cow.rs:143`) returns `[u8; 56]` — but nothing enforces that on the inbound side. |

R9's own scoping note is correct and worth keeping: `Bytes`'s `Display` emits `0x`-prefixed hex only, so there
is no path- or query-injection component. This is amplification and liveness, not request smuggling. A reviewer
who checked for the more alarming interpretation and then ruled it out has done the right thing.

### 2. Relationship to F-ENG-005 — I read C-ENG-A's Critic section, and I concur

**Overlapping, not duplicate**, and C-ENG-A's split is the correct one. Recording it from this side so both
files agree:

- **Canonical for the engine-wide absence of any deadline: F-ENG-005.** That file establishes what this one does
  not — that `x-request-timeout` is parsed and discarded (`api/mod.rs:48-50`), that the router carries only
  `TraceLayer`, that `tower`/`tower-http`'s timeout, concurrency-limit and body-limit middlewares are **not
  compiled into the binary** at all (`crates/sentinel-engine/Cargo.toml:7-21`), and that the alloy provider is
  likewise untimed (`crates/core/src/provider/mod.rs:129-137`). The untimed CoW client is one instance of that
  engine-wide absence and should be counted there.
- **Canonical for the unbounded, unvalidated `orderUid`: F-ENG-043 (this file).** That is a separate defect
  F-ENG-005 does not make: even with a perfect server-side deadline and a timed client, a ~2 MiB `orderUid`
  still becomes a ~4 MB request line sent to a third party for a lookup that cannot succeed. The fix is also
  different and local — reject any `orderUid` whose length is not 56 at `cow.rs:561-569`, which is a one-line
  guard and worth doing independently of any timeout work.

The report must not count the CoW timeout twice. Neither file should be merged or deleted; the timeout half of
this file's Claim should be presented as a cross-reference to F-ENG-005.

### 3. Severity — Low confirmed, and I can corroborate C-ENG-A's bound

**Low**, unchanged. R9 reaches it via A3; I reached it independently via the caller, and the evidence is
stronger than either file states on its own. The sentinel does not merely send the advisory header — it sets a
real client-side `reqwest` timeout on every check (`crates/sentinel/src/engine.rs:154-161`,
`self.request.timeout(timeout)`), and that path is actually taken in production, not just in tests:
`crates/sentinel/src/effect.rs:66` calls `.timeout(self.engine_timeout)` when building the check. So a stalled
`api.cow.fi` costs the attacker's own transaction its vote (`CheckOutcome::Unknown`, request dropped unanswered,
`crates/sentinel/src/service.rs:176-179`) and does not degrade service for other requests. No engine-side
amplification is demonstrated, which is what §8's High and Medium bands both require.

The design weakness is still real and should survive into the report in C-ENG-A's words: the bound lives
entirely in the caller, in another crate. Nothing in the engine enforces it.

One addition neither file makes, relevant to the amplification half: the outbound base URL is selected from
`transaction.chain_id` (`cow.rs:112`, `:422-427`), not from the engine's configured provider chain. So a
proposal can steer a Gnosis-configured engine into issuing requests to the mainnet CoW endpoint. This is not a
correctness problem — the order UID is recomputed against the same `chain_id`, so the binding stays
self-consistent — but it does mean the set of third parties the engine will contact is proposer-selected rather
than operator-selected, which widens the surface this finding describes. R9 records the underlying observation
in their coverage log (§6, "`CowChecker` gates on `transaction.chain_id` alone"); it belongs in this file's
remediation discussion.

### 4. Finding verdict

**Confirmed.** Certainty **74%**. The unbounded-`orderUid` half is pure code and fully `E2`; the no-timeout half
rests on a `reqwest` default that cannot be verified with no dependency source on disk and no toolchain, and
that `I` step is what holds this below the low 80s rather than any doubt about the mechanism. Severity **Low**
(unchanged).

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: both
halves need a peer — a never-responding listener for the timeout half, and an observable outbound request
for the URL half — and the pivotal dependency question (does an un-timed `reqwest` request hang
indefinitely?) is filed as **Q4** in `rust-audit/poc/UNRESOLVED-DEPENDENCY-QUESTIONS.md`. The *validation*
half, however, has a trivially writable unit test today; see the test-hook note below.

**Certainty unchanged at 74%. Severity unchanged at Low.** Confirmed by inspection that
`CowChecker::new` (`cow.rs:229-231`) builds a bare `reqwest::Client::new` with no timeout and no
connect timeout, and that `presignature_order_uid` (`cow.rs:561-573`) returns the decoded `orderUid`
without any length constraint.

### Remediation check

- **Option 2 (reject a malformed UID before the lookup) — sound, free, and it should ship first**, even
  though the finding lists it second. The argument in the finding is airtight and worth restating because
  it makes the fix uncontroversial: a UID of any length other than 56 **can never** match
  `compute_order_uid`'s 56-byte output (`cow.rs:294-302` compares them), so the request is *always*
  wasted — there is no legitimate traffic behind it. Adding `call.orderUid.len == 56` to
  `presignature_order_uid` removes the amplification entirely and costs nothing. It also shrinks the
  attack surface for anything reachable through the URL, independently of the timeout question.
- **Option 1 (build the client with `timeout`/`connect_timeout`) — sound, and it is F-ENG-005 option 1's
  CoW half.** Do not implement it twice: `cow.rs:234` (`CowChecker::with_client`) already exists as the
  seam, so the single change is at the `main.rs` construction site and it fixes both findings. Deriving the
  budget from `x-request-timeout` is the better end state but depends on F-ENG-005 option 2 threading a
  deadline through `CheckContext`; a config value is the right interim.
- **Option 3 (a `tower::timeout::TimeoutLayer` around the whole chain) — sound as a backstop only**, with
  the same caveat recorded under F-ENG-005 option 3: a layer-level timeout can only return a status code,
  converting a would-be denial into a missing vote, so set it well above the in-chain budget.
- **Order: option 2, then option 1, then option 3.** Option 2 is free and independent; option 1 is shared
  with F-ENG-005; option 3 is a last resort.
- **Test hook: half of it needs nothing.** `presignature_order_uid` is a pure function and `cow.rs` has the
  crate's largest test suite — the 55-byte and 57-byte UID cases the finding names are a few lines each and
  can be written today. The `fetch_order` timeout test needs a never-responding listener, which is
  infrastructure the crate lacks; `CowChecker::with_order_api` (`cow.rs:242`) can fake a slow `OrderApi`
  without a socket and is the cheaper approximation.
- **Where the fix belongs: the checker (UID validation) and the HTTP client construction (timeout).** Not
  the combinator or the `RuleId` mapping.
