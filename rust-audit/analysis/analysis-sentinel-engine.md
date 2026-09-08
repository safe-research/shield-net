> Generated on 2026-09-07 at commit 82b3e0d by a read-only analysis agent (Claude Fable 5.1) with no toolchain available. Every statement is class I or E2 by inspection; nothing here was executed. The Manager spot-checked the citations of the top hypotheses only. Treat every hypothesis as a lead to confirm or refute, never as a finding.

# `sentinel-engine` — technical map and risk analysis

Scope: `/home/shebin.guest/safe/safenet/crates/sentinel-engine` (every `.rs` file read in full, 5,113 lines), plus the required context (`AGENTS.md`, `docs/sentinel-engine.md`, `openapi.yaml`, `sentinel-engine.sample.toml`, `crates/sentinel/src/engine.rs`) and, for cross-checks, `crates/sentinel/src/bindings.rs` (lines 60–135), `crates/sentinel/src/effect.rs` (25–100), `crates/sentinel/src/main.rs` (40–80), `crates/core/src/provider/mod.rs`, `crates/core/src/observability/{mod,logging,metrics}.rs`, `crates/core/src/utils.rs` (1–36), `contracts/src/guard/SafenetGuard.sol` (18–45, 120–270, 355–368), `contracts/src/libraries/SafeTransaction.sol` (30–43), `contracts/src/libraries/TransactionAnnouncement.sol` (27–37), `scripts/run_sentinel_engine_integration_test.sh`, the root `Justfile`/`Cargo.toml`/`Cargo.lock` (version lines only).

Not read: the Safenet Arbitration Charter (not in the repo), the `sentinel-test-vectors` corpus (external), Safe's `Safe.sol`/`MultiSend.sol`, CoW's contracts, and any crate source under `~/.cargo/registry` (no registry on this host, cargo not installed). Statements about axum/alloy/ruint/serde_json/hyper/Safe/CoW behaviour below are therefore **inferences from protocol/library knowledge** and are marked `[inference]`.

All file paths below are relative to `crates/sentinel-engine/src/` unless prefixed otherwise.

---

## 1. Purpose and runtime shape

**Binary wiring (`main.rs`).** `main.rs:35-41` parses `--config-file` (default `sentinel-engine.toml`) and `--version`, loads `Config` (`config.rs:61-67`), then `observability::init` (`main.rs:45`; installs global tracing subscriber and a Prometheus listener, `crates/core/src/observability/mod.rs:56-63`). It connects exactly one `Provider` (`main.rs:51`; `crates/core/src/provider/mod.rs:129-137` — reads `eth_chainId` once and caches it, `provider/mod.rs:135,152-155`). It constructs one `AddressPoisoningChecker` in an `Arc` (`main.rs:52-56`) and the checker chain in this fixed order (`main.rs:57-73`):

| # | Checker | Type | Can affirm `secure`? | Can deny? | External I/O |
| --- | --- | --- | --- | --- | --- |
| 1 | `CancellationChecker` | pure | yes | no | none |
| 2 | `EscapeHatchChecker` | pure | yes | no | none |
| 3 | `BaseChecker` | pure | no | yes (R-4.1/R-4.2) | none |
| 4 | `BlocklistChecker` | config | no | yes (R-4.6) | none |
| 5 | `NestedSafeChecker` | pure | yes | no | none |
| 6 | `ExcessiveApprovalChecker` | pure | no | yes (R-4.5) | none |
| 7 | `CowChecker` | HTTPS to api.cow.fi | yes | yes (R-4.4/R-4.5) | `GET {base}/api/v1/orders/{uid}` |
| 8 | `StakingChecker` | pure | yes | yes (R-4.3/R-4.5) | none |
| 9 | `RefundChecker` | wraps #10 | no (squashed) | yes in theory (R-4.3) — **dead, see H1** | eth_getLogs (never reached) |
| 10 | `AddressPoisoningChecker` | RPC | yes | yes (R-4.3/R-4.4) | eth_getLogs |

The `main.rs:66-70` comment justifies placing `RefundChecker` next to `AddressPoisoningChecker` because it "can only deny or abstain (never affirm)"; it does not consider that seven _affirming_ checkers run before it (see §5, H4).

**Serving.** `TcpListener::bind(bind_address)` (`main.rs:75`), then `tokio::select!` between `axum::serve(listener, api::router(engine))` and `utils::shutdown_signal()` (`main.rs:79-84`). On SIGTERM/SIGINT (`crates/core/src/utils.rs:17-36`) the `serve` future is simply dropped — there is **no** `with_graceful_shutdown`, so in-flight requests are aborted (the sentinel then sees a transport error → `CheckOutcome::Unknown`, `crates/sentinel/src/engine.rs:181-187`). `shutdown_signal` unwraps signal registration (`utils.rs:20,27`).

**Router and handler (`api/mod.rs`).** One route: `POST /v1/security-check` (`api/mod.rs:27-30`), state `Arc<SentinelEngine>`, one layer `TraceLayer::new_for_http()` (`api/mod.rs:30`). No body-size override, no timeout layer, no concurrency limit, no auth (documented as intentional, `docs/sentinel-engine.md:35`). The handler (`api/mod.rs:33-60`) extracts `RequestId`, `RequestTimeout`, then `Json<CheckRequest>`; opens an `info_span!` `security_check` with `safe` and optional `request_id` (`api/mod.rs:39-46`); **discards the timeout** (`api/mod.rs:48-50`, `let _ = timeout;` with a TODO); builds `CheckContext { block }` (`api/mod.rs:52-54`) and awaits `engine.security_check` (`api/mod.rs:55-58`); returns `Json(verdict)`.

`CheckRequest` (`api/mod.rs:14-22`) is `#[serde(deny_unknown_fields)]`, `block: u64` via `alloy::serde::quantity`, `transaction: SafeTransaction`.

