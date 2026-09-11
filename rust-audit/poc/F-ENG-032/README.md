# PoC — F-ENG-032 (Medium, 88%)

**`RefundChecker` is dead: its synthetic refund transfer carries `chainId = 0`, so the delegated address-poisoning check always abstains.**

> **Never compiled.** No Rust toolchain on the audit host (`rust-audit/state/baseline.md` §1). One identifier — `Asserter::is_empty` — could not be verified at all, because no dependency sources are on disk (assumption A6); the PoC says so inline and tells you which three lines to delete if it is absent.

## Why this finding gets a PoC despite being the lowest-severity of the five

**A checker that has never fired in production is worth a regression test of its own.** `RefundChecker` has been in the chain since it was written, has never issued an `eth_getLogs` call, and has never returned anything but `Abstain` — and nothing detected that, because its existing unit suite (`refund.rs:120-206`) tests `refund_transfer` and `deny_or_abstain` in isolation and **never calls `check`**. Every test passes; the checker does nothing. That is the shape of defect a corpus of request/response vectors cannot catch either, since the wire output is `abstain` both when the checker works and when it is dead.

## Apply and run

```bash
cat rust-audit/poc/F-ENG-032/append-to-src-checkers-refund.rs \
  >> crates/sentinel-engine/src/checkers/refund.rs
cargo test -p sentinel-engine poc_f_eng_032
git checkout -- crates/sentinel-engine/src/checkers/refund.rs
```

