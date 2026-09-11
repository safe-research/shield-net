// PoC for F-XC-005 — the shipped engine sample pairs a 50,000-block
// single-call lookback with no range cap, so any provider error on the one
// `eth_getLogs` call disables the address-poisoning check.
//
// NOT COMPILED, NOT RUN — no Rust toolchain on the audit machine.
//
// Append inside the existing `mod tests` block at the end of
// `crates/sentinel-engine/src/checkers/address_poisoning.rs` (before its
// closing brace).
//
// Run:  cargo test -p sentinel-engine qa_xc_005 -- --nocapture
//
// `Provider::mocked` comes from `safenet-core`'s `test-util` feature, which
// `crates/sentinel-engine/Cargo.toml:24` already enables as a dev-dependency.
// `Asserter::push_failure_msg` is used the same way elsewhere in the workspace
// (e.g. crates/core/src/index/events.rs:1191, "range too large").

#[tokio::test]
async fn qa_xc_005_a_first_chunk_error_disables_the_check_entirely() {
    use alloy::{sol_types::SolCall as _, transports::mock::Asserter};
    use safenet_core::provider::Provider;

    const SAFE: Address = Address::new([1u8; 20]);
    const TOKEN: Address = Address::new([2u8; 20]);
    const CANDIDATE: Address = Address::new([3u8; 20]);

    let asserter = Asserter::new();
    // `Provider::mocked` reports chain id 0x5afe, so the transaction below
    // must carry the same value or the chain-id guard at
    // address_poisoning.rs:311-319 abstains before any lookup happens —
    // which would make this test pass for the wrong reason.
    let provider = Provider::mocked(&asserter);

    // EXACTLY the shipped sample's pair
    // (crates/sentinel-engine/sentinel-engine.sample.toml): a 50,000-block
    // lookback with `address_poisoning_max_block_range` unset, so
    // `block_chunks` yields ONE chunk covering the whole window.
    let checker = AddressPoisoningChecker::new(provider, 50_000, None);

    // The single `eth_getLogs` fails. The message is the real one a capped
    // provider returns, and A4 puts a rate-limited or incomplete RPC
    // explicitly in scope.
    asserter.push_failure_msg("query returned more than 10000 results");

    let transaction = SafeTransaction {
        chain_id: U256::from(0x5afe_u64),
        safe: SAFE,
        to: TOKEN,
        data: transferCall {
            to: CANDIDATE,
            amount: U256::from(1_000_000u64),
        }
        .abi_encode()
        .into(),
        operation: Operation::Call,
        ..Default::default()
    };

    let verdict = checker
        .check(&transaction, &CheckContext { block: 1_000_000 })
        .await;

    // The check is not degraded, it is absent: no partial scan is possible
    // because there is only one chunk, and `established_recipients` returns
    // `Err` rather than an incomplete result (the `Err(err) if
    // !recipients.is_empty()` arm at address_poisoning.rs:201-211 cannot fire
    // when nothing was gathered).
    assert_eq!(
        verdict,
        Verdict::Abstain,
        "with the shipped sample's configuration, one provider error is a hard \
         abstention with no partial-scan fallback"
    );

    // And under F-XC-010 nothing records that this happened: the engine
    // exports no checker-outcome metric, so the only evidence is one `warn!`
    // line per request.
    assert!(asserter.read_q().is_empty(), "the single queued response was consumed");
}

#[test]
fn qa_xc_005_the_shipped_sample_leaves_the_range_cap_unset() {
    // Pins the configuration half of the finding, so a fix to the sample is
    // visible here rather than only in a diff of a TOML file nobody tests.
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("sentinel-engine.sample.toml");
    let contents = std::fs::read_to_string(path).unwrap();
    let config = toml::from_str::<crate::config::Config>(&contents).unwrap();

    assert_eq!(config.engine.address_poisoning_lookback_blocks, 50_000);
    assert!(
        config.engine.address_poisoning_max_block_range.is_none(),
        "the sample now sets a range cap — F-XC-005's configuration half is fixed; \
         update this assertion to the shipped value"
    );
}