**Header extractors (`api/extractors.rs`).** Both are optional-header extractors sharing `parse_header` (`extractors.rs:52-69`): absent header → `Ok(None)`; present → `to_str()` (fails on non-ASCII/opaque bytes) then `T::from_str`. `x-request-id` → `B256::from_str` (`extractors.rs:21-26`; alloy's `FromStr` accepts an optional `0x` prefix and either case `[inference]`); `x-request-timeout` → `u64::from_str` then `Duration::from_millis` (`extractors.rs:42-48`; `from_millis(u64)` cannot panic). Garbage in either header → rejection `(StatusCode::BAD_REQUEST, &'static str)` (`extractors.rs:18,23-25,44-46`), a plain-text 400, before the body is read. No upper bound on the timeout value (u64::MAX accepted, then ignored anyway). The sentinel always sends a canonical `0x`+64-lowercase-hex id (`crates/sentinel/src/engine.rs:150`, `B256::to_string()`) and a decimal millisecond count (`engine.rs:156-159`), so the honest path never trips these.

**Chain semantics (`engine/mod.rs`).** `SentinelEngine(Vec<Box<dyn Checker>>)` (`engine/mod.rs:18`). `security_check` (`engine/mod.rs:57-72`) runs checkers sequentially; **the first non-`Abstain` verdict wins and the loop breaks** (`engine/mod.rs:62-69`); if every checker abstains the result is `Abstain` (`engine/mod.rs:62`). Consequently: `secure` from checker _k_ suppresses every denial a later checker would have produced (this is the root of H2/H4/H5). There is no aggregation, no "all must agree", no evidence object. Unit tests pin this (`engine/mod.rs:104-120`). `Verdict` (`engine/mod.rs:36-48`) serialises as `{"verdict":"secure"|"abstain"}` or `{"verdict":"insecure","rule":"R-x.y"}` via `RuleId::code()` (`engine/rule.rs:83-90,103-114`).

**Timeout enforcement.** None server-side. The caller's budget is parsed then dropped (`api/mod.rs:48-50`). The sentinel enforces its own reqwest timeout (`crates/sentinel/src/engine.rs:155-161`), computed as ¾·(voting_window−1)· block_time with a 1 s floor (`crates/sentinel/src/main.rs:50-61`). If the RPC or the CoW API hangs, the engine handler hangs until the client disconnects; hyper drops the handler future on connection close `[inference]`.

**Per-request logging.** At the default `info` filter (`observability/mod.rs:32`): the `security_check` span fields (`safe`, `request_id`) and `TraceLayer`'s `request` span; checker-level `trace!` lines (`engine/mod.rs:65,70`) and `TraceLayer`'s DEBUG response line are filtered out `[inference for tower-http defaults]`. Request-triggerable `warn!`/ `error!` lines: `address_poisoning.rs:203-208, 313-317, 351-357, 370-376`; `cow.rs:297-301, 319-323`. Bodies are never logged; addresses and `order_uid` are.

---

## 2. Module map

| File | LOC | Responsibility | Key pub items | Main deps |
| --- | --: | --- | --- | --- |
| `main.rs` | 87 | CLI, config load, observability init, provider connect, checker chain, serve + shutdown | `Options`, `main` | argh, tokio, axum, safenet-core |
| `api/mod.rs` | 60 | Router + handler; `CheckRequest` | `CheckRequest`, `router`, `security_check` | axum, tower-http, serde |
| `api/extractors.rs` | 69 | Optional header extractors | `RequestId(Option<B256>)`, `RequestTimeout(Option<Duration>)` | axum, alloy |
| `engine/mod.rs` | 121 | Checker chain, `CheckContext`, `Verdict` | `SentinelEngine`, `CheckContext{block}`, `Verdict` | serde |
| `engine/rule.rs` | 164 | Charter rule enum ↔ `R-x.y` codes | `RuleId` (6 variants), `code`, `from_code` | serde |
| `engine/transaction.rs` | 170 | Wire types: `Operation`, `SafeTransaction` (deny_unknown_fields, camelCase) | `Operation`, `SafeTransaction`, `InvalidOperation` | alloy primitives, serde |
| `config.rs` | 154 | TOML schema | `Config{rpc,bind_address,observability,engine}`, `EngineConfig{blocklist,address_poisoning_lookback_blocks,address_poisoning_max_block_range}` | toml, serde, url |
| `checkers/mod.rs` | 48 | `Checker` trait; `Arc<T>` blanket impl | `Checker` | async-trait |
| `checkers/cancellation.rs` | 72 | Empty self-call ⇒ secure | `CancellationChecker` | — |
| `checkers/escape_hatch.rs` | 61 | `announceTransaction`/`cancelAnnouncement` selector ⇒ secure (any `to`) | `EscapeHatchChecker` | bindings::safenet_guard |
| `checkers/base.rs` | 765 | Article IV-A: self-call allow-list, delegatecall allow-list, MultiSend recursion (1 level) | `BaseChecker`, `check_transaction` | bindings::safe, multi_send |
| `checkers/blocklist.rs` | 89 | `to ∈ blocklist` ⇒ R-4.6 | `BlocklistChecker` | — |
| `checkers/nested.rs` | 47 | `Call` to other addr with `execTransaction` calldata ⇒ secure | `NestedSafeChecker` | bindings::safe |
| `checkers/excessive_approval.rs` | 136 | `approve(_, MAX)` / `setApprovalForAll(_, true)` ⇒ R-4.5 | `ExcessiveApprovalChecker` | target_effects |
| `checkers/cow.rs` | 1407 | CoW dangling-approve denial; TWAP batch; presignature batch (HTTP order lookup + EIP-712 UID recompute) | `CowChecker`, `OrderApi` (private trait) | bindings::cow, reqwest, alloy sol_types |
| `checkers/staking.rs` | 183 | SAFE staking `claim`/`approve`+`stake` (chain 1) | `StakingChecker` | bindings::staking, multi_send |
| `checkers/refund.rs` | 206 | Resynthesise gas refund as ERC-20 `transfer`, delegate to address-poisoning, squash `Secure` | `RefundChecker` | address_poisoning |
| `checkers/address_poisoning.rs` | 457 | R-4.3/4.4 lookalike detection via `eth_getLogs` history | `AddressPoisoningChecker` | safenet-core Provider, alloy rpc types |
| `contracts/mod.rs` | 5 | re-exports | — | — |
| `contracts/bindings.rs` | 172 | `sol!` ABI bindings (Safe, guard, ERC-20/721/1155, MultiSend, staking, CoW) | modules `safe`, `safenet_guard`, `erc20`, `erc721`, `erc1155`, `multi_send`, `staking`, `cow` | alloy sol! |
| `contracts/multi_send.rs` | 186 | Known MultiSend deployments; packed-blob decoder; `sub_transactions` | `MultiSendVersion`, `known_deployment`, `decode_multi_send`, `decode_multi_send_call`, `sub_transactions` | alloy |
| `contracts/target_effects.rs` | 454 | Value/approval effect decoding, recursing through MultiSend | `TargetEffect`, `EffectKind`, `decode_target_effects` | bindings, multi_send |

---

## 3. External interfaces

### 3.1 HTTP (inbound, from the sentinel)

- **Endpoint:** `POST /v1/security-check` only (`api/mod.rs:28`). Any other path → 404, other methods → 405 (axum defaults `[inference]`). The sentinel builds the URL by appending `v1/security-check` to the configured base (`crates/sentinel/src/engine.rs:104-115`).
- **Body size:** no explicit limit; axum 0.8's `DefaultBodyLimit` (2 MiB) applies to the `Json` extractor `[inference — registry not available to verify]`. Oversized → 413.
- **Content-Type:** axum `Json` requires `application/json` (or `+json`) else 415 `[inference]`. Sentinel sends `.json(&Request)` (`engine.rs:131-134`) ✔.
- **JSON strictness:** `CheckRequest` and `SafeTransaction` both `deny_unknown_fields` (`api/mod.rs:15`, `engine/transaction.rs:56`); tests pin unknown-member and `operation: 42` rejection (`transaction.rs:159-169`). `operation` is `u8` restricted to 0/1 (`transaction.rs:20-30,41-50`). Big numbers: all quantities are hex strings parsed into `U256` (alloy/ruint) or `u64` (`block`, `alloy::serde::quantity`); values above the type width fail deserialisation → 422 `[inference]`. Hex leniency: the engine is more permissive than the OpenAPI patterns (`openapi.yaml:181-190`): mixed-case addresses accepted by design (`transaction.rs:78-79,143-157`); non-minimal `0x01` and upper-case hex for quantities/bytes are probably accepted `[inference]`. No security impact — the sentinel is the only expected caller and is trusted.
- **Error response shapes vs `openapi.yaml`:** the spec documents only `200` (`openapi.yaml:39-48`). Actual non-200s are axum defaults with `text/plain` bodies: 400 (bad header, `extractors.rs:23-25,44-46`; JSON syntax error), 415 (content type), 422 (JSON data error: unknown field, bad hex, bad operation), 413 (too large) `[inference for the axum mapping]`. The sentinel treats every non-2xx and every parse failure as `Unknown` → no vote (`crates/sentinel/src/engine.rs:164-191`), so the mismatch is harmless but undocumented.
- **Wire agreement (both sides):** sentinel `Request{block: quantity u64, transaction}` (`engine.rs:71-76`) ↔ engine `CheckRequest` (`api/mod.rs:14-22`) ✔. Sentinel `SafeTransaction` is a `sol!` struct with 12 fields named exactly as `SafeTransaction.T` (`crates/sentinel/src/bindings.rs:77-92`; on-chain `contracts/src/libraries/SafeTransaction.sol:30-43`) and a custom `Operation` serialiser emitting `0`/`1` as `u8` (`bindings.rs:119-131`) ↔ engine's `rename_all = "camelCase"` snake-case fields (`transaction.rs:56-76`) and `Operation` u8 (`transaction.rs:41-50`) ✔. `U256`/`Bytes`/`Address` serialise as `0x` hex strings on both sides `[inference for alloy serde]`. Response: sentinel `Response{Secure, Insecure{rule}, Abstain}` tagged by `verdict` (`engine.rs:78-84`) ↔ engine `Verdict` (`engine/mod.rs:36-48`) ✔; sentinel `RuleId::parse` accepts any `R-<u32>.<u32>` (`engine.rs:37-42`), engine emits only the six codes in `rule.rs:105-114` ✔.

### 3.2 RPC (outbound, one provider)

- **Startup:** `eth_chainId` once (`crates/core/src/provider/mod.rs:135`); cached forever (`provider/mod.rs:153`). A provider that later fails over to another chain is not re-detected (Info).
- **Per request:** only `AddressPoisoningChecker::established_recipients` (`address_poisoning.rs:182-228`) issues `eth_getLogs` with filter `address = transaction.to (the token)`, `topics[0] ∈ {Transfer, Approval}`, `topics[1] = safe`, `fromBlock/toBlock` = each chunk of `[block − lookback, block]` (`address_poisoning.rs:189-199`; chunking `address_poisoning.rs:250-268`). Calls are sequential; the loop returns early on an exact match (`address_poisoning.rs:218-220`). Max calls per lookup = number of chunks = ⌈(lookback+1)/(max_range+1)⌉, or 1 when `max_block_range` is unset. No `eth_call` is made anywhere. No retries, no RPC timeout configured (alloy/reqwest default: none `[inference]`).
- **Failure mapping:** first chunk fails → `Err` → `Abstain` + `warn!` (`address_poisoning.rs:212,369-378`); a later chunk fails → partial scan with `complete=false` (`address_poisoning.rs:202-211`) → exact match still `Secure` if found earlier, lookalike → `Abstain` (`address_poisoning.rs:348-359`), nothing → `Abstain`. Chain-id mismatch → `Abstain` + `warn!` (`address_poisoning.rs:312-319`). **On the wire an RPC-failure abstain is indistinguishable from a deliberate abstain** (`Verdict::Abstain` carries no reason); only logs differ.

### 3.3 Third-party HTTPS (outbound, CoW API)

`ReqwestOrderApi::fetch_order` (`cow.rs:191-204`) issues `GET {base}/api/v1/orders/{order_uid}` with `base` chosen by `transaction.chain_id` (`cow.rs:112-122`; mainnet/xdai/arbitrum_one). `order_uid` is the attacker-supplied `bytes` from `setPreSignature` rendered as `0x`-hex (`cow.rs:198`; `Bytes: Display` → hex only, so no path injection, but unbounded length). Client is `reqwest::Client::new()` with **no timeout** (`cow.rs:229-231`). Responses are never trusted at face value: `compute_order_uid` (`cow.rs:143-169`) recomputes the EIP-712 order digest ‖ owner ‖ validTo and compares with the requested UID (`cow.rs:296-303`). This dependency is not listed in `docs/sentinel-engine.md` (line 88 says only address poisoning is externally backed; the Dockerfile comment at `Dockerfile:14-15` hints at "TLS endpoints").

### 3.4 Configuration (`config.rs`)

`rpc: Url` (required), `bind_address: SocketAddr` default `127.0.0.1:5473` (`config.rs:25-26,57-59`), `observability` default (`log_filter = "info"`, `metrics_address = 127.0.0.1:0`, `crates/core/src/observability/mod.rs:29-36`), `engine.blocklist: Vec<Address>` (required, may be empty), `engine.address_poisoning_lookback_blocks: u64` (required, no upper bound), `engine.address_poisoning_max_block_range: Option<NonZeroU64>` (`config.rs:35-55`). `deny_unknown_fields` on both tables (`config.rs:20,36`). Sample config parses in a test (`config.rs:146-153`).

### 3.5 Metrics / logging

The engine itself records **no metrics** (grep: only config tests mention `metrics_address`). Through safenet-core it exports `rpc_requests_total{method,result}` from the transport layer (`provider/mod.rs:112-114`) and a `/health` endpoint on the metrics listener (`observability/metrics.rs:6-16`), which by default binds an **ephemeral loopback port** — orchestrator health probes cannot reach it unless `metrics_address` is set. Logging is JSON when stdout is not a TTY (`observability/logging.rs:14-21`). Flood potential: each attacker-proposed transaction can trigger up to one or two `warn!`/`error!` lines (see §1); volume is bounded by proposal throughput on the Consensus contract.

---

## 4. Trust boundaries and untrusted inputs

Threat model: the sentinel (co-deployed, trusted) relays a `SafeTransaction` that was **proposed on-chain by an arbitrary party**; every field of `transaction` (including `chain_id`, `safe`, `to`, `data`, `value`, gas/refund fields) is attacker-controlled. `block` is sentinel-controlled. Headers are sentinel-controlled.

### 4.1 `engine/transaction.rs`

Derived `Deserialize` with `deny_unknown_fields`; `Operation` validated to {0,1} (`transaction.rs:20-29`). No custom parsing code; no panics. Malformed → 422 (handler never runs).

### 4.2 `contracts/multi_send.rs` (packed MultiSend decoder)

`decode_multi_send` (`multi_send.rs:92-133`) walks a `Cursor` (`multi_send.rs:174-186`) using `split_at_checked` — every read is bounds-checked and returns `None` on truncation. Operation byte ∉ {0,1} → `None` (`multi_send.rs:100-104`). `dataLength` is a `U256` converted with `try_into().ok()?` (`multi_send.rs:108`) — values

> `usize::MAX` → `None`; values ≤ `usize::MAX` but > remaining → `read` returns `None`. **No recursion** here; nested MultiSend sub-calls are returned as opaque `DelegateCall` sub-transactions. Entry count is bounded by input length (≥ 85 bytes per entry). Memory: one `Bytes` copy per entry (`multi_send.rs:109`), O(input). Version quirk: for the two `V150Plus` deployments a sub-call with `to == 0` is rewritten to `safe` (`multi_send.rs:111-114`), modelling the 1.5.0 self-call convention `[inference — MultiSend.sol not read]`; for `Legacy` deployments `to == 0` is left as a call to the zero address, which `base.rs:91-93` then treats as a call to "another contract" (allowed). If any legacy deployment in `DEPLOYMENTS` (`multi_send.rs:27-68`) actually redirects zero to `address(this)`, that would be an R-4.1 bypass (see checklist). `decode_multi_send_call` (`multi_send.rs:142-165`) requires `DelegateCall` + known deployment + `multiSend(bytes)` ABI decode. Result: **total, no panics.**

### 4.3 `contracts/target_effects.rs`

`decode_target_effects` (`target_effects.rs:46-52`) **recurses without a depth limit** through nested MultiSend (`target_effects.rs:47-48`). Reachability in production: only `ExcessiveApprovalChecker` (#6) calls it, and `BaseChecker` (#3) denies any batch containing a delegatecall sub-call to a MultiSend address (a MultiSend target is not in `check_delegate_calls`' lists, `base.rs:151-193`, so `check_multi_send`'s `all(...)` at `base.rs:210-212` fails → `Insecure R-4.2` → chain stops). So the recursion is unreachable **only because of checker ordering** (latent; H11). `decode_call` (`target_effects.rs:58-132`) is an `else-if` selector chain using alloy `abi_decode`; unrecognised or short calldata → no effect; native value is recorded independently (`target_effects.rs:60-65`). Total; no panics.

### 4.4 `contracts/bindings.rs`

Pure `sol!` declarations. alloy's `abi_decode` (non-validating in alloy-sol-types 1.x `[inference]`) checks the 4-byte selector and decodes parameters; trailing bytes are ignored, dirty padding is not rejected. Callers that only use `starts_with(SELECTOR)` (`escape_hatch.rs:56-61`, `nested.rs:45`, `base.rs:105-122,161-190`) accept **any** suffix, including truncated/garbage arguments. Where arguments matter, `abi_decode` is used (`base.rs:125-144`). The `safenet_guard` selector is computed from a struct mirror (`bindings.rs:42-54`) whose field types match the on-chain `AnnouncedTransaction` (`contracts/src/libraries/TransactionAnnouncement.sol:27-37`; the Solidity `Enum.Operation` ABI-encodes as `uint8` `[inference]`) ✔.

### 4.5 Per-checker adversarial behaviour

| Checker | Malformed/adversarial input → |
| --- | --- |
| Cancellation (`cancellation.rs:15-28`) | structural equality; anything else → Abstain. |
| EscapeHatch (`escape_hatch.rs:52-61`) | selector prefix on **any** `to`, value 0, gasPrice 0 → **Secure** (H5). |
| Base (`base.rs:52-213`) | self-call to unknown selector → R-4.1; delegatecall to unknown target → R-4.2; MultiSend blob malformed → `decode_multi_send_call` None → R-4.2; nested MultiSend → R-4.2 (misattributed, TODO at `base.rs:198-204`). Never Secure. |
| Blocklist (`blocklist.rs:24-32`) | top-level `to` only; sub-calls never inspected (H6). |
| NestedSafe (`nested.rs:42-47`) | `Call`, `to != safe`, `execTransaction` decodes → **Secure**, regardless of `value`, `gasPrice`, `to` being a Safe (H2, H4). |
| ExcessiveApproval (`excessive_approval.rs:19-33`) | only literal `U256::MAX` / `approved=true`; `MAX−1` passes. |
| Cow (`cow.rs:421-449`) | unsupported chain → Abstain; malformed `staticInput` → Abstain (`cow.rs:539`); API 4xx/5xx/hang → Abstain (`cow.rs:318-325`); UID mismatch → Abstain. Attacker-chosen `n`/`partSellAmount` shape the tolerance (H8). |
| Staking (`staking.rs:88-116`) | chain ≠ 1 → Abstain; `claim(account≠safe)` → R-4.3; `[approve>stake]` → R-4.5; `[approve≤stake]`, `[stake]`, `[claims only]` → **Secure** (ignores `gasPrice`/refund, H4). |
| Refund (`refund.rs:52-57,97-118`) | always Abstain in practice (H1). |
| AddressPoisoning (`address_poisoning.rs:308-380`) | `value` ignored; `to` (token) unverified; exact match → **Secure** (H3); lookalike → Insecure; novel → Abstain; RPC error → Abstain. |

### 4.6 `block` trust

`from = block.saturating_sub(lookback)`, `to = block` (`address_poisoning.rs:189,192`). The span is always ≤ `lookback_blocks` regardless of `block`, so a bad `block` cannot widen the range; it can only shift the window (a block beyond the head → provider-dependent empty/error → Abstain; a block after the transaction's own inclusion → the transaction's own logs count as evidence, documented at `docs/sentinel-engine.md:31` and `address_poisoning.rs:41-46`). `block = 0` → `(0,0)`. No validation against the provider's head. The sentinel's value comes from its indexer (`crates/sentinel/src/effect.rs:56-66`; origin of `block` upstream of `Effect::EngineCheck` not read).

### 4.7 `chain_id` handling

No central check (`engine/mod.rs` has none; AGENTS.md:112 acknowledges this). Per checker: AddressPoisoning compares `transaction.chain_id` with the cached provider chain id **after** decoding the target (`address_poisoning.rs:309-319`) ✔. Refund inherits that check but builds its synthetic transaction with `..Default::default()`, i.e. `chain_id = 0` (`refund.rs:105-117`) → always mismatches → **always Abstain** (H1). Cow gates on `{1, 100, 42161}` (`cow.rs:106, 422-427`) and uses `chain_id` to pick the API host and EIP-712 domain (`cow.rs:112-137`) — no provider comparison, but it uses no RPC state. Staking gates on chain 1 (`staking.rs:77,89-91`). Base/MultiSend constants are chain-agnostic canonical deployments `[inference]`. Blocklist is not chain-scoped.

---

## 5. Verdict semantics and safety

| Checker | Rules cited | `secure` when | `insecure` when | `abstain` when | Basis for `secure` |
| --- | --- | --- | --- | --- | --- |
| Cancellation | — | tx == `{safe→safe, nonce, chain_id, all else zero/empty, Call}` (`cancellation.rs:16-27`) | never | otherwise | structural; sound (empty self-call). |
| EscapeHatch | — | `Call`, `value=0`, `gasPrice=0`, data starts with `announceTransaction`/`cancelAnnouncement` selector, **any `to`** (`escape_hatch.rs:53-61`) | never | otherwise | shape only; broader than the on-chain guard's `_isAutoAllowed`, which additionally requires `to == address(this)` (`SafenetGuard.sol:360`). |
| Base | R-4.1, R-4.2 | never | self-call outside allow-list (`base.rs:103-147`), delegatecall outside allow-lists (`base.rs:151-193`), MultiSend with any failing sub-call (`base.rs:205-213`) | all base guarantees hold (`base.rs:43`) | n/a |
| Blocklist | R-4.6 | never | top-level `to` ∈ list | otherwise | n/a |
| NestedSafe | — | `Call`, `to != safe`, `execTransaction` decodes (`nested.rs:42-47`) | never | otherwise | shape only; **ignores `value`, gas/refund fields, target identity, and the nested payload**. |
| ExcessiveApproval | R-4.5 | never | any effect `Erc20Approval{MAX}` or `OperatorApproval{true}` (`excessive_approval.rs:20-31`) | otherwise | n/a |
| Cow | R-4.4, R-4.5 | exact 2-call TWAP batch with receiver ∈ {safe, 0}, token match, amount ≤ total+n−1 (`cow.rs:351-383`); exact 2-call presig batch whose fetched order digest matches, receiver==safe, token/amount equal (`cow.rs:295-317`) | approve→relayer w/o trigger (`cow.rs:388-402`); TWAP/presig receiver≠safe (R-4.4); token/amount mismatch (R-4.5) | unsupported chain; other shapes; API error/UID mismatch; malformed staticInput | calldata-derived order terms (TWAP) or API-fetched-and-digest-verified order (presig). Ignores `gasPrice`/refund. |
| Staking | R-4.3, R-4.5 | claims-only (all `account==safe`), lone `stake`, `[approve≤stake]` on canonical contracts (`staking.rs:109-116,124-150`) | `claim(account≠safe)` (R-4.3, `staking.rs:99-103`); `[approve>stake]` (R-4.5, `staking.rs:141-144`) | chain≠1; other shapes | canonical addresses; ignores `gasPrice`/refund. |
| Refund | (R-4.3 via AP) | never (squashed, `refund.rs:68-73`) | in theory lookalike refund receiver | gasPrice/gasToken/refundReceiver zero (`refund.rs:98-103`), or **always** (chain_id 0, H1) | n/a |
| AddressPoisoning | R-4.3, R-4.4 | non-zero prior `Transfer`/`Approval` from `safe` to the exact candidate on `to` within the window (`address_poisoning.rs:218-219,325-333`) | complete scan, no exact match, candidate is a 4+4-nibble lookalike of another established address (`address_poisoning.rs:338-367`) | no ERC-20 target; `value`-less semantics ignored; chain mismatch; RPC error; novel candidate; incomplete scan with lookalike | onchain history keyed by **attacker-chosen `to`**; ignores `value` (H3). |

**Ordering dependencies (short-circuit hazards).** Because the chain stops at the first non-abstain (`engine/mod.rs:66-68`):

- EscapeHatch (#2) affirms before Blocklist (#4) → blocklist bypass for the escape-hatch shape (H5). `nested.rs:10-11` explicitly orders NestedSafe _after_ Blocklist for exactly this reason; EscapeHatch did not get the same treatment.
- NestedSafe (#5) affirms before ExcessiveApproval, Cow, Staking, Refund, AddressPoisoning.
- Cow/Staking (#7/#8) affirm before Refund/AddressPoisoning.
- **Refund (#9), the only deny-only checker for the refund leg, runs after every affirming checker**, so it could never deny a transaction that Nested/Cow/Staking affirmed even if it worked (H4). Only EscapeHatch (`gasPrice==0`) and Cancellation (all-zero) are immune to the refund hole.

**Documented TODO holes.** `refund.rs:83-96`: native-currency refunds and `refundReceiver == 0` (→ `tx.origin`) are uninspected; "a transaction another checker calls `Secure` can now drain unbounded native currency to `refundReceiver`". `address_poisoning.rs:26-39`: `transferFrom(safe, X, 1)` forgery can manufacture `Secure` or deny genuine payments. `address_poisoning.rs:303-307`: novel recipients only abstain. `base.rs:198-204`: MultiSend denials always cite R-4.2.

**RPC-failure abstain vs deliberate abstain:** not distinguishable on the wire (same `{"verdict":"abstain"}`); only `warn!` logs (`address_poisoning.rs:370-376`, `cow.rs:319-323`) differ. The sentinel's metric `engine_check_verdicts_total{Abstain}` cannot tell them apart either (`crates/sentinel/src/engine.rs:175,189`).

---

## 6. Invariants

| Invariant | Status | Where |
| --- | --- | --- |
| A checker never returns `secure`/`insecure` without evidence (AGENTS.md:110) | **assumed, not enforced** — EscapeHatch/NestedSafe affirm on calldata shape alone | `escape_hatch.rs:52-61`, `nested.rs:42-47` |
| The engine never returns `insecure` without a valid rule code | enforced by types: `Verdict::Insecure { rule: RuleId }`, serialised via `code()` | `engine/mod.rs:42-45`, `rule.rs:83-90,105-114` |
| Rule codes match the OpenAPI pattern `^R-[0-9]+\.[0-9]+$` | enforced by the constant table + round-trip test | `rule.rs:105-114,138-152`; `openapi.yaml:167` |
| `chain_id` verified before RPC-derived evidence is used | enforced only inside `AddressPoisoningChecker`; no central check; Refund's synthetic tx violates it (chain 0) | `address_poisoning.rs:312-319`; `refund.rs:105-117` |
| The engine's chain view is at or before `block` | enforced for eth_getLogs (`to_block ≤ block`); CoW API data is not block-anchored | `address_poisoning.rs:192,262`; `cow.rs:191-204` |
| Decoding is total (no panics on attacker input) | enforced by construction (checked cursor, `Option`/`Result` everywhere); no fuzz tests | `multi_send.rs:174-186`; §8 |
| Request schema is strict (no unknown fields, operation ∈ {0,1}) | enforced | `api/mod.rs:15`, `transaction.rs:20-29,56` |
| First non-abstain wins; deny-only checkers placed after affirmers cannot deny | property of the loop; ordering is a manual invariant with no test on the real chain | `engine/mod.rs:62-69`, `main.rs:57-73` |
| No keys, no bond, no on-chain writes | enforced by absence: only `get_logs`/`get_chain_id` through `Provider`; no signer constructed | `main.rs:51-56`; `provider/mod.rs:158-166` |
| `x-request-timeout` is honoured | **not enforced** (parsed, discarded) | `api/mod.rs:48-50` |
| Blocklist covers every destination the transaction touches | **not enforced** (top-level `to` only; ordering after EscapeHatch) | `blocklist.rs:25`, `main.rs:58-61` |
| Refund leg is vetted whenever the primary leg is affirmed | **not enforced** (Refund after affirmers; Refund dead) | `main.rs:71-72`, `refund.rs:105-117` |
| One RPC, one chain (docs) | enforced for the single `Provider`; the CoW API host is chosen per transaction chain, not per provider | `main.rs:51`, `cow.rs:112-122` |

---

## 7. Concurrency and resource limits

- **Per-request CPU/memory:** JSON body ≤ 2 MiB `[inference]`; MultiSend entries ≤ body/85 (~12k); one-level decoding in Base/Cow/Staking (`multi_send.rs:168-172`); unbounded recursion only in `decode_target_effects` (`target_effects.rs:47-48`), unreachable for nested batches while Base precedes ExcessiveApproval (§4.3). Log decoding allocates a `HashSet<Address>` per lookup (`address_poisoning.rs:190`) sized by the number of distinct recipients returned — bounded by the provider's log-count cap, not by the engine.
- **Per-request I/O:** ≤ ⌈(lookback+1)/(max_range+1)⌉ sequential `eth_getLogs` (AddressPoisoning); Refund would add the same again but currently returns before any RPC (chain mismatch, `address_poisoning.rs:312`); ≤ 1 CoW API GET (presignature batches only). No caching anywhere (`CowChecker::new()` holds only a `reqwest::Client`, `cow.rs:229-239`; `AddressPoisoningChecker` holds provider + two integers, `address_poisoning.rs:143-147`), so no memory growth across requests.
- **Concurrency limits:** none (no `tower::limit`, no `TimeoutLayer`, `api/mod.rs:25-31`). Tokio multi-thread runtime (`main.rs:33`). Each in-flight request holds one RPC and/or one HTTPS connection with no timeout.
- **Timeouts:** none server-side; the sentinel's client timeout (`crates/sentinel/src/engine.rs:155-161`) bounds wall time from the caller's perspective only.
- **Response size:** fixed tiny JSON.
- **DoS potential:** a single sentinel can only issue as many requests as it has proposals; a malicious proposer can craft transactions that force the most expensive path (presignature batch → outbound HTTPS with a huge `orderUid` URL; ERC-20 transfer → up to N `eth_getLogs` over 50k blocks). Cost per proposal is bounded by config (chunks) and by the third-party's own limits; exhausting the CoW API's rate limit or the RPC's quota degrades every later check to `Abstain` (no vote) — a liveness, not safety, failure. The unbounded-length `orderUid` also lets a proposer make the engine emit multi-megabyte URLs to `api.cow.fi` (`cow.rs:198`).

---

## 8. Error handling (non-test code) — every unwrap/expect/panic/index/slice/cast/arithmetic

| Location | Construct | Provably safe? |
| --- | --- | --- |
| `cow.rs:130` | `u64::try_from(chain_id).expect("chain id expected to be in u64 range")` | Safe **today**: only reachable from `check_presignature_batch`, which first passes `order_api_base_url` (`cow.rs:282-284`) admitting only 1/100/42161. Latent panic if `compute_order_uid` gains another caller. |
| `cow.rs:164-167` | `uid[..32]`, `uid[32..52]`, `uid[52..]` `copy_from_slice` | Safe: sources are 32 (`B256`), 20 (`Address`), 4 (`u32::to_be_bytes`) bytes. |
| `cow.rs:296` | `&compute_order_uid(..)[..]` | Safe (full slice). |
| `cow.rs:540` | `checked_mul` | Safe (returns `None`). |
| `cow.rs:553` | `saturating_add`/`saturating_sub` | Safe. |
| `multi_send.rs:106` | `Address::from_slice(cursor.read(20)?)` | Safe: `read(20)` yields exactly 20 bytes or `None`. |
| `multi_send.rs:107-108` | `U256::from_be_slice(cursor.read(32)?)` ×2 | Safe: exactly 32 bytes (panics only if > 32). |
| `multi_send.rs:108` | `U256 → usize` via `try_into().ok()?` | Safe. |
| `multi_send.rs:178` | `self.read(1)?[0]` | Safe: length-1 slice. |
| `multi_send.rs:182` | `split_at_checked(len)?` | Safe. |
| `address_poisoning.rs:63-70` | `out[2*i]`, `out[2*i+1]`, `i < 20` | Safe: max index 39 of 40; no overflow. |
| `address_poisoning.rs:189` | `saturating_sub` | Safe. |
| `address_poisoning.rs:255,262,265` | `NonZeroU64::MAX.get()`, `saturating_add(..).min(to)`, `saturating_add(1)` | Safe; loop terminates because `end ≥ start` and `done = end >= to`; `from ≤ to` always holds (`from = to.saturating_sub(..)`). |
| `address_poisoning.rs:276` | `topics().first()` | Safe. |
| `refund.rs:110-112` | `saturating_mul`/`saturating_add` | Safe. |
| `transaction.rs:37` | `*self as _` (fieldless `#[repr(u8)]` enum → u8) | Safe. |
| `extractors.rs:48` | `Duration::from_millis(u64)` | Safe (never panics). |
| `staking.rs:95` | `Vec::with_capacity(calls.len())` | Safe. |
| `main.rs:37` | `env!("CARGO_PKG_VERSION")` | Compile-time. |
| `crates/core/src/utils.rs:20,27` (called from `main.rs:81`) | `unix::signal(..).unwrap()` ×2 | Panics only if the runtime cannot register SIGTERM/SIGINT handlers; would abort `main`. Outside the crate. |

