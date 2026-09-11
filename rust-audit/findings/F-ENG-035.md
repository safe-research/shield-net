# F-ENG-035 The blocklist is applied only to the top-level `to`, so R-4.6 misses token recipients, approval spenders, batch sub-calls and the refund receiver — and a blocklisted address with prior history is affirmed `secure`

| Field                | Value                                                                          |
| -------------------- | ------------------------------------------------------------------------------ |
| Status               | Verified                                                                             |
| Crate and module     | sentinel-engine, checkers/blocklist.rs                                          |
| Location             | crates/sentinel-engine/src/checkers/blocklist.rs:24-32 (related: contracts/multi_send.rs:167-172, checkers/address_poisoning.rs:321-333, checkers/nested.rs:42-47) |
| Severity             | High / High                                                                  |
| Certainty            | 93% (V-ENG, Phase 5; E1 — PoC executed) |
| Assumptions involved | A2, A3, A15                                                                     |
| Tags                 | verdict-policy, charter, config                                                 |

## Claim

`BlocklistChecker` denies only when `transaction.to` — the *immediate* call destination — is in the configured
set. Every other address the transaction touches is invisible to it:

- the recipient of an ERC-20 `transfer`/`transferFrom` (`to` is the token contract, not the payee);
- the `spender` of an `approve` / operator of a `setApprovalForAll`;
- every sub-call destination inside a MultiSend batch (`to` is the MultiSend deployment);
- the inner `to` of a nested `execTransaction` payload;
- `gas_token` and `refund_receiver`.

The Charter draws the line the other way round. §2.4 defines the target address as "the address that receives
value or tokens, is granted approvals or permissions, or otherwise receives economically relevant effects from
the transaction; not merely an intermediate contract address called by the Safe transaction", and R-4.6 makes a
transaction insecure when it "interacts with an address or contract" that admissible evidence flags. A token
payment to a flagged address is exactly the case the Charter is describing and exactly the case this checker
cannot see.

Worse than a missed denial: when the flagged address also has prior genuine history with the Safe — the normal
situation for an address that is flagged *after* a counterparty is compromised —
`AddressPoisoningChecker` finds an `ExactMatch` and the engine answers **`secure`**. The operator's explicit
"this address is malicious" configuration is then not merely ignored but contradicted.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| - | ----- | ----------------- | -------- | -------------- |
| 1 | Only `transaction.to` is consulted | E2 | crates/sentinel-engine/src/checkers/blocklist.rs:24-32 | Q1 |
| 2 | Nothing else in the crate reads the blocklist: `BlocklistChecker` is constructed once, from config, and holds only the set | E2 | crates/sentinel-engine/src/checkers/blocklist.rs:8-16; crates/sentinel-engine/src/main.rs:61 | Q2 |
| 3 | MultiSend sub-call destinations are available (`sub_transactions`) but the blocklist checker does not use them | E2 | crates/sentinel-engine/src/contracts/multi_send.rs:167-172 | Q3 |
| 4 | An exact-history match on a flagged recipient yields `Secure` | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:321-333 | Q4 |
| 5 | The Charter's target address is the effect recipient, not the intermediate contract; R-4.6 covers any interaction | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:133-136, :659-663 | Q5 |

**Q1** `crates/sentinel-engine/src/checkers/blocklist.rs:24-32`

```rust
    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        if self.0.contains(&transaction.to) {
            Verdict::Insecure {
                rule: RuleId::R4_6KnownMaliciousTarget,
            }
        } else {
            Verdict::Abstain
        }
    }
```

**Q2** `crates/sentinel-engine/src/checkers/blocklist.rs:8-16` and `crates/sentinel-engine/src/main.rs:61`

```rust
/// Denies transactions to a configured destination.
pub struct BlocklistChecker(HashSet<Address>);

impl BlocklistChecker {
    /// Creates a checker with the destinations to deny.
    pub fn new(blocklist: impl IntoIterator<Item = Address>) -> Self {
        Self(blocklist.into_iter.collect)
    }
}
```

```rust
        Box::new(BlocklistChecker::new(engine_config.blocklist)),
```

**Q3** `crates/sentinel-engine/src/contracts/multi_send.rs:167-172`

