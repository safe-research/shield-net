# F-ENG-032 `RefundChecker` is dead: its synthetic refund transfer carries `chainId = 0`, so the delegated address-poisoning check always abstains

| Field | Value |
| --- | --- |
| Status | Verified |
| Crate and module | sentinel-engine, checkers/refund.rs |
| Location | crates/sentinel-engine/src/checkers/refund.rs:105-117 (related: checkers/address_poisoning.rs:312-319, engine/transaction.rs:55-76) |
| Severity | High / Medium |
| Certainty | 99% (V-ENG, Phase 5; E1 — PoC executed) |
| Assumptions involved | A2, A3 |
| Tags | input-validation, verdict-policy, config |

## Claim

`refund_transfer` builds the synthetic ERC-20 `transfer` that models the Safe's gas refund with `..Default::default`. `SafeTransaction` derives `Default`, so the synthetic transaction's `chain_id` is `U256::ZERO`. `AddressPoisoningChecker::check`, to which `RefundChecker` delegates, compares `transaction.chain_id` against the configured provider's cached chain id and abstains on a mismatch. A live provider never reports chain id 0, so the comparison fails on **every** relayed transaction and `RefundChecker` returns `Abstain` unconditionally, before any `eth_getLogs` is issued.

Consequences:

- The engine's only refund-leg guard never denies anything. Combined with F-ENG-031 (the refund leg is unvetted behind every affirmer) this leaves the ERC-20 refund path with no check at all, which is precisely the mitigation PR #876 relied on when it removed the engine-wide "abstain on any non-zero `gasPrice`" gate.
- Every relayed transaction with a non-zero `gasToken` and `refundReceiver` emits a misleading `warn!` line claiming "transaction chain id does not match the configured provider" with `tx_chain_id=0`, which will read to an operator as a misconfigured RPC rather than as this bug.
- The defect is invisible to the unit suite because `refund.rs`'s seven tests exercise only the pure helpers (`refund_transfer`, `deny_or_abstain`) and never the integrated `check` path (`refund.rs:120-206`).

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The synthetic transfer is built with `..Default::default` — no `chain_id` is copied from the real transaction | E2 | crates/sentinel-engine/src/checkers/refund.rs:105-117 | Q1 |
| 2 | `SafeTransaction` derives `Default`, so `chain_id` defaults to `U256::ZERO` | E2 | crates/sentinel-engine/src/engine/transaction.rs:55-60 | Q2 |
| 3 | `AddressPoisoningChecker::check` abstains on a chain-id mismatch, after successfully decoding the target | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:308-319 | Q3 |
| 4 | `RefundChecker::check` delegates straight to that method and passes its verdict through `deny_or_abstain` | E2 | crates/sentinel-engine/src/checkers/refund.rs:52-57 | Q4 |
| 5 | The unit tests cover only the pure helpers, so the integrated path is untested | E2 | crates/sentinel-engine/src/checkers/refund.rs:142-149, :188-205 | Q5 |

**Q1** `crates/sentinel-engine/src/checkers/refund.rs:105-117`

```rust
    Some(SafeTransaction {
        safe: transaction.safe,
        to: transaction.gas_token,
        data: transferCall {
            to: transaction.refund_receiver,
            amount: transaction
                .gas_price
                .saturating_mul(transaction.safe_tx_gas.saturating_add(transaction.base_gas)),
        }
        .abi_encode
        .into,
        ..Default::default()
    })
```

**Q2** `crates/sentinel-engine/src/engine/transaction.rs:55-60`

```rust
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct SafeTransaction {
    /// The chain the transaction is to execute on.
    pub chain_id: U256,
    /// The Safe executing the transaction.
```

**Q3** `crates/sentinel-engine/src/checkers/address_poisoning.rs:308-319`

```rust
    async fn check(&self, transaction: &SafeTransaction, context: &CheckContext) -> Verdict {
        let Some((candidate, kind)) = decode_target(transaction) else {
            return Verdict::Abstain;
        };
        if transaction.chain_id != U256::from(self.provider.chain_id()) {
            tracing::warn!(
                tx_chain_id = %transaction.chain_id,
                provider_chain_id = self.provider.chain_id(),
                "address-poisoning check: transaction chain id does not match the configured provider"
            );
            return Verdict::Abstain;
        }
```