No `unreachable!`, `panic!`, `unwrap()` or `expect()` exists in the crate's non-test code besides `cow.rs:130`. `base.rs:55` `unwrap_or(Err(R4_1SettingsChange))` is a fallback for an impossible case (operation is always Call or DelegateCall). `serde_json` recursion depth is capped by the library (128) `[inference]`, so nested-JSON stack overflow is not reachable. **Conclusion: no attacker-reachable panic/500 found; the decoders are total.**

---

## 9. Test coverage

97 `#[test]`/`#[tokio::test]` functions: `cow.rs` 31, `base.rs` 20, `target_effects.rs` 14, `refund.rs` 7, `transaction.rs` 4, `config.rs` 4, `excessive_approval.rs` 4, `address_poisoning.rs` 4, `blocklist.rs` 3, `rule.rs` 2, `engine/mod.rs` 2, `cancellation.rs` 2. Zero tests in: `main.rs`, `api/mod.rs`, `api/extractors.rs`, `contracts/multi_send.rs`, `contracts/bindings.rs`, `checkers/staking.rs`, `checkers/nested.rs`, `checkers/escape_hatch.rs`, `checkers/mod.rs`.

What is covered:

- `cow.rs:840-1406`: dangling-approve denial; TWAP exact/over/under/rounding-headroom/wrong-token/receiver zero/ receiver other/3-call/unrelated-approve/unknown handler/unknown factory/delegatecall & value variants; presignature found/not-found/amount mismatch/receiver mismatch/receiver zero/UID mismatch/shape; `compute_order_uid` pinned to a real mainnet order (`cow.rs:1379-1406`).
- `base.rs:277-764`: allow-listed self-calls, singleton migration, CreateCall, MultiSend (incl. the Bybit calldata vector at `base.rs:508-522`, call-only MultiSend denial, empty MultiSend, nested delegatecall denial).
- `target_effects.rs:178-453`: each effect kind + MultiSend recursion (one level).
- `refund.rs:142-205`: **only the pure helpers** (`refund_transfer`, `deny_or_abstain`); the integrated `check()` path through `AddressPoisoningChecker` is never exercised — which is why the `chain_id = 0` defect (H1) is invisible to the unit suite. `Provider::mocked` (chain `0x5afe`, `provider/mod.rs:139-150`) exists in safenet-core `test-util` (dev-dependency enabled, `Cargo.toml:21`) but is unused in this crate.
- `address_poisoning.rs:392-456`: `nibbles`, `is_lookalike`, `block_chunks` only.

