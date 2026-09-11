# PoC — F-XC-010 (the engine's own behaviour is unmeasured)

**Never compiled, never run.** No Rust toolchain on the audit machine (A9 FALSE). Written
against the checkout at commit `2893917`.

## Why this one gets a PoC when most cross-cutting findings do not

Most of the `F-XC` range is configuration, packaging and documentation, where a test would be
padding — "the Dockerfile runs as root" (`F-XC-004`) is checked by `docker inspect`, not by
`cargo test`. This one earns a test because it is not really an observability wish: it is the
reason two **Confirmed** defects survived into production unnoticed.

- `F-ENG-032`: `RefundChecker` has abstained on *every* transaction since it was written — its
  synthetic sub-transaction carries `chain_id = 0` (`crates/sentinel-engine/src/checkers/refund.rs:105-117`,
  `..Default::default`), which `AddressPoisoningChecker`'s chain-id guard
  (`crates/sentinel-engine/src/checkers/address_poisoning.rs:311-319`) rejects every time. One
  counter with a `checker` label would have shown a checker with zero non-abstain outcomes on
  its first day.
- `F-XC-005`: the shipped sample's 50,000-block single-call lookback turns the address-poisoning
  check into an `Abstain` on any provider error, including the rate limiting A4 puts in scope.

The test therefore does not assert "a metric is missing". It asserts the operationally
meaningful thing: **a checker that failed and a checker that had no opinion produce byte-identical
scrapes.**

## Install and run

Append `append-to-crates-sentinel-engine-src-engine-mod.rs` inside the existing `mod tests` block
at the end of `crates/sentinel-engine/src/engine/mod.rs`. It reuses that module's `StubChecker`.

```sh
cargo test -p sentinel-engine qa_xc_010 -- --nocapture --test-threads=1
```

`--test-threads=1` is required. `safenet_core::observability::metrics::serve` installs a
process-global recorder and succeeds only once per process
(`crates/core/src/observability/metrics.rs:13-15`). Nothing else in `sentinel-engine` installs
one today — `grep -rn 'metrics' crates/sentinel-engine/src/` returns five hits, all
`config.rs` field references, none a recording call — so the single call is safe.

## Reading the result

| Outcome | Meaning | Action |
| --- | --- | --- |
| **Passes** | Confirms the finding as filed: the endpoint answers 200, the boot log says "serving prometheus metrics and health endpoint" (`crates/core/src/observability/mod.rs:61`), and the body contains nothing about checkers, verdicts or failures. | `F-XC-010` moves from `E2` to `E1`; raise certainty to the top of the band this run allows. Save the `--nocapture` scrape here as the artefact — the printed body is the evidence, and it is short. |
| `after_failure != after_abstention` | A checker-outcome metric now exists. | `F-XC-010` is fixed; close it and delete the test. |
| A `needle` assertion fires | Some series mentioning checkers/verdicts appeared. | Same as above — read the printed body and close the finding against it. |
| `serve(...)` panics with a `BuildError` | Another test in the same binary installed the recorder first. | Run with `--test-threads=1`, or run this test alone: `cargo test -p sentinel-engine qa_xc_010_a_failing_checker`. |

## Fixtures

No attacker input, no chain state, no network beyond loopback. `SafeTransaction::default` and
`CheckContext::default` are sufficient because the assertion is about what the *process*
records, not about what any checker decides; both stubs are local to the test.

## Remediation check (QA-XC)

`F-XC-010` lists four options. Assessment:

1. **Option 1 (a `checker_outcomes_total` counter labelled by `Checker::name` and outcome) is
   sound, and it is the one to take.** `Checker::name(&self) -> &'static str`
   (`crates/sentinel-engine/src/checkers/mod.rs:26-27`) is a closed set of `&'static str`, which
   matches the label convention every metric in the three existing metrics modules already
   follows, so there is no cardinality risk. The finding's own stated tradeoff is the real
   design question and it is stated correctly: `Checker::check` returns `Verdict`, which has no
   error variant (`engine/mod.rs:37-48`), so distinguishing `error` from `abstain` needs either
   a richer return type or per-checker recording at the point the error is swallowed. **The
   second is right for now** — it is strictly local, it touches only the `Err` arms that already
   `warn!`, and it does not change a public trait that four checkers implement.
2. **Option 2 (outbound dependency counters) is sound but secondary.** `core` already counts
   RPC requests globally (`crates/core/src/metrics.rs:26-35`); the value added is attribution.
3. **Option 3 (raise the two verdict lines from `trace!` to `debug!`) is sound as a stopgap and
   should not be mistaken for the fix.** It removes the situation where the default
   configuration emits nothing, but a per-request log line is not an alertable series — which is
   the finding's actual complaint, and the reason the two sibling services have metrics modules
   rather than relying on their own `warn!` lines.
4. **Option 4 (document it in `docs/sentinel-engine.md`'s configuration table) is sound and
   free.** The table currently describes `observability.metrics_address` as a "Prometheus
   listener" without saying the reference engine publishes no engine-specific metrics to it,
   which is precisely the misleading impression this finding is about.

None of the four touches `core::state`'s documented contracts (purity, at-least-once effects,
undefined resume ordering) or any Solidity reference under A7. The finding's own "tests to add"
line says none is meaningful; I disagree mildly, and this PoC is the counter-example — the
*absence* is testable even where the presence would not be worth a unit test.
