# F-ENG-009 `EngineConfig` performs no validation: the lookback and max-range pair silently sets the per-request `eth_getLogs` fan-out, with no bound, no derived-value check and no startup log

| Field | Value |
| --- | --- |
| Status | QA-done |
| Crate and module | sentinel-engine, `config.rs` |
| Location | `crates/sentinel-engine/src/config.rs:37-68` (related: `crates/sentinel-engine/src/checkers/address_poisoning.rs:189-213`, `250-268`; `crates/sentinel-engine/src/main.rs:41-56`; `crates/sentinel-engine/sentinel-engine.sample.toml:24-33`) |
| Severity | Low / Low |
| Certainty | 75% |
| Assumptions involved | A1, A4 |
| Tags | config, dos |

## Claim

`Config::load` reads the file and hands it to `toml::from_str`. That is the whole of configuration validation — there is no `validate`, no cross-field check, and no bound on any numeric field. `address_poisoning_lookback_blocks` is a bare `u64` and `address_poisoning_max_block_range` an `Option<NonZeroU64>`, and neither is individually dangerous. Their _ratio_ is: it silently determines how many sequential `eth_getLogs` calls the engine issues on the critical path of a single security check.

`block_chunks` splits `[block - lookback, block]` into chunks of `max_range + 1` block numbers, so the count is `ceil((lookback + 1) / (max_range + 1))`, and `established_recipients` issues one `eth_getLogs` per chunk in a sequential `for` loop with no deadline. With the sample's `lookback = 50000` and the commented-out `max_block_range = 10000`, that is 5 calls — entirely reasonable, which is exactly why nobody notices the shape of the function. Set `max_block_range = 1` against the same lookback and it is **25,001 sequential RPC round trips per request**. The configuration is accepted, the engine starts, and nothing anywhere reports the derived number: `main.rs:46-49` logs only that a config file was loaded, at `debug`, without echoing a single value.

The knob is unusually easy to get wrong, and the code knows it. `max_block_range`'s own doc comment spends ten lines explaining that it is a `toBlock - fromBlock` _span_ rather than a block count, and warns "if a provider documents its cap as an inclusive block count `N`, use `N - 1` here". A setting that needs a paragraph of prose to prevent an off-by-one is a setting that warrants a validity check, not just a comment — and the failure it guards against is silent, since a too-small value produces a working engine that is merely thousands of times slower per request.

Under A1 the operator is trusted, so this is a Low-band configuration weakness rather than an attack. It is worth filing because of what it composes with. Under A2 the proposer decides whether a request reaches the address-poisoning path at all (any ERC-20-shaped `to` with a matching `chainId` does it), and under F-ENG-005 nothing bounds the wall-clock time of that fan-out or the engine's concurrency. A misconfiguration that would be self-evident in a batch job — "it is slow" — is here a per-request amplifier on an attacker-selected code path. Under A4 a rate-limited provider is explicitly in scope, and thousands of calls per check is a reliable way to become rate-limited, which degrades every subsequent check to `Abstain`.

## Basis