Notable untested paths: header extractors (garbage/oversized values), JSON rejection shapes, `decode_multi_send` on truncated/oversized/`operation ≥ 2` blobs (only exercised indirectly), `NestedSafeChecker`, `EscapeHatchChecker`, `StakingChecker` (entirely), `AddressPoisoningChecker::check` (chain mismatch, partial scan, exact match, forged history), the real `main.rs` checker order (the engine tests use stubs, `engine/mod.rs:79-90`), graceful shutdown.

**External test-vector runner.** `scripts/run_sentinel_engine_integration_test.sh` requires `TEST_VECTORS` pointing at a `sentinel-test-vectors` checkout with `bin/run-tests.sh` (lines 22-31), builds the engine (line 71), writes a temp TOML with `rpc` from `SENTINEL_ENGINE_RPC_URL` (default `https://ethereum-rpc.publicnode.com`), empty blocklist, lookback 50000, optional `SENTINEL_ENGINE_MAX_BLOCK_RANGE`/`SENTINEL_ENGINE_LOG_FILTER` (lines 9-20, 46-68), starts the engine on `127.0.0.1:5473`, polls with `curl` (lines 77-92; a 404 on `/` still counts as ready since `curl` without `-f` exits 0), then runs the corpus with `SENTINEL_ENGINE_URL` set (line 95). Invoked via `just test-integration-sentinel-engine <checkout> [args]` (`Justfile:104-105`). Corpus content: **not read**; note it runs against a live mainnet RPC, so address-poisoning vectors are only as stable as the referenced history and the provider's `eth_getLogs` range cap (50000-block single call by default).