```rust
/// `tx` itself, or, if it's a MultiSend batch, each of its sub-calls.
pub fn sub_transactions(tx: &SafeTransaction) -> Vec<SafeTransaction> {
    decode_multi_send_call(tx)
        .map(|(sub_txs, _)| sub_txs)
        .unwrap_or_else(|| vec![tx.clone()])
}
```

**Q4** `crates/sentinel-engine/src/checkers/address_poisoning.rs:321-333`

```rust
        match self
            .established_recipients(transaction.to, transaction.safe, candidate, context.block)
            .await
        {
            Ok(RecipientLookup::ExactMatch) => {
                tracing::debug!(
                    token = %transaction.to,
                    %candidate,
                    rule = kind.rule.code,
                    "address-poisoning: genuine prior interaction found"
                );
                Verdict::Secure
            }
```

**Q5** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:133-136` and `:659-663`

```text
#### Target address means

- the address that receives value or tokens, is granted approvals or permissions, or otherwise receives economically relevant effects from the transaction;
- not merely an intermediate contract address called by the Safe transaction.
```

```text
### R-4.6 — Known malicious or compromised target

#### Rule

- A transaction is insecure if it interacts with an address or contract where admissible evidence supports a reasonable finding that the target is malicious, compromised, exploited, or otherwise high-risk before the Council ruling.
```

## Trigger

Engine config: `blocklist = ["0xBadBadBadBadBadBadBadBadBadBadBadBadBad0"]` (stand-in for a real flagged
address), `address_poisoning_lookback_blocks = 50000`, RPC on the same chain as `chainId` below.

**Trigger A — missed denial (no history required).**

```json
{
  "block": "0x1500000",
  "transaction": {
    "chainId": "0x1",
    "safe": "<the Safe>",
    "to":   "0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48",
    "value": "0x0",
    "data": "<transfer(0xBadBad…Bad0, 1000000000)>",
    "operation": 0,
    "safeTxGas": "0x0", "baseGas": "0x0", "gasPrice": "0x0",
    "gasToken": "0x0000000000000000000000000000000000000000",
    "refundReceiver": "0x0000000000000000000000000000000000000000",
    "nonce": "0x2a"
  }
}
```

Expected per the Charter: `insecure R-4.6`. Actual: `abstain` (the top-level `to` is USDC, which is not listed;
`AddressPoisoning` then finds no history for a fresh flagged address, so it abstains too).

**Trigger B — false `secure`.** The same body, but with the flagged address being one the Safe has genuinely
paid in that token inside the lookback window (the post-compromise case: the counterparty was flagged after the
Safe last paid it). `AddressPoisoningChecker` returns `ExactMatch` → `{"verdict":"secure"}` for a transaction
whose recipient the operator has explicitly marked malicious.

**Trigger C — MultiSend wrapper.** `to = 0x40A2aCCbd92BCA938b02010E17A5b8929b49130D` (a canonical call-only
MultiSend from `multi_send.rs:53-57`), `operation = 1`, `data = multiSend(<one packed Call entry: operation 0, to
= 0xBadBad…Bad0, value = 1, dataLength = 0>)`. `BaseChecker` allows it (`check_calls` returns `true` for a
sub-call whose `to != safe`, `base.rs:91-93`), `BlocklistChecker` sees only the MultiSend address → `abstain`.

**Trigger D — refund receiver.** Any transaction whose `refundReceiver` is the blocklisted address: never
consulted at all.

Corpus shape: A/B/C/D as four request/response pairs against a config whose `blocklist` contains the address
used, each paired with the direct-`to` control (`to = 0xBadBad…Bad0`) that does return `insecure R-4.6` today.

## Considered and rejected

- **Another checker covers the sub-calls.** `ExcessiveApprovalChecker` decodes recipients via
  `decode_target_effects` (`excessive_approval.rs:19-31`) but only for the *amount* test — it never consults the
  blocklist. `AddressPoisoningChecker` decodes the ERC-20 recipient but compares it against on-chain history, not
  against the configured set (`address_poisoning.rs:308-333`). No other code reads
  `EngineConfig::blocklist`.
- **`sub_transactions` is unavailable here.** It is a public helper already used by `staking.rs:93` and
  `cow.rs:429`; nothing prevents `BlocklistChecker` from using it.
- **The operator is expected to list token contracts, not recipients.** The field is documented as "destinations
  to deny" (`blocklist.rs:12`) and the rule it emits is R-4.6, whose Charter text is about the target the
  transaction interacts with; §2.4 explicitly excludes "merely an intermediate contract address", which is what
  the token contract is in Trigger A.
- **Trigger B is really F-ENG-033.** F-ENG-033 is about forged or `value`-blind evidence; here the history is
  genuine and the defect is that a *positive* history signal outranks an explicit operator denial, because the
  denier never got to run for that address.
- **Blocklists are advisory.** They are this engine's only implementation of R-4.6 (`rule.rs:27`,
  `blocklist.rs:26-28`); if they are advisory, the engine implements R-4.6 nowhere.

## Remediation options

1. Check every address the transaction reaches: `sub_transactions(tx)` for batch destinations, plus the
   recipients `decode_target_effects` already extracts (`contracts/target_effects.rs:46-52`), plus `gas_token`
   and `refund_receiver`. Denies as R-4.6 on any hit.
2. As above, and additionally give the blocklist precedence over affirmations by running it before every
   affirming checker (it is already 4th but sits behind `EscapeHatchChecker`; see F-ENG-034) — or by giving
   denials priority over affirmations in `SentinelEngine::security_check` rather than first-wins ordering.
3. Minimum: extend to MultiSend sub-calls and ERC-20 recipients only, and document that approvals inside nested
   `execTransaction` payloads remain out of scope.

Tests to add: unit tests for each of the four address positions (only three exist today, all on the top-level
`to`: `blocklist.rs:50-88`); corpus vectors A–D. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 84%. (Confirms lead ENG-H6 and extends it: the Charter's §2.4
  "not merely an intermediate contract address" makes the ERC-20-recipient case, not the MultiSend case, the
  central one, and Trigger B turns the missed denial into a false `secure`. Cited lines re-read at commit
  2893917; not executed — A9 is FALSE this run.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts

**Claim 1 — only `transaction.to` is consulted: Supported.** `blocklist.rs:24-32` is
`if self.0.contains(&transaction.to)`, and that is the entire body. I checked for any other consumer of the
configured list in the crate and there is none (`config.rs:39` `pub blocklist: Vec<Address>` flows only into
`BlocklistChecker::new` at `main.rs:61`).

**The five blind spots R9 lists are all correct**, and I verified the two that matter most:

- *MultiSend sub-calls.* `decode_multi_send_call` requires `operation == DelegateCall`
  (`contracts/multi_send.rs:143-151`) and the top-level `to` of such a transaction is the MultiSend deployment
  itself (`:152`), which is by definition not the flagged address. `BlocklistChecker` never calls
  `sub_transactions` at all.
- *ERC-20 recipients.* For `to = <token>, data = transfer(<flagged>, X)` the blocklist sees the token.

**The `secure` escalation — Supported, and it is the part that makes this a finding rather than a gap.**
`AddressPoisoningChecker::decode_target` returns `(call.to, TargetKind::Transfer)` for a non-zero `transfer`
(`address_poisoning.rs:122-124`), and an `ExactMatch` in the Safe's own prior history on that token returns
`Verdict::Secure` (`:325-333`). A counterparty flagged *after* being compromised is exactly the address that
*does* have genuine prior history with the Safe, so the operator's explicit "this address is malicious"
configuration is not merely ignored — the engine returns the opposite answer. That inversion is the real
content of this finding and R9 is right to centre it.

### 2. Charter check

I re-read §2.4 in full rather than trusting the excerpt. The definition does say the target address is "the
address that receives value or tokens, is granted approvals or permissions, or otherwise receives economically
relevant effects from the transaction; not merely an intermediate contract address called by the Safe
transaction", and R-4.6:663 is framed as "interacts with an address or contract". The checker implements
precisely the "merely an intermediate contract address" reading the Charter excludes. **Supported**, and under
A15 this is a verdict-policy claim that may reach Confirmed.

### 3. Severity — High confirmed, not Critical

The `secure` inversion satisfies the letter of §8's Critical bullet. I keep **High** because both halves of the
trigger are conditional on deployment state rather than on attacker choice alone: the operator must have
populated `blocklist` (empty in the shipped sample, `sentinel-engine.sample.toml:22`) *and* the flagged address
must already hold non-zero `Transfer`/`Approval` history with this Safe on this token inside the configured
lookback. That is a narrower reachability than F-ENG-030/031/033, where the attacker controls every input. Where
the blocklist is unpopulated the finding degrades to "a configuration surface that does nothing useful", which
is not Critical.

### 4. Trigger quality

Adequate but thinner than R9's other files. For QA I would pin it as: `chainId` = provider's chain, `to` =
token `T`, `data = transfer(F, 1)` where `F` is in the configured `blocklist` **and** has a prior non-zero
`Transfer` from `safe` on `T` within `address_poisoning_lookback_blocks` (50,000 in the sample,
`sentinel-engine.sample.toml:26`), `value = 0`, `operation = 0`, request `block` at or after that prior
transfer. Expected today: `secure`; expected: `insecure R-4.6`.

### 5. Finding verdict

**Confirmed.** Certainty **80%**. Severity **High** (unchanged). See F-ENG-034 §4: distinct defect, same victim.
Note for whoever fixes this: the obvious remediation (recurse the blocklist through `sub_transactions` and
decoded effects) walks into the field-zeroing hazard I documented in F-ENG-032 §3.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection.** `BlocklistChecker::check` is
`if self.0.contains(&transaction.to)` (`blocklist.rs:24-32`) — a single comparison against the immediate
call destination, and the file's whole content. `sub_transactions` is never called from it, and no decoded
recipient, spender, `gas_token` or `refund_receiver` is consulted. Trigger B's false `secure` follows from
`address_poisoning.rs:325-333` returning `Secure` on an `ExactMatch`, which a flagged-after-the-fact
counterparty with genuine history satisfies. Not `E1`.

**Certainty unchanged at 80%. Severity unchanged at High.**

**PoC: `rust-audit/poc/F-ENG-035/`** — three tests appended to `blocklist.rs`, covering all four uncovered
address positions (ERC-20 recipient, `approve` spender, MultiSend sub-call destination, `refundReceiver`)
plus a control on the one position that works today. One test is expected to fail on unfixed code, and it
iterates so a partial fix shows exactly which positions it closed.

**Trigger B is deliberately not in this file.** The false-`secure` variant needs `AddressPoisoningChecker`
and a mocked provider; the README points at `rust-audit/poc/F-ENG-033/`'s harness and gives the three-line
recipe (seed `Transfer(safe → FLAGGED, 1000)`, register both checkers, assert the engine does not return
`Secure`) rather than duplicating the mock plumbing. Whoever fixes this should add it there.

### Remediation check

- **Option 1 (check every address the transaction reaches) — sound, and closer to hand than the finding
  says.** `decode_target_effects` (`contracts/target_effects.rs:44-51`) already returns a
  `TargetEffect { recipient, kind }` for every ERC-20/721/1155 recipient, approval spender and native
  transfer, **and already recurses through MultiSend**, so option 1 is roughly that plus `gas_token` and
  `refund_receiver` — not a new decoder. **Two cautions the finding does not raise.** (a) That recursion has
  **no depth limit** (F-ENG-006), and the only thing keeping attacker-chosen depth away from it today is
  that `BaseChecker` at position 3 denies deep batches first; moving the decoder into `BlocklistChecker`
  at position 4 does not change that ordering, but any future reordering would — fix F-ENG-006 in the same
  change rather than relying on it. (b) It still does not decode the inner `to` of a nested
  `execTransaction` payload, so that position stays uncovered and must be documented as an explicit scope
  boundary rather than left implicit.
- **Option 2 (give the blocklist precedence over affirmations) — sound and necessary for Trigger B.**
  Option 1 alone lets `BlocklistChecker` *see* the flagged recipient but does not stop
  `EscapeHatchChecker` (F-ENG-034) affirming ahead of it, nor `AddressPoisoningChecker` affirming behind it
  when the blocklist abstains for some other reason. The "run it before every affirming checker" half is
  another per-pair patch; the "give denials priority in `security_check`" half is F-ENG-044 option 1 and is
  the durable form. **Take the latter.**
- **Option 3 (ERC-20 recipients and MultiSend sub-calls only) — sound as a staged first step**, and it
  closes the two positions with the clearest §2.4 standing. If it is taken, **close the refund receiver
  too**: it is a one-line comparison with no decoding at all, so omitting it is not a scoping decision.
- **Charter support is unusually direct here**, and the fix should quote it: §2.4 defines the target
  address as one that "receives value or tokens, is granted approvals or permissions, or otherwise receives
  economically relevant effects from the transaction; **not merely an intermediate contract address called
  by the Safe transaction**" — which is a description of `transaction.to` in three of the four positions.
- **Test hook: none needed for A/C/D** (`BlocklistChecker` is pure; its three existing tests,
  `blocklist.rs:50-88`, all exercise the position that works). Trigger B needs `Provider::mocked`, which
  exists.
- **Where the fix belongs: the checker, plus the combinator for Trigger B.** The `RuleId` mapping is fine.

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 80% -> **93%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a
binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source
file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under
`rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file
was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only
executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-035/append-to-src-checkers-blocklist.rs` appended to
`crates/sentinel-engine/src/checkers/blocklist.rs`, then `cargo test -p sentinel-engine poc_f_eng_035`.
Compiled on the first attempt. Full output: `rust-audit/poc/F-ENG-035/run-output.txt`.

### Verbatim result

```
running 3 tests
test checkers::blocklist::poc_f_eng_035::poc_f_eng_035_control_the_top_level_to_is_denied ... ok
test checkers::blocklist::poc_f_eng_035::poc_f_eng_035_every_other_position_abstains_today ... ok
test checkers::blocklist::poc_f_eng_035::poc_f_eng_035_every_address_the_transaction_reaches_must_be_checked ... FAILED

