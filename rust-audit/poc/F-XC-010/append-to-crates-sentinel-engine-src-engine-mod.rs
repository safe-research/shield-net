// PoC for F-XC-010 — the engine serves a Prometheus endpoint that says nothing
// about checkers, verdicts, or checker failures.
//
// NOT COMPILED, NOT RUN — no Rust toolchain on the audit machine.
//
// Append this test inside the existing `mod tests` block at the end of
// `crates/sentinel-engine/src/engine/mod.rs` (before its closing brace). It
// reuses that module's own `StubChecker` (crates/sentinel-engine/src/engine/mod.rs:76-88).
//
// Run:  cargo test -p sentinel-engine qa_xc_010 -- --nocapture --test-threads=1
//
// `--test-threads=1` matters: `observability::metrics::serve` installs a
// PROCESS-GLOBAL Prometheus recorder and can only succeed once per process
// (crates/core/src/observability/metrics.rs:13-15). Nothing else in this crate
// installs it today (`grep -rn 'metrics' crates/sentinel-engine/src/` returns
// only `config.rs` field references), so the single call below is safe, but a
// future test that also installs one will collide.

/// A checker that abstains because its lookup FAILED, as opposed to one that
/// abstains because it examined the transaction and had no opinion. Both are
/// `Verdict::Abstain` — `Verdict` has no error variant
/// (crates/sentinel-engine/src/engine/mod.rs:37-48) — and this test shows the
/// process emits nothing that tells them apart.
struct FailingChecker;

#[async_trait::async_trait]
impl Checker for FailingChecker {
    fn name(&self) -> &'static str {
        "failing"
    }

    async fn check(&self, _: &SafeTransaction, _: &CheckContext) -> Verdict {
        // The shape of every RPC/CoW error path in the real checkers: the
        // error is swallowed into an abstention, e.g.
        // crates/sentinel-engine/src/checkers/address_poisoning.rs (the
        // `Err(err) => { tracing::warn!(...); Verdict::Abstain }` arm).
        Verdict::Abstain
    }
}

#[tokio::test]
async fn qa_xc_010_a_failing_checker_and_a_quiet_one_scrape_identically() {
    // The listener the engine actually starts at boot
    // (crates/sentinel-engine/src/main.rs:45 -> observability::init ->
    // metrics::serve). Port 0 picks an ephemeral loopback port, which is also
    // the shipped default (crates/core/src/observability/mod.rs:31-33).
    let addr = safenet_core::observability::metrics::serve((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("the Prometheus recorder installs once per process");

    async fn scrape(addr: std::net::SocketAddr) -> String {
        reqwest::get(format!("http://{addr}/metrics"))
            .await
            .expect("the metrics listener is up")
            .text()
            .await
            .expect("a Prometheus text body")
    }

    // 1. A checker whose lookup failed. Under F-XC-005 this is the shipped
    //    sample's steady state for the address-poisoning check on any capped
    //    or rate-limited provider (A4).
    let broken = SentinelEngine::new(vec![Box::new(FailingChecker)]);
    let broken_verdict = broken
        .security_check(SafeTransaction::default(), CheckContext::default())
        .await;
    let after_failure = scrape(addr).await;

    // 2. A checker that ran fine and had no opinion.
    let quiet = SentinelEngine::new(vec![Box::new(StubChecker(Verdict::Abstain))]);
    let quiet_verdict = quiet
        .security_check(SafeTransaction::default(), CheckContext::default())
        .await;
    let after_abstention = scrape(addr).await;

    println!("--- scrape after a FAILED check ---\n{after_failure}\n--- end ---");

    // The two are indistinguishable to the caller...
    assert_eq!(broken_verdict, Verdict::Abstain);
    assert_eq!(quiet_verdict, Verdict::Abstain);
    // ...and indistinguishable to the operator. `docs/sentinel-engine.md:25`
    // is explicit that they must not be conflated: "An `abstain` response is
    // successful and deliberate. It must not be interpreted as either a secure
    // or insecure transaction."
    assert_eq!(
        after_failure, after_abstention,
        "if these now differ, the engine has gained a checker-outcome metric \
         and F-XC-010 should be closed"
    );

    // And nothing in the body describes the checker chain at all, even though
    // the boot log claims to be "serving prometheus metrics"
    // (crates/core/src/observability/mod.rs:61).
    let lowered = after_failure.to_ascii_lowercase();
    for needle in [
        "checker",
        "verdict",
        "abstain",
        "insecure",
        "security_check",
        "sentinel_engine",
    ] {
        assert!(
            !lowered.contains(needle),
            "expected the engine to export no `{needle}` series; if this fires, \
             F-XC-010 has been fixed and should be closed. Body:\n{after_failure}"
        );
    }
}