---

## 10. Hypotheses

Severity scale per the brief: Critical = a malicious transaction gets `secure` (sentinel bonds an approving vote); High = honest transactions denied at scale or engine DoS; Medium; Low; Info. Evidence: E2 = defect visible in cited code with a concrete input; I = inference without a concrete trigger.

### H1 — `RefundChecker` is dead code: its synthetic transaction has `chain_id = 0`, so the delegated address-poisoning check always abstains

- **Where:** `checkers/refund.rs:105-117`, `checkers/address_poisoning.rs:309-319`, `checkers/mod.rs:40-47`.
- **Code:**
  ```rust
  // refund.rs:105-117
  Some(SafeTransaction {
      safe: transaction.safe,
      to: transaction.gas_token,
      data: transferCall { to: transaction.refund_receiver, amount: ... }.abi_encode().into(),
      ..Default::default()          // chain_id = U256::ZERO, operation = Call
  })
  // address_poisoning.rs:312-319
  if transaction.chain_id != U256::from(self.provider.chain_id()) {
      tracing::warn!(... "transaction chain id does not match the configured provider");
      return Verdict::Abstain;
  }
  ```
- **Reasoning:** `RefundChecker::check` (`refund.rs:52-57`) calls `AddressPoisoningChecker::check` on the synthetic transfer. That method decodes the target first (`address_poisoning.rs:309-311`, succeeds for a non-zero amount) and then compares `chain_id` (zero) with the provider's real chain id (never zero for a live chain) → `Abstain` plus a `warn!`. `deny_or_abstain(Abstain)` = `Abstain`. The refund checker therefore never denies anything, never issues an RPC call, and logs a misleading chain-mismatch warning on every relayed ERC-20-refund transaction. The `#876` "Remove global refund abstain" change relied on this checker as the replacement mitigation.
- **Evidence class:** E2. **Confidence:** 95%.
- **Confirm:** POST any transaction with `gasPrice: "0x1"`, `safeTxGas: "0x186a0"`, `baseGas: "0x5208"`, `gasToken: <any non-zero address>`, `refundReceiver: <lookalike of a known payee>`, `chainId` equal to the engine's chain. Expect (per design) `insecure R-4.3` when the receiver is a lookalike; observe `abstain` and a log line `address-poisoning check: transaction chain id does not match the configured provider` with `tx_chain_id=0`.
- **Severity:** Medium on its own (a deny-only guard silently disabled); it is the enabler that makes H4 unmitigated.
- **Fix direction:** copy `chain_id` (and ideally `nonce`) into the synthetic transaction; add an integration test with `Provider::mocked_with_chain`.