---- ..._every_address_the_transaction_reaches_must_be_checked stdout ----
assertion `left == right` failed: erc20 recipient: § 2.4's target address is not `transaction.to`
  left: Abstain
 right: Insecure { rule: R4_6KnownMaliciousTarget }

test result: FAILED. 2 passed; 1 failed; 0 ignored; 0 measured; 98 filtered out
```

The single expected-to-fail test failed for the claimed reason.

### All four positions, not just the one named in the panic

The regression test loops and `assert_eq!` panics on the first position, so its own output names only
`erc20 recipient`. **All four positions are nevertheless proven un-checked**, by test (1)
(`poc_f_eng_035_every_other_position_abstains_today`), which **passed** and whose body is an `assert_eq!` to
`Verdict::Abstain` over each in turn — it cannot pass unless every one of them returned `Abstain`:

| Position | Fixture | Executed verdict |
| --- | --- | --- |
| ERC-20 recipient | `to = USDC`, `transfer(FLAGGED, 1e9)` | `Abstain` |
| `approve` spender | `to = USDC`, `approve(FLAGGED, 1e9)` | `Abstain` |
| MultiSend sub-call | `to = MultiSendCallOnly`, `DelegateCall`, one packed entry sending 1 wei to `FLAGGED` | `Abstain` |
| `refundReceiver` | `gasPrice = 1`, `gasToken = USDC`, `refundReceiver = FLAGGED` | `Abstain` |

The control (test 0) passed: with `FLAGGED` as the top-level `to` the checker does deny under
`R4_6KnownMaliciousTarget`, so the blocklist itself works and the fixture's address really is configured.

Residual uncertainty (why 93 and not higher): the code behaviour is settled by execution, but the finding's
*severity* argument depends on the Charter §2.4 definition of "target address" under A7/A15, and on the second
half of the title — "a blocklisted address with prior history is affirmed `secure`" — which is a composition
claim across `AddressPoisoningChecker` and the F-ENG-044 combinator rather than something this PoC exercised
end to end. Both of those components are independently verified this phase (F-ENG-033, F-ENG-044), but the
composed path was not itself run.
