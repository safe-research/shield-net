# PoC — F-XC-005 (the shipped sample disables the address-poisoning check on any provider error)

**Never compiled, never run.** No Rust toolchain (A9 FALSE). Commit `2893917`.

## What it shows

C-XC's note asks QA for two tests: a mocked-provider first-chunk error asserting `Abstain`, and a
later-chunk error asserting the partial-scan path. **This file contains the first**, plus a
configuration assertion pinning the shipped sample. The second is specified below but not written
— see "What is missing and why".

The first test is the finding: with `address_poisoning_lookback_blocks = 50000` and
`address_poisoning_max_block_range` unset — the pair shipped in
`crates/sentinel-engine/sentinel-engine.sample.toml`, echoed in the crate's own test fixture
(`crates/sentinel-engine/src/config.rs:75-81`) and in `docs/sentinel-engine.md:45-76` — the whole
window is one `eth_getLogs` call. When it fails, `established_recipients` returns `Err` rather
than an incomplete result, because the partial-scan arm
(`crates/sentinel-engine/src/checkers/address_poisoning.rs:201-211`) only fires when
`!recipients.is_empty` and nothing was gathered. The verdict is a hard `Abstain`: the check is
not degraded, it is absent.

Under A4 a stale, rate-limited or incomplete RPC response is explicitly in scope, so this is not
a hypothetical failure.

## Install and run

Append `append-to-crates-sentinel-engine-src-checkers-address_poisoning.rs` inside the existing
`mod tests` block at the end of
`crates/sentinel-engine/src/checkers/address_poisoning.rs`.

```sh
cargo test -p sentinel-engine qa_xc_005 -- --nocapture
```

`Provider::mocked` comes from `safenet-core`'s `test-util` feature, already enabled as a
dev-dependency (`crates/sentinel-engine/Cargo.toml:24`). `Asserter::push_failure_msg` is used the
same way at `crates/core/src/index/events.rs:1191`.

## Reading the result

| Outcome | Meaning | Action |
| --- | --- | --- |
| Both **pass** | The finding is confirmed with executed evidence. | `F-XC-005` moves to `E1`; keep **Medium**. |
| `…first_chunk_error…` returns something other than `Abstain` | The error path behaves differently than traced. | Re-open the finding; report what it returned. |
| `…the_shipped_sample…` **fails** | Somebody set a range cap in the sample. | The configuration half is fixed. Update the assertion to the shipped value and re-score. |

**Watch for a false pass**: `Provider::mocked` reports chain id `0x5afe`, and the transaction in
the test carries `chain_id = 0x5afe` deliberately. If those ever diverge, the chain-id guard at
`address_poisoning.rs:311-319` abstains before any lookup happens and the test passes for the
wrong reason — the same trap that made `RefundChecker` dead code (`F-ENG-032`).

## What is missing and why

The later-chunk (partial-scan) test is **not** written here. To be decisive it needs an
established recipient gathered from a successful first chunk and a lookalike candidate, which
means hand-encoding `Transfer` logs so that `decode_target_and_amount`
(`address_poisoning.rs:275-285`) accepts them and `is_lookalike` fires. That is a real test worth
having — it pins the `complete: false` contract, which is what stops an incomplete scan from
denying — but it is long enough that writing it unrun and unverified would risk shipping a test
whose fixtures are subtly wrong. Its specification: push one successful `eth_getLogs` returning a
non-zero-value `Transfer` from `safe` to an established address `R`, then
`push_failure_msg` for the second chunk, and assert that a candidate that is a lookalike of `R`
still gets `Abstain` and **not** `Insecure`, because `complete == false`.

## Fixtures (attacker-controlled under A2, so literal)

- Safe `0x0101…01`, token `0x0202…02`, candidate `0x0303…03`.
- Calldata: `transfer(0x0303…03, 1000000)`, `operation = Call`, `chain_id = 0x5afe`.
- Context block `1000000`, so the lookback window is `950000..=1000000`.
- Provider response: one failure, `"query returned more than 10000 results"` — the real shape of
  a capped provider's error.

## Remediation check (QA-XC)

- **Option 1 (ship `address_poisoning_max_block_range = 9999` uncommented in the sample) is sound
  and is the one-line fix.** It is strictly safer than the current default on every provider,
  capped or not: with a cap set, a mid-scan failure keeps the evidence already gathered instead of
  discarding the request. It costs extra RPC calls on an uncapped provider, which is the correct
  trade.
- **Option 3 depends on `F-XC-010`** (a checker-outcome metric), which is the finding that makes
  this degradation visible at all. Fixing this one without that one leaves the operator no way to
  tell whether the fix worked.
- One addition neither option makes: `AddressPoisoningChecker::new` accepts
  `lookback_blocks = 50_000` with `max_block_range = None` silently. A startup `warn!` when the
  lookback exceeds a conservative single-call span would surface the misconfiguration at boot,
  which is where `F-ENG-009` says the engine says nothing at all.