### H2 — `NestedSafeChecker` affirms `secure` for any `Call` carrying `execTransaction` calldata, ignoring `value` (native-value drain to an arbitrary address)

- **Where:** `checkers/nested.rs:42-47`; ordering `main.rs:62`.
- **Code:**
  ```rust
  fn is_nested_exec_transaction(tx: &SafeTransaction) -> bool {
      tx.operation == Operation::Call
          && tx.to != tx.safe
          && tx.data.starts_with(&safe::execTransactionCall::SELECTOR)
          && safe::execTransactionCall::abi_decode(&tx.data).is_ok()
  }
  ```
- **Reasoning:** Neither `value`, `gasPrice`, nor the identity of `to` is checked. A plain ETH transfer with empty calldata ends the chain in `abstain` (no vote); the same transfer with a well-formed `execTransaction` payload is affirmed `secure` at position #5, before ExcessiveApproval/Cow/Staking/Refund/AddressPoisoning. `to` may be an EOA (accepts any calldata) or any contract with a payable fallback. Earlier checkers cannot stop it: Base allows calls to other contracts (`base.rs:91-93`), Blocklist only if listed. The module doc (`nested.rs:4-11`) argues Article IV-A "lets a Safe call any other contract freely" — but "free" elsewhere in this engine means _abstain_, not _affirm_.
- **Evidence class:** E2. **Confidence:** 90% that the engine returns `secure`; 75% that reviewers will class it as a Critical false-affirm (it depends on whether the Charter ever wants a sentinel to affirm an unvetted value transfer).
- **Confirm:** body with `to: 0x000000000000000000000000000000000000dEaD`, `value: "0x3635c9adc5dea00000"` (1000 ETH), `operation: 0`, `data:` = `cast calldata "execTransaction(address,uint256,bytes,uint8,uint256,uint256,uint256,address,address,bytes)" 0x0000000000000000000000000000000000000000 0 0x 0 0 0 0 0x0000000000000000000000000000000000000000 0x0000000000000000000000000000000000000000 0x`, all gas fields `"0x0"`. Expect `{"verdict":"secure"}`.
- **Severity:** Critical.
- **Fix direction:** require `value == 0` and `gasPrice == 0` (as EscapeHatch does), and consider returning `abstain` rather than `secure` unless `to` is verifiably a Safe (needs RPC: `getOwners`/singleton check) — or make the checker deny-only.

### H3 — `AddressPoisoningChecker` affirms `secure` from history on an attacker-chosen `to`, ignoring `value` (forged-log native-value drain)

- **Where:** `checkers/address_poisoning.rs:116-139` (`decode_target` checks only `operation`), `:194-199` (filter `address(token = transaction.to)`), `:214-222` (`target == candidate` → `ExactMatch`), `:325-333` (→ `Secure`).
- **Code:**
  ```rust
  // address_poisoning.rs:194-199
  let filter = Filter::new().address(token)
      .event_signature(vec![Transfer::SIGNATURE_HASH, Approval::SIGNATURE_HASH])
      .topic1(safe).from_block(chunk_from).to_block(chunk_to);
  // :218-219
  if target == candidate { return Ok(RecipientLookup::ExactMatch); }
  // :325-333  Ok(RecipientLookup::ExactMatch) => { ...; Verdict::Secure }
  ```