Hermetic: `Provider::mocked` (`crates/core/src/provider/mod.rs:139`, behind `safenet-core`'s `test-util` feature, already a dev-dependency of this crate) plus an `alloy` `Asserter`. No network, no Anvil.

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._abstains_without_any_rpc_call_today` | **passes** | fails | Pins the defect **and** proves no RPC was issued: the `Asserter` queue is left empty and `alloy`'s mock transport panics on a request with nothing queued, so merely reaching the assertion is the proof. |
| `..._the_synthetic_transfer_has_chain_id_zero` | **passes** | fails | The root cause in one line: `..Default::default` at `refund.rs:116`. |
| `..._denies_a_poisoned_erc20_refund_receiver` | **fails** | passes | The finding. `left: Abstain`, `right: Insecure { rule: R4_3ValueTarget }`. |
| `..._an_established_refund_receiver_abstains_after_a_real_lookup` | **fails** on the queue assertion (the verdict assertion passes) | passes | The discriminator. Today (3) and (4) return the same bytes on the wire; only "was the lookup issued?" separates them. |

## The fixture, written out

Engine configured against a mocked provider on chain **`0x5afe` (23294)** — `Provider::mocked`'s default — with `address_poisoning_lookback_blocks = 50000` and `address_poisoning_max_block_range = None` (so `block_chunks` issues the whole window as **one** `eth_getLogs` call, which is why exactly one queued response suffices).

Request `block`: `22020096`.

| Field | Value | Why |
| --- | --- | --- |
| `chainId` | `0x5afe` (23294) | **Must equal the provider's chain id**, or the _real_ transaction's own check would abstain for an unrelated reason and the test would prove nothing. |
| `safe` | `0x5aFE3855358E112B5647B952709E6165e1c1eEEe` | The victim. |
| `to` | `0x000000000000000000000000000000000000B0B0` | Deliberately inert: an unrelated address with empty calldata, so nothing but the refund leg is under test. |
| `value` | `0x0` | — |
| `data` | `0x` | — |
| `operation` | `0` (`Call`) | — |
| `safeTxGas` | `0x186a0` (100000) | Feeds the synthetic amount. |
| `baseGas` | `0x5208` (21000) | Feeds the synthetic amount. |
| `gasPrice` | `0x1` | **Must be non-zero**, or `refund_transfer` returns `None` (`refund.rs:98`). |
| `gasToken` | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` (USDC) | **Must be non-zero**, or `refund_transfer` returns `None`. This is the ERC-20 path — the one the two TODOs do _not_ cover. |
| `refundReceiver` | `0xAbCd999999999999999999999999999999992345` | **Must be non-zero**, or `refund_transfer` returns `None`. The poisoned lookalike. |
| `nonce` | `0x2a` | — |

**The lookalike pair, chosen to sit exactly on `is_lookalike`'s thresholds** (`address_poisoning.rs:78-91`, `prefix >= 4 && suffix >= 4`):

- established: `0xAbCd111111111111111111111111111111112345`
- candidate: `0xAbCd999999999999999999999999999999992345`
- shared leading nibbles: `A b C d` = **4**; shared trailing: `2 3 4 5` = **4**; middle differs.

**The seeded evidence:** one `Transfer` log, `address = USDC`, `from = safe`, `to = 0xAbCd…2345` (the _established_ relayer), `amount = 1000`, at block `22020095`. The amount must be non-zero — zero-value events are excluded from the evidence pool (`address_poisoning.rs:215-217`).

**The synthetic transfer `refund_transfer` builds** (`refund.rs:105-117`): `safe = safe`, `to = gasToken` (USDC), `data = transfer(refundReceiver, gasPrice × (safeTxGas + baseGas))` = `transfer(0xAbCd…2345, 121000)`, **and every other field defaulted — including `chain_id = 0`.**

`AddressPoisoningChecker::check` then compares `0` against the provider's `23294`, logs `address-poisoning check: transaction chain id does not match the configured provider` with `tx_chain_id=0 provider_chain_id=23294`, and returns `Abstain` (`address_poisoning.rs:312-319`) before `established_recipients` is ever called.

**Expected by design:** `{"verdict":"insecure","rule":"R-4.3"}`. **Actual:** `{"verdict":"abstain"}`.

## Remediation check (QA-ENG)

- **Option 1 (copy `chain_id`, and `nonce` for log accuracy) — sound, and it is the smallest correct fix.** Replacing `..Default::default` with explicit fields including `chain_id: transaction.chain_id` restores the lookup. `nonce` and `value` are genuinely not load-bearing: `decode_target` reads only `operation` and `data`, and `established_recipients` reads only `to`, `safe` and `context.block`. Ship it, but do not stop there — see the ordering point below.
- **Option 2 (pass `(token, recipient, chain_id, block)` directly instead of re-synthesising a whole `SafeTransaction`) — sound, and the better fix.** The resynthesis is the mechanism: a defaulted field silently disabled a guard, and it will recur the moment `AddressPoisoningChecker` starts reading a field the synthesiser doesn't set. Extracting the lookup into a method taking exactly its inputs makes the failure impossible rather than merely fixed. Cost: a small refactor of `AddressPoisoningChecker::check` into a decode step and a lookup step.
- **Option 3 (a debug assertion that a synthesised `SafeTransaction` never has `chain_id == 0`) — weak, and misleading if taken alone.** `debug_assert!` is compiled out in the release binary that actually runs, so it protects the test suite and nothing else; and it encodes the accident (chain id zero) rather than the requirement (the synthetic transaction must carry the fields its consumer reads). Acceptable only as a belt alongside option 2, never instead of it.
- **The fix does not make the checker effective.** Even fully repaired, `RefundChecker` runs 9th and cannot deny a transaction an earlier affirmer already approved (F-ENG-031, F-ENG-044). Fixing F-ENG-032 alone converts a dead checker into a live checker that is still unreachable for every transaction that matters. Sequence the two fixes together, or the repair will be measured as having changed nothing in production.
- **Missing test hook — the hook exists and was simply not used.** `Provider::mocked` has been available since `crates/core/src/provider/mod.rs:137-149`. The rule that hid this is `AGENTS.md`'s "checkers get no unit tests, the corpus is the oracle": `refund.rs` complied by testing only its pure helpers, and the one thing that needed a test — `check` — was out of scope by policy. This finding is the strongest argument in the engine set for amending that rule, because the corpus **cannot** catch it: a dead checker and a working one that abstains emit identical HTTP responses. The distinguishing observation is "was an RPC call issued?", which only an in-process mock can make.
- **Where the fix belongs: the checker** (`refund.rs`), with the ordering half in the combinator. Not the `RuleId` mapping.