**Q4** `crates/sentinel-engine/src/checkers/refund.rs:52-57`

```rust
    async fn check(&self, transaction: &SafeTransaction, context: &CheckContext) -> Verdict {
        let Some(refund) = refund_transfer(transaction) else {
            return Verdict::Abstain;
        };
        deny_or_abstain(self.0.check(&refund, context).await)
    }
```

**Q5** `crates/sentinel-engine/src/checkers/refund.rs:142-149` and `:188-205`

```rust
    #[test]
    fn none_when_gas_price_is_zero {
        let transaction = SafeTransaction {
            gas_price: U256::ZERO,
            ..relayed_tx()
        };

        assert_eq!(refund_transfer(&transaction), None);
```

```rust
    #[test]
    fn never_lets_a_secure_refund_leg_affirm_the_whole_transaction {
        assert_eq!(deny_or_abstain(Verdict::Secure), Verdict::Abstain);
    }

    #[test]
    fn passes_through_a_denial {
        let denial = Verdict::Insecure {
            rule: RuleId::R4_3ValueTarget,
        };

        assert_eq!(deny_or_abstain(denial), denial);
    }
```

## Trigger

Any relayed transaction with an ERC-20 gas token and a poisoned refund receiver — the exact case the checker exists to deny. Engine configured against a mainnet RPC (`rpc = "https://…"`, provider chain id 1):

```json
{
  "block": "0x1500000",
  "transaction": {
    "chainId": "0x1",
    "safe": "0x5aFE3855358E112B5647B952709E6165e1c1eEEe",
    "to": "0x000000000000000000000000000000000000B0B0",
    "value": "0x0",
    "data": "0x",
    "operation": 0,
    "safeTxGas": "0x186a0",
    "baseGas": "0x5208",
    "gasPrice": "0x1",
    "gasToken": "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    "refundReceiver": "<a 4-leading/4-trailing-nibble lookalike of an address the Safe has paid in that token>",
    "nonce": "0x2a"
  }
}
```

Expected by design: `{"verdict":"insecure","rule":"R-4.3"}`. Actual: `{"verdict":"abstain"}`, with a log line `address-poisoning check: transaction chain id does not match the configured provider` carrying `tx_chain_id=0 provider_chain_id=1`. No `eth_getLogs` call is issued.