- **Reasoning:** The "token" whose history is consulted is `transaction.to`, which the proposer controls. An attacker deploys a contract `T` that emits `Transfer(indexed from = safe, indexed to = X, 1)` (events are free-form; no allowance needed — this differs from the documented `transferFrom` forgery at `address_poisoning.rs:26-39`, which assumed a real token). They then propose `{to: T, value: <all ETH>, data: transfer(X, 1), operation: 0}`. Every earlier checker abstains (Base: call to other contract; Blocklist: not listed; Nested: wrong selector; ExcessiveApproval: value/transfer effects only; Cow: no relayer approve; Staking: wrong addresses; Refund: gasPrice 0). AddressPoisoning decodes `transfer(X, 1)`, chain matches, the `eth_getLogs` on `T` returns the forged log → `ExactMatch` → `Secure`. The native `value` is never inspected by this checker, and `T.transfer` can be payable.
- **Evidence class:** E2. **Confidence:** 85%.
- **Confirm:** on a devnet, deploy `T` with `function transfer(address to, uint256) external payable { emit Transfer(SAFE, to, 1); }` and call it once so the log exists at a block ≤ `block`; POST the transaction above with `block` ≥ that block. Expect `secure`.
- **Severity:** Critical.
- **Fix direction:** require `tx.value.is_zero()` in `decode_target`; consider making address poisoning deny-only (`Secure` → `Abstain`), or at minimum never affirm when `to` has no independent evidence of being a token.

### H4 — The gas-refund leg is unvetted whenever any affirming checker fires (refund drain rides on Nested/Cow/Staking/AddressPoisoning `secure`)

- **Where:** `main.rs:57-73` (order), `checkers/refund.rs:83-96` (TODOs), `nested.rs:42-47`, `cow.rs:351-383`, `staking.rs:109-116`, `address_poisoning.rs:325-333` (none read `gas_price`).
- **Code:**
  ```rust
  // refund.rs:83-89 (doc)
  //   TODO(follow-up): ... Since the engine-wide "abstain on any nonzero `gasPrice`" guard that used to sit
  //   ahead of the whole checker chain is gone, a transaction another checker calls `Secure` can now drain
  //   unbounded native currency to `refundReceiver` uncommented on.
  // main.rs:66-70 (comment): "`RefundChecker` can only deny or abstain (never affirm), so its position
  //   relative to `address_poisoning` doesn't affect correctness"
  ```
- **Reasoning:** Safe pays `(gasUsed + baseGas) × gasPrice` in `gasToken` (uncapped by `tx.gasprice` for ERC-20 tokens; capped by `tx.gasprice` for native, but `baseGas` is unbounded) to `refundReceiver` `[inference: Safe.sol handlePayment]`. Only `EscapeHatch` (`gasPrice == 0`) and `Cancellation` (all-zero) guard against this among the affirmers. Refund runs 9th, after all affirmers, so its (already dead, H1) denial could never apply to an affirmed transaction; and it only ever covered the ERC-20 + lookalike case anyway. Combined with H2: one request drains both `value` and a refund.
- **Evidence class:** E2. **Confidence:** 90%.
- **Confirm:** the H2 body plus `baseGas: "0xe8d4a51000"` (10^12), `gasPrice: "0x3b9aca00"`, `gasToken: 0x0`, `refundReceiver: <attacker>`. Expect `secure`. Variant with `gasToken: <USDC>` and `gasPrice: "0xde0b6b3a7640000"` also `secure`.
- **Severity:** Critical (in combination); High standalone for the Cow/Staking shapes (a genuine-looking CoW batch with a hostile refund leg is affirmed).
- **Fix direction:** reinstate a global "nonzero `gasPrice` ⇒ abstain unless a refund policy affirms it" gate ahead of the chain, or move a working Refund checker to position #1 and make it deny on any nonzero refund it cannot vet.

### H5 — `EscapeHatchChecker` affirms `secure` for the announcement shape to **any** `to` and runs before `BlocklistChecker`

- **Where:** `checkers/escape_hatch.rs:52-61`; order `main.rs:58-61`; on-chain rule `contracts/src/guard/SafenetGuard.sol:355-368`.
- **Code:**
  ```rust
  fn is_escape_hatch_call(tx: &SafeTransaction) -> bool {
      if tx.operation != Operation::Call || !tx.value.is_zero() || !tx.gas_price.is_zero() { return false; }
      tx.data.starts_with(&safenet_guard::announceTransactionCall::SELECTOR)
          || tx.data.starts_with(&safenet_guard::cancelAnnouncementCall::SELECTOR)
  }
  ```
  vs. `SafenetGuard.sol:360`: `if (to != address(this) || value != 0 || operation != Call || gasPrice != 0 || data.length < 4) return false;`