| # | Claim | Class | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The two fields are unbounded and unrelated in the schema; nothing constrains their combination. | E2 | `crates/sentinel-engine/src/config.rs:37-51` | <pre>pub struct EngineConfig {<br> /// Transaction destinations that are always considered insecure.<br> pub blocklist: Vec<Address>,<br> /// Number of blocks inspected for a prior interaction with a candidate<br> /// address.<br> pub address_poisoning_lookback_blocks: u64,<br> /// The widest `toBlock - fromBlock` _span_ the configured RPC allows in<br> /// a single `eth_getLogs` call — the same number reported in a<br> /// provider's own "range exceeds limit" error, **not** a block count<br> /// (a call from block 100 to 110 has a span of 10 but covers 11 block<br> /// numbers; if a provider documents its cap as an inclusive block count<br> /// `N`, use `N - 1` here). Unset by default, which issues the whole<br> /// `address_poisoning_lookback_blocks` window as a single call; set<br> /// this when the provider caps it below that, and the lookback is<br> /// split into consecutive calls of at most this width to still cover</pre> |
| 2 | Loading is `read_to_string` plus `from_str` — there is no validation step of any kind. | E2 | `crates/sentinel-engine/src/config.rs:61-68` | <pre>impl Config {<br> /// Loads a configuration from a file.<br> pub async fn load(file: &Path) -> Result<Self, Error> {<br> let contents = fs::read_to_string(file).await?;<br> let config = toml::from_str(&contents)?;<br> Ok(config)<br> }<br>}</pre> |
| 3 | The chunk count is `ceil((lookback + 1) / (max_range + 1))`: each chunk covers `max_range + 1` block numbers. | E2 | `crates/sentinel-engine/src/checkers/address_poisoning.rs:254-268` | <pre>) -> impl Iterator<Item = (u64, u64)> {<br> let max_range = max_range.unwrap_or(NonZeroU64::MAX).get;<br> let mut start = from;<br> let mut done = false;<br> std::iter::from_fn(move \|\| {<br> if done {<br> return None;<br> }<br> let end = start.saturating_add(max_range).min(to);<br> let chunk = (start, end);<br> done = end >= to;<br> start = end.saturating_add(1);<br> Some(chunk)<br> })<br>}</pre> |
| 4 | One `eth_getLogs` is issued per chunk, sequentially, inside a single security check. | E2 | `crates/sentinel-engine/src/checkers/address_poisoning.rs:189-200` | <pre> let from_block = current_block.saturating_sub(self.lookback_blocks);<br> let mut recipients = HashSet::new;<br> let mut complete = true;<br> for (chunk_from, chunk_to) in block_chunks(from_block, current_block, self.max_block_range)<br> {<br> let filter = Filter::new<br> .address(token)<br> .event_signature(vec![Transfer::SIGNATURE_HASH, Approval::SIGNATURE_HASH])<br> .topic1(safe)<br> .from_block(chunk_from)<br> .to_block(chunk_to);<br> let logs = match self.provider.get_logs(&filter).await {</pre> |
| 5 | The values are passed straight from config into the checker with no inspection. | E2 | `crates/sentinel-engine/src/main.rs:51-56` | <pre> let provider = Provider::connect(&rpc).await?;<br> let address_poisoning = Arc::new(AddressPoisoningChecker::new(<br> provider,<br> engine_config.address_poisoning_lookback_blocks,<br> engine_config.address_poisoning_max_block_range,<br> ));</pre> |
| 6 | Startup logs the config _path_ only, at `debug`, so no effective value and no derived fan-out is ever reported. | E2 | `crates/sentinel-engine/src/main.rs:41-49` | <pre> let config = Config::load(&options.config_file).await?;<br> let bind_address = config.bind_address;<br> let rpc = config.rpc;<br> let engine_config = config.engine;<br> observability::init(config.observability)?;<br> tracing::debug!(<br> config_file = %options.config_file.display,<br> "sentinel engine configuration loaded"<br> );</pre> |
| 7 | The shipped sample pairs `lookback = 50000` with a suggested `max_block_range = 10000`, i.e. the benign five-call case that makes the ratio invisible. | E2 | `crates/sentinel-engine/sentinel-engine.sample.toml:24-33` | <pre># Number of recent blocks the address-poisoning check searches for a prior<br># interaction with a candidate address.<br>address_poisoning_lookback_blocks = 50000<br><br># Optional: widest toBlock-fromBlock span the RPC above allows in one<br># eth_getLogs call (not a block count — if a provider documents its cap as<br># an inclusive block count N, use N - 1 here). Unset issues the lookback<br># above as a single call; set this to a provider's own documented cap and<br># it's split into consecutive calls instead.<br># address_poisoning_max_block_range = 10000</pre> |

## Trigger

Configuration, then any request that reaches checker #10:

```toml
[engine]
blocklist = []
address_poisoning_lookback_blocks = 50000
address_poisoning_max_block_range = 1
```

This parses, passes every existing test in `config.rs:70-153`, and starts the engine. Then `POST /v1/security-check` with `operation: 0`, a `chainId` matching the configured RPC, any `to`, and `data` = `transfer(<recipient>, 1)`. `AddressPoisoningChecker` computes `block_chunks(block - 50000, block, Some(1))`, which yields 25,001 chunks, and issues 25,001 sequential `eth_getLogs` calls — none of which has a timeout (F-ENG-005) — before the engine can answer. Under A2 the proposer chose that path by choosing the calldata.

The realistic version of the same mistake is smaller and likelier: an operator reads their provider's "10,000 blocks per request" limit and writes `address_poisoning_max_block_range = 10` after confusing the units, or copies a value meant for a chain with different block times. Nothing rejects it, nothing logs it, and the only symptom is that checks became slow.

## Considered and rejected

- **"A1 says the operator is trusted, so a bad config is their own problem."** Accepted as far as severity — this is Low, in the band PROMPT § 8 describes as "configuration weaknesses with limited impact", and I have not inflated it. But A1 means the operator is not an _adversary_; it does not mean they cannot make a mistake that no code catches and no log reveals. The crate already validates elsewhere for exactly this reason: `deny_unknown_fields` on both tables (`config.rs:20,36`) catches a typo'd key, and `NonZeroU64` catches the one degenerate value the type system can express.
- **"The unbounded `lookback_blocks` is the real problem."** Checked and rejected as the lesser half. With `max_block_range` unset, a huge lookback produces exactly one `eth_getLogs` over an absurd range (`block_chunks` with `NonZeroU64::MAX` yields a single chunk, row 3), which the provider rejects; with `recipients` still empty the error propagates (`address_poisoning.rs:212`) and the verdict is `Abstain` with a `warn!`. Noisy and fail-safe. The _ratio_ is the amplifier, not either field alone.
- **"`block_chunks` might not terminate or might overflow for adversarial values."** Checked and rejected. `end = start.saturating_add(max_range).min(to)` is monotonically increasing and clamped, `done = end >= to` fires on the first chunk that reaches `to`, and `from = to.saturating_sub(lookback)` guarantees `from <= to` for every `u64`. `max_range.unwrap_or(NonZeroU64::MAX)` degenerates to a single chunk rather than to zero-width chunks. There is no non-termination and no overflow here.
- **"The `blocklist: Vec<Address>` is unbounded too."** Rejected as immaterial: it is a linear scan of an operator-supplied list against one address per request, with no I/O and no allocation.
- **"`observability.metrics_address` defaulting to an ephemeral loopback port is a related config defect"** — it is (the `/health` endpoint is unreachable to an orchestrator by default), but the field is defined in `safenet-core`'s `observability::Config`, not here, and the sample file (`sentinel-engine.sample.toml:39-42`) documents the override. Out of my scope; recorded in the R8 coverage log as an observation for R2/R10.
- **"This duplicates F-ENG-005."** They meet but do not overlap: F-ENG-005 is that no _time_ bound exists anywhere, this is that no _call-count_ bound exists and that the count is a silently-derived configuration product. Fixing one does not fix the other — a timeout on a 25,001-call walk still burns the whole budget and yields `Abstain`.

## Remediation options

1. **Validate the derived value, not just the fields.** Give `Config` a `validate` called from `load` that computes `ceil((lookback + 1) / (max_range + 1))` and rejects a configuration whose per-request chunk count exceeds a small constant (tens, not thousands), with an error naming both inputs and the computed count. This is the one change that turns a silent misconfiguration into a startup failure, and it is where a `max_range` given in the wrong units gets caught.
2. **Log the effective configuration at startup.** `main.rs:46-49` already has a `debug!` there; make it `info!` and include `lookback_blocks`, `max_block_range` and the derived chunk count. Cheap, and it makes the mistake diagnosable from the first line of a service's logs rather than from a latency graph.
3. **Bound the work at the point of use instead.** Cap the number of chunks `established_recipients` will issue and set `complete = false` past the cap — the function already has that concept for a failed chunk (`address_poisoning.rs:202-211`), and an over-long scan is arguably the same situation as an interrupted one. Tradeoff: it silently narrows the configured lookback, so it should accompany option 2 rather than replace it. (`checkers/address_poisoning.rs` is R9's scope.)

Tests to add: a `config.rs` case asserting that an out-of-bounds pair is rejected by `validate`; a `block_chunks` case pinning the chunk count for the sample values and for a pathological pair — the existing `block_chunks` tests (`address_poisoning.rs:392-456`) cover the splitting arithmetic but assert nothing about how many calls that implies.

## Trail

- Reviewer R8: drafted, self-estimate 75%. All rows are direct reads of this checkout; nothing here is class `I`. The self-estimate reflects that this is a judgement about where validation belongs rather than a defect with a wrong output — a reviewer could fairly say an operator who writes `max_block_range = 1` deserves what they get. I file it because the failure is silent, the knob's own doc comment concedes it is easy to misread, and the resulting amplification lands on an attacker-selected code path with no time bound (F-ENG-005) above it. The chunking half lives in `checkers/address_poisoning.rs`, which is R9's scope.

## Critic (C-ENG-A)

I derived the chunk arithmetic from `block_chunks` myself before reading the argument. **No claim is `H`**; all seven `Basis` rows re-opened clean.

### Per-claim verdicts

**Supported — the arithmetic, which I recomputed independently.** `address_poisoning.rs:250-268`:

```
fn block_chunks(from: u64, to: u64, max_range: Option<NonZeroU64>) -> impl Iterator<Item = (u64, u64)> {
    let max_range = max_range.unwrap_or(NonZeroU64::MAX).get;
    ...
        let end = start.saturating_add(max_range).min(to);
        let chunk = (start, end);
        done = end >= to;
        start = end.saturating_add(1);
```

Each chunk covers `max_range + 1` block numbers, so the count over a `lookback + 1` window is `ceil((lookback + 1) / (max_range + 1))`. With `lookback = 50000` and `max_range = 1` that is `ceil(50001 / 2) = 25_001`. R8's figure is exactly right. And `established_recipients:192-213` issues one `provider.get_logs(&filter).await` per chunk in a sequential `for` loop, on the request's critical path.

**Supported — no validation exists.** `config.rs:61-67` is `read_to_string` then `toml::from_str`, and there is no `validate` anywhere in the file. `main.rs:41-56` logs only `config_file = %options.config_file.display` at `debug` — no effective value, no derived fan-out.

**Supported — the knob is easy to get wrong and the code knows it.** `config.rs:43-52` spends ten lines explaining that `max_block_range` is a `toBlock - fromBlock` _span_ and warning "if a provider documents its cap as an inclusive block count `N`, use `N - 1` here". R8's observation that a setting needing a paragraph of prose to prevent an off-by-one warrants a validity check is, I think, the right way to frame this.

### Severity and certainty

**Low / Low — confirmed.** Correct band and correctly reasoned. Under **A1** the operator is trusted, so the misconfiguration is not an attack; what makes it more than a documentation nit is what it composes with — under **A2** the proposer selects whether a request reaches the address-poisoning path at all (any ERC-20-shaped `to`), and under **A4** a rate-limited provider is explicitly in scope, so a per-request amplifier on an attacker-selected path is a real degradation vector. Not Medium: the trigger requires an operator error that no one is forced into, and the symptom (slow checks, then abstains) is fail-safe.

I would also add — and the reviewer's own Trigger says it — that the _realistic_ version is smaller and likelier than 25,001: an operator writing `max_block_range = 10` after confusing span for count still gets ~4,546 sequential calls per check. The extreme figure makes the report vivid; the plausible one makes it credible, and both belong in the summary.

**Confirmed — 75%.** Mechanism fully verified by recomputation; trigger is a config file plus one request. Held below F-ENG-001's 85 because the harm depends on operator behaviour that no evidence in this checkout predicts, and because "thousands of `eth_getLogs` calls degrade the deployment" is reasoned rather than measured — nothing was executed this run.

### Relationship to F-XC-005 (R10)

**Related but distinct — neither subsumes the other, and both should be reported.** They are the two opposite failure modes of the same unvalidated field pair:

- **F-XC-005** — `max_block_range` **unset** with `lookback = 50000` (the shipped sample) issues the whole window as _one_ oversized `eth_getLogs`, which a capped provider rejects on the first chunk, so `recipients` is still empty, the partial-scan branch at `address_poisoning.rs:202-211` is not taken, the error propagates, and the checker abstains on **every** request. Too few chunks.
- **F-ENG-009** — `max_block_range` set too **small** produces thousands of chunks per request. Too many chunks.

**Canonical: `F-XC-005` for the shipped-sample harm** (it is concrete, it ships, and it silently disables a checker); **`F-ENG-009` for the absent validation** (no `validate`, no cross-field check, no derived-value startup log), which is the root cause of both and the thing a fix must address. The remediation in this file — validate the pair and log the derived fan-out at startup — would have caught F-XC-005 as well, and the report should say so.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the finding is about a _configuration_ that is accepted without validation, and its observable is RPC volume, not a verdict. The nearest executable form is option 1's own test (a `Config::validate` case), which does not exist yet because the function does not exist.

**Certainty unchanged at 75%. Severity unchanged at Low.** Confirmed by inspection: `EngineConfig` (`config.rs:36-55`) has no validation of any kind; `Config::load` (`:63-67`) is `read_to_string` plus `toml::from_str`; `address_poisoning_max_block_range` is `Option<NonZeroU64>` so only zero is excluded, and nothing relates it to `address_poisoning_lookback_blocks`; `block_chunks` (`address_poisoning.rs:249-268`) then issues `ceil((lookback + 1) / (max_range + 1))` sequential `eth_getLogs` calls per request with no cap.

### Remediation check

- **Option 1 (validate the derived value, not just the fields) — sound, and it is the right shape**: the defect is that two individually-plausible numbers combine into an implausible chunk count, so validating each field separately would not have caught it. The error message must name **both inputs and the computed count**, because the realistic mistake is a units error (the config comment at `config.rs:44-52` documents a span-vs-count gotcha at some length, which is itself evidence that the units are easy to get wrong), and a message quoting only the count does not tell the operator which number to change.
- **Option 2 (log the effective configuration at startup) — sound, cheap, and I would treat it as mandatory rather than optional.** `main.rs:46-49` already logs at `debug!` that a config was loaded without saying what it contains, so today the derived fan-out is invisible until it shows up as latency.
- **Option 3 (bound the work at the point of use) — sound, and the finding's caveat is the right one:** it silently narrows the configured lookback, so it must accompany option 2, not replace it. I would add that option 3 has a **second** benefit the finding does not claim: `established_recipients` already has the `complete = false` concept for an interrupted scan (`address_poisoning.rs:202-211`), and an over-long scan genuinely _is_ the same situation — so option 3 is not a new concept, only a new trigger for an existing one, and it correctly prevents a truncated scan from producing a _denial_.
- **This finding is a prerequisite for two others, which the report should sequence explicitly.** F-ENG-044 option 1 (conjunctive affirmation) makes the RPC-backed checkers run on far more requests, and F-ENG-033 option 4 (per-log provenance) multiplies the fan-out by the log count. Both are unsafe while the fan-out is unbounded. **Order: F-ENG-005 option 1 (wall-clock cap), F-ENG-009 options 1+2 (count cap), then F-ENG-044, then F-ENG-033 option 4.**
- **Test hook: exists on both sides.** `config.rs:70-153` already has a test module for the parser, and `address_poisoning.rs:392-456` already tests `block_chunks`' splitting arithmetic — it simply asserts nothing about _how many_ calls the split implies, which is the one property that matters here.
- **Where the fix belongs: config validation and startup logging**, with an optional bound in the checker. Not the combinator or the `RuleId` mapping.