Deterministic offline confirmation without a live chain: `safenet_core::provider::Provider::mocked` (chain `0x5afe`, available through the crate's `test-util` dev-dependency) plus a `RefundChecker` over it — the mismatch fires for any non-zero mocked chain id.

Corpus shape: the vector above, plus a control where `refundReceiver` is an address with genuine history (must be `abstain` for a different reason). Both currently return the same bytes on the wire, which is itself why the defect is invisible.

## Considered and rejected

- **Some other field also blocks the path first.** No: `refund_transfer` returns `Some` whenever `gas_price`, `gas_token` and `refund_receiver` are all non-zero (`refund.rs:98-103`), and `decode_target` decodes the synthetic `transfer` successfully because its amount is non-zero (`address_poisoning.rs:122-124`; the amount is `gas_price * (safe_tx_gas + base_gas)`, non-zero for the trigger above). So control reaches the chain-id comparison every time.
- **The provider might report chain id 0.** `Provider::connect` reads `eth_chainId` once and caches it (`crates/core/src/provider/mod.rs`); no live chain uses 0, and a provider returning 0 would break the real `AddressPoisoningChecker` too.
- **`U256`'s `Default` might not be zero.** `Default for U256` is `ZERO` in ruint; the same assumption is relied on throughout this crate, e.g. `multi_send.rs:117-128` sets those fields explicitly to `U256::ZERO`.
- **This is the same finding as F-ENG-031.** No: F-ENG-031 is about _ordering and coverage_ (the refund leg is never consulted by an affirmer, and the native case is out of scope by design). This one is a single-line construction bug that additionally disables the ERC-20 case the checker does claim to cover. Either fix alone leaves the other hole open.
- **Copying `chain_id` alone fixes it.** It restores the lookup, but the synthetic transaction still carries `nonce = 0` and `value = 0`; only `chain_id` is load-bearing for the guard, so that part is sufficient — the remediation notes both.

## Remediation options

1. Copy `chain_id` (and, for log-field accuracy, `nonce`) from the real transaction into the synthetic one: replace `..Default::default` with explicit fields including `chain_id: transaction.chain_id`. Smallest fix.
2. Restructure so the refund leg is checked by a method that takes `(token, recipient, chain_id, block)` directly rather than by re-synthesising a whole `SafeTransaction` — the resynthesis is what let a defaulted field silently disable the guard, and it will happen again with the next field `AddressPoisoningChecker` starts reading.
3. Add a debug assertion / type-level guard that a synthesised `SafeTransaction` never has `chain_id == 0`.

Tests to add: an integration test of `RefundChecker::check` over `Provider::mocked` asserting a denial for a lookalike refund receiver (this is the test whose absence hid the bug — `refund.rs`'s current suite never calls `check`); a corpus vector for a poisoned ERC-20 refund receiver. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 93%. (Confirms lead ENG-H1; all four cited locations re-read in this checkout at commit 2893917. Not executed — A9 is FALSE this run.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts — all four Supported

I verified the `chainId = 0` claim precisely, as instructed, because F-ENG-031 also leans on it.

| # | Verdict | Counter-check |
| --- | --- | --- |
| 1 | **Supported** | `refund.rs:105-117` constructs `SafeTransaction { safe, to: transaction.gas_token, data: transferCall{…}.abi_encode.into, ..Default::default }`. `chain_id` is not among the three fields set, so it comes from `Default`. |
| 2 | **Supported** | `engine/transaction.rs:55-59`: `#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)] pub struct SafeTransaction { pub chain_id: U256, …`. `U256::default` is `U256::ZERO`. |
| 3 | **Supported** | `address_poisoning.rs:312-319`: `if transaction.chain_id != U256::from(self.provider.chain_id) { … return Verdict::Abstain; }`. |
| 4 | **Supported** | `refund.rs:52-57` calls `self.0.check(&refund, context).await` and pipes it through `deny_or_abstain`. |

The missing link R9 did not cite, which I checked because the claim stands or falls on it: `provider.chain_id` is not a configurable constant that could plausibly be zero. `crates/core/src/provider/mod.rs:135-136` caches it from the live endpoint at connect time — `let chain_id = root.get_chain_id.await?;` — and no EIP-155 chain uses id 0. So `U256::ZERO != U256::from(chain_id)` holds on every real deployment and the delegated check abstains unconditionally. **The claim is correct: `RefundChecker` has never returned anything but `Abstain` in production.**

Two ordering details I checked and which do not change the outcome: `decode_target` runs _before_ the chain-id test (`address_poisoning.rs:309-311`), so on the synthetic transfer it succeeds first (`Operation::default` is `Call`, `engine/transaction.rs:9-13`, and the synthesised calldata is a well-formed `transfer` with a non-zero amount whenever `gas_price * (safe_tx_gas + base_gas) != 0`); and when that product _is_ zero the checker abstains one line earlier at `:310` instead. Either way: `Abstain`, and no `eth_getLogs` is ever issued.

### 2. Severity — corrected from High to Medium, with reasoning

R9 rates this High. I disagree and set **Medium**, for two reasons that only became clear after tracing the consequence rather than the defect:

1. **The failure direction is fail-closed.** A dead deny-only checker can only turn a would-be `Insecure` into an `Abstain`. `Abstain` becomes `CheckOutcome::Unknown` (`crates/sentinel/src/engine.rs:175`) and the sentinel _drops the request unvoted_ (`crates/sentinel/src/service.rs:176-179`), so no attestation is produced and the Guard refuses the transaction. Nothing is approved that would otherwise have been denied.
2. **The reachable class is narrow.** Because `RefundChecker` sits 9th (`main.rs:71`), it only ever gets a turn on transactions all eight earlier checkers abstained on. Its unique contribution is therefore: an ERC-20 gas-token refund whose `refundReceiver` is a 4+4-nibble lookalike of an address the Safe has previously transacted with _on that same gas token_. That is a real but small set.

PROMPT.md §8's High bullet requires a liveness loss, an unbounded drain, engine DoS, or wrong votes at scale; none applies. §8's Medium — "missing validation with contained impact" — is the right row. The High/Critical weight of this area belongs to **F-ENG-031**, which is where the actual `secure`-rated drain lives; this file is the mechanism, and F-ENG-031's §3 (my note there) explains why fixing this one alone would not fix that one, since `StakingChecker` at position 8 affirms before position 9 is reached.

I do agree with all three of R9's stated consequences, including the misleading `warn!` (`address_poisoning.rs: 313-317` prints `tx_chain_id=0` and reads as an RPC misconfiguration) and the observation that the file's seven tests exercise only the pure helpers and never `check` (`refund.rs:120-206`) — which is exactly how a bug this simple survived. Per AGENTS.md:113 checkers are deliberately covered by the external corpus rather than unit tests, and under A8 that corpus is unavailable, so nothing in this repository could have caught it.

### 3. A hazard for whoever fixes this — new, not in R9's file

The one-line fix is `chain_id: transaction.chain_id` in `refund_transfer`. Before landing it, note that the **same field-zeroing pattern exists a second time**, in `decode_multi_send` at `crates/sentinel-engine/src/contracts/multi_send.rs:116-129`, which stamps every decoded sub-transaction with `chain_id: U256::ZERO`, `base_gas: U256::ZERO`, `gas_price: U256::ZERO`, `gas_token: Address::ZERO`, `refund_receiver: Address::ZERO`, `nonce: U256::ZERO`. That is currently **latent** — I checked every consumer of `sub_transactions`: `StakingChecker` tests `chain_id` on the outer transaction only (`staking.rs:89`), `CowChecker` likewise and passes the outer `chain_id` explicitly into `check_presignature_batch` (`cow.rs:422-437`), `BaseChecker::check_multi_send` reads no such field, and `AddressPoisoningChecker` is never given sub-calls at all. So it is below the bar for a finding of its own. But the natural remediation for F-ENG-035 (make the blocklist see sub-call destinations) and for F-ENG-031 (vet the refund leg) both involve running an RPC-backed check over a synthesised or decoded transaction, and both would walk straight into this identical dead-check bug. A guard that rejects a synthesised `SafeTransaction` with `chain_id == 0`, or a constructor that requires it, would prevent the third occurrence.

### 4. Finding verdict

**Confirmed.** Mechanism and trigger both verified purely from code, with no dependency-behaviour inference anywhere in the chain — this is the most securely established finding in R9's set. Certainty **88%**, just under the read-only ceiling of 89 imposed by the brief (`E1` unreachable). Severity **Medium** (corrected from High).

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection.** The path is four steps and I walked all four: `refund.rs:105-117` builds the synthetic transfer with `..Default::default`; `SafeTransaction` derives `Default` (`engine/transaction.rs:55`) so `chain_id` is `U256::ZERO`; `RefundChecker::check` passes it straight to `AddressPoisoningChecker::check` (`refund.rs:56`); and that compares `transaction.chain_id != U256::from(self.provider.chain_id)` and returns `Abstain` (`address_poisoning.rs:312-319`) before `established_recipients` is called. `Provider::chain_id` is read once at `connect` and cached (`crates/core/src/provider/mod.rs:129-133`), and no live chain reports 0. Not `E1`; the 90-100 band stays closed.

**Certainty unchanged at 88%. Severity unchanged (High reviewer / Medium final).**

**PoC: `rust-audit/poc/F-ENG-032/`** — four tests appended to `refund.rs` as a sibling of its existing suite, over `Provider::mocked` + an `alloy` `Asserter`. Hermetic; no network.

Three things the PoC establishes that the finding's own prose does not:

1. **It proves no RPC is issued**, rather than asserting it. The first test leaves the `Asserter` queue **empty**; `alloy`'s mock transport panics on a request with nothing queued, so merely reaching the assertion is the proof.
2. **It supplies the discriminator.** Today a working `RefundChecker` and a dead one emit identical bytes on the wire (`abstain` either way), which is why the defect survived. The only observable that separates them is "was the lookup issued?", and only an in-process mock can see it.
3. **The lookalike pair sits exactly on `is_lookalike`'s thresholds** (`prefix >= 4 && suffix >= 4`, `address_poisoning.rs:78-91`): `0xAbCd1111…1111112345` established, `0xAbCd9999…9999992345` candidate. A pair chosen casually will not trip the branch and the test will pass for the wrong reason.

**One unresolved dependency question**, filed as **Q-ENG-A** in `rust-audit/poc/UNRESOLVED-DEPENDENCY-QUESTIONS-ENG.md`: whether `alloy::transports::mock::Asserter` exposes a queue-emptiness accessor. It is the only identifier in the ten engine PoCs that could not be checked against a checkout (A6 — no dependency sources on disk). The PoC says inline which three lines to delete if it does not exist, and nothing is lost, because item 1 above establishes the same fact more robustly.

### Remediation check

- **Option 1 (copy `chain_id`, and `nonce` for log accuracy) — sound, and the smallest correct fix.** I confirmed that only `chain_id` is load-bearing: `decode_target` reads `operation` and `data` (`address_poisoning.rs:116-139`); `established_recipients` reads `to`, `safe` and `context.block` (`:182-192`). So `value` and `nonce` genuinely do not matter to the verdict, exactly as the finding's "Considered and rejected" says.
- **Option 2 (pass `(token, recipient, chain_id, block)` directly instead of re-synthesising a whole `SafeTransaction`) — sound, and the better fix.** The resynthesis _is_ the mechanism: a defaulted field silently disabled a guard, and it recurs the moment `AddressPoisoningChecker` reads a field the synthesiser does not set. Extracting the lookup into a method that takes exactly its inputs makes the failure unrepresentable rather than merely repaired.
- **Option 3 (a debug assertion that a synthesised transaction never has `chain_id == 0`) — weak, and misleading alone.** `debug_assert!` is compiled out of the release binary that actually runs, so it protects the test suite only; and it encodes the accident (chain id zero) rather than the requirement (the synthetic transaction must carry every field its consumer reads). Acceptable only alongside option 2.
- **The fix does not make the checker effective, and the report must say so.** Fully repaired, `RefundChecker` still runs 9th and cannot deny a transaction an earlier affirmer already approved (F-ENG-031, F-ENG-044). Fixing F-ENG-032 alone converts a dead checker into a live checker that remains unreachable for every transaction that matters — and if F-ENG-031 option 1 (the `gas_price != 0` pre-gate) ships first, `RefundChecker` becomes unreachable outright. Sequence the two together.
- **Test hook: it already existed and was not used, and the reason is a policy the report should challenge.** `Provider::mocked` has been available since `crates/core/src/provider/mod.rs:137-149`. `AGENTS.md`'s "checkers get no unit tests, the corpus is the oracle" is what left `refund.rs`'s suite testing only `refund_transfer` and `deny_or_abstain` while `check` — the one function with the bug — was out of scope by policy. **This finding is the strongest argument in the engine set for amending that rule**, because the corpus _cannot_ catch it: a dead checker and a working one that abstains produce byte-identical responses.
- **Where the fix belongs: the checker** (`refund.rs`), with the ordering half in the combinator. Not the `RuleId` mapping.

## Verification (V-ENG, Phase 5)

**Reproduced by execution, and `Q-ENG-A` settled. Basis class E1.** Certainty 88% -> **99%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under `rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only executable oracle for this checker.

### Q-ENG-A settled: `Asserter::is_empty` does not exist

QA-ENG flagged one unverifiable identifier. Dependency sources are readable at `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/alloy-transport-2.0.5/src/mock.rs:44-89`. `Asserter`'s complete inherent API in the version this workspace resolves (`alloy-transport 2.0.5`) is:

```
new, push, push_success, push_failure, push_failure_msg, pop_response, read_q, write_q
```

There is **no `is_empty`**. QA-ENG's caution was correct. Two runs were therefore made:

- **Variant A** — the three lines deleted exactly as QA-ENG instructed (`rust-audit/poc/F-ENG-032/run-output.txt`, source `ran-source-checkers-refund.rs`).
- **Variant B** — the same assertion expressed through the API that _does_ exist, `asserter.read_q.is_empty`. This is a mechanical repair of a wrong identifier, not a change to what the test asserts (`rust-audit/poc/F-ENG-032/run-output-variant-read_q.txt`, source `ran-source-checkers-refund-variant-read_q.rs`).

Both compiled on the first attempt. **Variant B is the better evidence and is reported as the result.**

### Verbatim result (variant B)

```
running 4 tests
test checkers::refund::poc_f_eng_032::poc_f_eng_032_the_synthetic_transfer_has_chain_id_zero ... ok
test checkers::refund::poc_f_eng_032::poc_f_eng_032_abstains_without_any_rpc_call_today ... ok
test checkers::refund::poc_f_eng_032::poc_f_eng_032_denies_a_poisoned_erc20_refund_receiver ... FAILED
test checkers::refund::poc_f_eng_032::poc_f_eng_032_an_established_refund_receiver_abstains_after_a_real_lookup ... FAILED

---- ..._denies_a_poisoned_erc20_refund_receiver stdout ----
assertion `left == right` failed
  left: Abstain
 right: Insecure { rule: R4_3ValueTarget }

---- ..._an_established_refund_receiver_abstains_after_a_real_lookup stdout ----
the queued eth_getLogs response was never consumed: no lookup was issued

test result: FAILED. 2 passed; 2 failed; 0 ignored; 0 measured; 97 filtered out
```

Both expected-to-fail tests failed for the claimed reason. Under variant A (the assertion deleted) the second test passes vacuously — `Abstain` is the observed _and_ the asserted value — and only one failure appears; that is exactly the indistinguishability the finding describes, and is why variant B is the discriminating run.

### What this establishes

1. **The synthetic refund transfer carries `chain_id = U256::ZERO`** (test 2, passed): a direct assertion on `refund_transfer(&tx).chain_id`, which is `..Default::default` at `refund.rs:116`.
2. **No RPC is ever issued** (test 1, passed): the `Asserter` queue was left empty and `alloy`'s mock transport panics on an unanswered request. Reaching the assertion proves `RefundChecker::check` returned without a single `eth_getLogs`, i.e. it abstained at the chain-id comparison (`address_poisoning.rs:312-319`) before any lookup.
3. **The queued response is never consumed even when one is provided** (test 4 under variant B): a genuine `Transfer` log was queued and remained in the FIFO after `check` returned.
4. **The exact case the checker was written for is missed** (test 3): a refund receiver that is a 4-leading / 4-trailing-nibble lookalike of an established relayer — `is_lookalike`'s exact thresholds — returns `Abstain`, not the `Insecure { rule: R4_3ValueTarget }` the module documents.

**`RefundChecker` has never fired.** On any live provider (chain id != 0) the comparison at `address_poisoning.rs:312-319` fails on every relayed transaction, so the checker is unconditionally inert. The existing test suite in `refund.rs` never calls `check` at all — it only exercises `refund_transfer` and `deny_or_abstain` — which is precisely why this was invisible.

Residual uncertainty is now essentially only A3 (that a production provider never reports chain id 0).

## Real-world validation (Phase 8, RW-ENG)

### Scenario

The claim is that `RefundChecker` is _dead_ — that it abstains before issuing any `eth_getLogs` because the synthetic transfer it builds carries `chain_id = 0`. Phase 5 established this with a mock transport, where "no RPC call was made" is a statement about an asserter. Phase 8 asked the stronger question: against a **real chain and a real RPC**, does the checker issue any `eth_getLogs` at all?

Live deployment: Anvil 1.8.1 on `127.0.0.1:8545` (chain id 31337, confirmed in the engine's own startup log as `result":"0x7a69"`), real Safe 1.5.0 proxy `0x643d887734c637f108B095dc3EE0e06F79bC320C`, real ERC-20 at `0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9`, and the real `sentinel-engine` binary on `127.0.0.1:5473` with `log_filter = "sentinel_engine=trace,safenet_core=trace,info"` so that every JSON-RPC request the provider sends is logged verbatim.

A payload was constructed so that `RefundChecker` is the **first checker that can reach a verdict** — checkers 1–8 all abstain — with a full ERC-20 refund leg for it to resynthesize.

### Verbatim outcome — the checker sends nothing

Request: `to` = the real ERC-20, `data` = `transfer(0x…DeaDBeef, 1)`, `gasToken` = the same ERC-20, `gasPrice` = 1e12, `safeTxGas` = `baseGas` = 5e5, `refundReceiver` = the attacker EOA.

```
{"verdict":"abstain"}
```

Every checker ran:

```
cancellation -> Abstain     escape_hatch -> Abstain    base -> Abstain
blocklist -> Abstain        nested_safe -> Abstain     excessive_approval -> Abstain
cow -> Abstain              staking -> Abstain         refund -> Abstain
address_poisoning -> Abstain  (final) -> Abstain
```

The engine emitted exactly one warning, and it is the misleading one this finding predicts:

```
{"message": "address-poisoning check: transaction chain id does not match the configured provider",
 "tx_chain_id": "0", "provider_chain_id": 31337}
```

And **exactly one** `eth_getLogs` was sent for the whole request — the `AddressPoisoningChecker`'s own primary-transfer lookup, not the refund one (note `address` is the transaction's `to`, and the fromBlock/toBlock span the live chain):

```json
{
  "method": "eth_getLogs",
  "params": [
    {
      "fromBlock": "0x0",
      "toBlock": "0xc",
      "address": "0xdc64a140aa3e981100a9beca4e685f962f0cf6c9",
      "topics": [
        ["0x8c5be1e5…c3b925", "0xddf252ad…23b3ef"],
        "0x000000000000000000000000643d887734c637f108b095dc3ee0e06f79bc320c"
      ]
    }
  ],
  "id": 1,
  "jsonrpc": "2.0"
}
```

`RefundChecker` issued **zero** RPC calls against a real, reachable, correctly-configured provider.

### Verbatim outcome — it cannot deny even the case it exists for

A sharper test than "it sends nothing": give the checker the exact poisoning it is supposed to catch, and show the same machinery catches it on the primary leg and misses it on the refund leg.

Real history was established on chain — `Transfer(from = safe, to = 0xAAAA1111…1BBBB1, 5)` — and then two requests were sent that differ **only** in which leg carries the lookalike `0xAAAA2222…2BBBB1` (4 shared leading nibbles, 4 shared trailing):

```
CONTROL  primary-leg transfer to the lookalike       -> {"verdict":"insecure","rule":"R-4.3"}
REFUND   identical lookalike, on the refund leg      -> {"verdict":"abstain"}
```

The refund request's trace, with the misleading warn in the middle:

```
… cow -> Abstain    staking -> Abstain
WARN  address-poisoning check: transaction chain id does not match the configured provider   tx_chain_id=0
refund -> Abstain
address_poisoning -> Abstain
```

The lookalike-detection machinery demonstrably works against real chain history — it denies `R-4.3` when the same address appears on the primary leg. The refund delegation is the only thing that fails, and it fails silently, blaming the operator's RPC.

### Verdict

**Reproduced end-to-end**, and strengthened. Phase 5 proved the checker never fires; Phase 8 proves it never fires _against a live chain_ and, additionally, that a poisoned `refundReceiver` the engine would otherwise deny passes unremarked. The consequence is no longer inferred from code reading: the same address, same token, same block, is `insecure R-4.3` on one leg and `abstain` on the other.

The operator-facing signal is confirmed as actively misleading: on a correctly configured engine against a correctly configured node, every relayed transaction with a nonzero `gasToken` produces a warning that reads as an RPC misconfiguration (`tx_chain_id: "0"`).

Certainty **99%**, unchanged (it was already at the ceiling). Severity **High / Medium**, unchanged — the drain impact stays attributed to F-ENG-031; what moved here is that the missed _denial_ is now demonstrated rather than argued.