- **Reasoning:** The on-chain guard auto-allows this shape only when `to` is the guard itself (in which case no attestation is needed and the sentinel's vote is moot). The engine affirms it for every `to`, which is precisely the set of cases where the vote _matters_. Concretely, a zero-value call to a blocklisted or attacker contract whose calldata begins with the `announceTransaction` selector (any suffix; no ABI decode) is `secure` and never reaches `BlocklistChecker` (#4), contradicting the ordering rationale written for `NestedSafeChecker` (`nested.rs:10-11`). Fund-loss exploitability needs pre-existing authority held by `to` or a fallback-driven contract, so this is mainly an "affirmation without evidence" and blocklist-bypass defect.
- **Evidence class:** E2. **Confidence:** 85% (fact); 50% (practical fund impact).
- **Confirm:** configure `blocklist = ["0x1111...1111"]`, POST `{to: 0x1111...1111, value: "0x0", gasPrice: "0x0", operation: 0, data: <selector of announceTransaction((address,uint256,bytes,uint8,uint256,uint256,uint256,address,address))> + "00"}`. Expect `secure`; the blocklist is never consulted.
- **Severity:** Medium.
- **Fix direction:** move EscapeHatch after Blocklist; restrict `to` to a configured/registered guard address (none is tracked today, `escape_hatch.rs:18-19`); or return `abstain` since the guard auto-allows the legitimate case anyway.

### H6 — `BlocklistChecker` inspects only the top-level `to`; MultiSend and nested-Safe wrappers bypass it

- **Where:** `checkers/blocklist.rs:25`; `multi_send.rs:168-172` unused here.
- **Reasoning:** A MultiSend delegatecall to a canonical deployment with one sub-call `to = blocklisted` has top-level `to = MultiSend`; Base allows sub-calls to other contracts; the chain ends in `abstain` (no vote) instead of the intended `insecure R-4.6`. Same for the inner `to` of a nested `execTransaction` (affirmed `secure`, H2).
- **Evidence class:** E2. **Confidence:** 90%. **Severity:** Medium (missed denial; the network cannot rely on the operator's blocklist).
- **Confirm:** MultiSend (`to: 0xA1dabEF33b3B82c7814B6D82A79e50F4AC44102B`, `operation: 1`) wrapping one `Call` to a blocklisted address with `value: 1`. Expect `abstain`, not `insecure R-4.6`.

### H7 — Near-unlimited `approve` to any previously-paid address is affirmed `secure`

- **Where:** `excessive_approval.rs:22` (`amount == U256::MAX` only); `address_poisoning.rs:132-137,325-333`.
- **Reasoning:** `approve(X, 2^256−2)` evades R-4.5 (literal max only, per `rule.rs:28-33`), and if the Safe ever paid or approved `X` (any non-zero `Transfer`/`Approval` in the window) the address-poisoning exact match affirms `secure`. Amount reasonableness is explicitly out of scope for the MVP, so this is a Charter-scope decision rather than a coding bug — but the _affirmation_ (rather than abstention) is what puts the bond at risk.
- **Evidence class:** E2. **Confidence:** 80% (behaviour); 40% (that it is out of policy). **Severity:** Medium/High.
- **Confirm:** `to: <real token>`, `data: approve(<prior payee>, 0xffff…fffe)`; expect `secure`.

### H8 — CoW TWAP tolerance is sized by attacker-chosen `n`; huge approvals to the relayer are affirmed

- **Where:** `cow.rs:376,552-554` (`total + n − 1`), `cow.rs:540` (`partSellAmount * n`).
- **Reasoning:** `partSellAmount = 0, n = 2^255` → `total = 0`, `max = 2^255 − 1`, so `approve(GPv2VaultRelayer, 2^255−1)` + such a `createWithContext` is `secure`. The relayer can only spend via the Safe's own orders (mitigated by CoW's design `[inference]`), so impact is limited to violating R-4.5's intent.
- **Evidence class:** E2. **Confidence:** 85%. **Severity:** Low/Medium.

### H9 — `is_presignature`/`is_twap_create` accept shapes that the paired decoders reject, turning a dangling relayer approval into `abstain` instead of `insecure`

- **Where:** `cow.rs:478-483` (no `signed` check) vs `cow.rs:561-569` (requires `signed == true`); `cow.rs:492-499` (no `staticInput` decode) vs `cow.rs:531-542`.
- **Reasoning:** `[approve(relayer, amt), setPreSignature(uid, false)]` passes the dangling check (a "trigger" is present, `cow.rs:392-401`), fails the presignature decode, fails TWAP → overall `abstain`. Likewise garbage `staticInput` with the canonical handler/factory. Missed denial only (no false `secure`).
- **Evidence class:** E2. **Confidence:** 85%. **Severity:** Low.

### H10 — No server-side deadline; `x-request-timeout` is parsed and discarded; RPC and CoW clients have no timeouts

- **Where:** `api/mod.rs:48-50`; `cow.rs:229-231,196-203`; `provider/mod.rs:129-137` (no timeout configured `[inference on alloy/reqwest defaults]`).
- **Reasoning:** A stalled provider or `api.cow.fi` pins handler tasks until the sentinel disconnects; nothing bounds concurrent in-flight work. Liveness only.
- **Evidence class:** E2 (code) / I (impact). **Confidence:** 95%. **Severity:** Medium (engine DoS/liveness).

### H11 — Latent unbounded recursion in `decode_target_effects`, shielded only by checker ordering

- **Where:** `target_effects.rs:46-49`; shield at `base.rs:205-213` + `main.rs:60,63`.
- **Reasoning:** A 2 MiB body can encode several thousand nesting levels; each level recurses through `decode_target_effects` → `decode_multi_send_call` → `flat_map`. Today Base denies nested MultiSend first, so the recursion never runs on adversarial depth. Reordering checkers, relaxing Base, or calling `decode_target_effects` from an earlier checker would expose a stack-overflow (process abort, not a 500).
- **Evidence class:** I. **Confidence:** 90% (latent fact). **Severity:** Low now; High if reachable.

### H12 — Shutdown is not graceful

- **Where:** `main.rs:79-84`. Dropping `axum::serve` aborts in-flight checks; the sentinel records `EngineCheckVerdict::Error` and drops the request (`crates/sentinel/src/engine.rs:181-187`). **Severity:** Low. **Confidence:** 90%.

### H13 — Abstain-on-failure is indistinguishable from deliberate abstain on the wire

- **Where:** `engine/mod.rs:46-47`; `address_poisoning.rs:369-378`; `cow.rs:318-325`. A degraded RPC/API silently turns the engine into an "abstain everything" engine; only logs reveal it; no engine-side metric exists. **Severity:** Info/Low (operability). **Confidence:** 95%.

### H14 — Operator documentation understates external dependencies

- `docs/sentinel-engine.md:88` names address poisoning as the only externally-backed check; `RefundChecker` (RPC via AP) and `CowChecker` (HTTPS to `api.cow.fi`) are omitted; `/health` lives on the ephemeral metrics port by default (`observability/mod.rs:33`). **Severity:** Info. **Confidence:** 95%.

### Considered and rejected (non-findings)

- **Panics/500s from malformed bodies or calldata:** none found (§8); the MultiSend cursor and all ABI decodes are total.
- **JSON depth/size bombs:** serde_json's recursion limit and axum's body limit apply `[inference]`; quantities are strings, not numbers.
- **Header injection:** rejections are plain 400s before body parsing; the sentinel always sends canonical values.
- **CoW API spoofing:** the EIP-712 digest recompute (`cow.rs:143-169,296-303`) binds the response to the requested UID (pinned against a real mainnet order in tests); `owner` is bound inside the UID; `setPreSignature` on-chain requires `owner == msg.sender` `[inference]`, so an order owned by someone else cannot be presigned by the Safe anyway.
- **`block_chunks` non-termination / range explosion:** impossible (`from ≤ to`, span ≤ lookback, §8).
- **Nested MultiSend reaching the recursive decoder:** blocked by Base ordering (H11 is the latent form).
- **`Operation` values outside {0,1} on either side:** rejected at deserialisation (`transaction.rs:20-29`) and the sentinel refuses to serialise them (`crates/sentinel/src/bindings.rs:126`).
- **Wire-contract drift between sentinel and engine:** field names, casing, quantity encoding, `operation` u8, verdict tags and rule-code grammar all agree (§3.1).
- **Selector collisions to weaponise EscapeHatch/Nested against existing privileged contracts:** ~2^-32 per function; impractical.
- **`transferFrom(safe, X, 1)` forgery on real tokens:** requires an allowance; documented and not new (H3 is the allowance-free variant via a malicious `to`).
- **Cancellation with `value > 0` or non-zero refund fields:** correctly falls through (`cancellation.rs:16-27`).

---

## 11. Suggested review checklist (ordered by risk)

1. **Should any checker ever return `secure`?** For each affirmer — `nested.rs:42-47`, `escape_hatch.rs:52-61`, `address_poisoning.rs:325-333`, `cow.rs:309-313,382`, `staking.rs:110-116,125-127,145-147` — decide whether the Charter authorises bonding an approving vote on that evidence, and whether `value`/`gasPrice`/`gasToken`/ `refundReceiver` must be zero for the affirmation to hold (H2, H3, H4).
2. **Refund leg:** confirm H1 (`refund.rs:105-117` vs `address_poisoning.rs:312`), then decide the policy for non-zero `gasPrice` given the chain short-circuits at the first affirmer (`engine/mod.rs:62-69`, `main.rs:57-73`). Compare with the pre-#876 global abstain (git `31af5e6`).
3. **Checker ordering as a security property:** enumerate every (affirmer before denier) pair in `main.rs:57-73`; EscapeHatch-before-Blocklist (H5), Nested-before-everything, Cow/Staking-before-AddressPoisoning. Consider a two-phase engine: run all deny-only checkers first, then affirmers.
4. **Address-poisoning evidence provenance:** `to` is attacker-chosen (`address_poisoning.rs:194,322`); is any history on an unverified contract acceptable evidence (H3)? Should `Secure` be downgraded to `Abstain`?
5. **Blocklist reach:** `blocklist.rs:25` vs MultiSend/nested sub-calls (H6); decide whether R-4.6 must apply to every touched address (`multi_send::sub_transactions`, nested `execTransaction.to`).
6. **MultiSend zero-address semantics per version:** verify against `MultiSend.sol` for each address in `multi_send.rs:27-68` that only the two `V150Plus` entries redirect `to == 0` to `address(this)`; a mis-tagged deployment is an R-4.1 bypass (`multi_send.rs:111-114`, `base.rs:91-93`).
7. **Base allow-lists:** review `base.rs:13-30,156-186` (fallback handlers, modules, migration/sign/CreateCall contracts) against current Safe deployments; note `setGuard(0)` (guard removal) is allowed while re-setting any guard is denied (`base.rs:23,130-134`), and `enableModule` to two specific modules is allowed (`base.rs:25-28`) — modules bypass the guard entirely (`SafenetGuard.sol:26-27`).
8. **CoW checks:** attacker-controlled `n` tolerance (`cow.rs:552-554`, H8); `signed=false`/garbage `staticInput` evasions (`cow.rs:478-499`, H9); `feeAmount` handling (`cow.rs:312` requires `approved == sellAmount`; orders with non-zero on-chain fee would be denied `[inference]`); presence of a request timeout and size cap on `order_uid` (`cow.rs:196-203`).
9. **Staking:** `validator` unchecked (`staking.rs:15-21`), `claim` set-aside semantics (`staking.rs:97-107`); confirm the canonical addresses at `staking.rs:66-74` and that `REWARDS_DISTRIBUTOR.claim` indeed pays `account`.
10. **Resource bounds:** add `TimeoutLayer`/concurrency limit, honour `x-request-timeout` (`api/mod.rs:48-50`), set RPC and CoW client timeouts; decide whether `decode_target_effects` needs a depth cap (`target_effects.rs:47-48`, H11).
11. **Observability of degradation:** distinguish failure-abstain from policy-abstain (metric or verdict reason); expose `/health` reachably (`observability/mod.rs:33`).
12. **Test-vector corpus:** check whether `sentinel-test-vectors` contains vectors for RefundChecker denial, Nested with value, EscapeHatch with non-guard `to`, MultiSend-wrapped blocklist hits, and forged-history address poisoning — the unit suite covers none of them (§9).
13. **Wire contract hygiene:** document non-200 responses in `openapi.yaml`; decide whether hex leniency beyond the documented patterns is acceptable (`transaction.rs:78-79`).
