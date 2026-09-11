# F-ENG-031 The gas-refund leg is never vetted on any transaction an affirming checker approves, so an unbounded native-currency refund drain is rated `secure`

| Field | Value |
| --- | --- |
| Status | Verified |
| Crate and module | sentinel-engine, checkers/{staking,nested,cow,address_poisoning,refund}.rs |
| Location | crates/sentinel-engine/src/checkers/refund.rs:97-103 (related: checkers/staking.rs:88-116, checkers/nested.rs:42-47, checkers/cow.rs:351-383, checkers/address_poisoning.rs:308-333, main.rs:57-73) |
| Severity | Critical / Critical |
| Certainty | 99% (RW-ENG, Phase 8; E1 — reproduced end-to-end against the running service on local Anvil; was 96%, V-ENG Phase 5) |
| Assumptions involved | A2, A3, A15 |
| Tags | verdict-policy, charter, input-validation, known |

## Claim

Four of the six checkers that can return `Verdict::Secure` — `NestedSafeChecker`, `CowChecker`, `StakingChecker` and `AddressPoisoningChecker` — reach that verdict without reading `gas_price`, `base_gas`, `gas_token` or `refund_receiver`. Only `CancellationChecker` (all fields default) and `EscapeHatchChecker` (`gas_price` must be zero) are immune.

The only checker that looks at the refund leg, `RefundChecker`, (a) is deny-only by construction (`refund.rs:68-73`), (b) runs 9th, after all four affirmers (`main.rs:57-73`), and (c) returns `None` — i.e. abstains — for exactly the case with the largest impact, a **native-currency** refund (`gas_token == 0`), which its own TODO says nothing else inspects (`refund.rs:83-89`). It is also dead for the ERC-20 case (F-ENG-032).

Safe's `handlePayment` pays `(gasUsed + baseGas) * gasPrice` to `refundReceiver`. `baseGas` is fully attacker-controlled and unbounded, so even the native path (where `gasPrice` is capped at `tx.gasprice`) lets a proposer drain an arbitrary amount of the Safe's ETH to an address of their choosing. With `gas_token` set to an ERC-20 there is no `tx.gasprice` cap at all.

Under the Charter, the refund parameters are part of the transaction under review (§2.3 lists "gas and refund parameters" among a transaction's contents) and the refund pays value to `refundReceiver`, so R-4.3 applies; §3.7 forbids calling a transaction secure unless _all_ applicable Article IV rules are satisfied. A `secure` verdict here is a malicious Safe transaction rated `secure` (PROMPT.md §8: Critical).

The `known` tag applies to the two TODO comments at `refund.rs:83` and `:93` (codebase-map §4); the _finding_ — that four affirmers reach `secure` with the refund leg entirely unexamined — is the consequence those TODOs predict and is not itself limited to them.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | `refund_transfer` abstains outright on a native-currency refund, and the code says nothing else inspects it | E2 | crates/sentinel-engine/src/checkers/refund.rs:83-103 | Q1 |
| 2 | `StakingChecker` returns `Secure` from a claims-only / lone-`stake` / `[approve<=stake]` shape without reading any refund field | E2 | crates/sentinel-engine/src/checkers/staking.rs:88-116 | Q2 |
| 3 | The per-call helpers `StakingChecker` uses check `operation`, `value` and `to` only — never `gas_price` | E2 | crates/sentinel-engine/src/checkers/staking.rs:156-161 | Q3 |
| 4 | `AddressPoisoningChecker` affirms on an exact history match without reading `value` or any refund field | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:325-333 | Q4 |
| 5 | `CowChecker::check_twap_batch` affirms on calldata terms alone | E2 | crates/sentinel-engine/src/checkers/cow.rs:375-383 | Q5 |
| 6 | Ordering: `RefundChecker` is 9th, behind Nested (5th), Cow (7th) and Staking (8th); the comment justifying its position only considers `address_poisoning` | E2 | crates/sentinel-engine/src/main.rs:62-73 | Q6 |
| 7 | `RefundChecker` can never affirm, so it cannot compensate — it only ever denies or abstains | E2 | crates/sentinel-engine/src/checkers/refund.rs:68-73 | Q7 |
| 8 | The Charter counts gas/refund parameters as part of the transaction, and R-4.3/§3.7 apply | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:108-125, :592, :439-440 | Q8 |

**Q1** `crates/sentinel-engine/src/checkers/refund.rs:83-89` and `:97-103`

```rust
///   TODO(follow-up): this checker abstaining doesn't mean anything else
///   inspects a native refund either. Since the engine-wide "abstain on any
///   nonzero `gasPrice`" guard that used to sit ahead of the whole checker
///   chain is gone, a transaction another checker calls `Secure` can now
///   drain unbounded native currency to `refundReceiver` uncommented on. A
///   native-value-aware check (or at least an amount cap) is needed before
///   this is safe to affirm.
```

```rust
fn refund_transfer(transaction: &SafeTransaction) -> Option<SafeTransaction> {
    if transaction.gas_price.is_zero
        || transaction.gas_token.is_zero
        || transaction.refund_receiver.is_zero
    {
        return None;
    }
```

**Q2** `crates/sentinel-engine/src/checkers/staking.rs:88-96` and `:109-116`

```rust
    async fn check(&self, transaction: &SafeTransaction, _context: &CheckContext) -> Verdict {
        if transaction.chain_id != U256::from(SUPPORTED_CHAIN_ID) {
            return Verdict::Abstain;
        }

        let calls = sub_transactions(transaction);

        let mut remaining = Vec::with_capacity(calls.len());
        let mut claimed = false;
```

```rust
        match remaining.as_slice {
            [] if claimed => Verdict::Secure,
            [] => Verdict::Abstain,
            [call] => check_lone_call(call),
            [first, second] => check_pair(first, second),
            _ => Verdict::Abstain,
        }
    }
}
```

**Q3** `crates/sentinel-engine/src/checkers/staking.rs:156-161`

```rust
fn claim_account(tx: &SafeTransaction) -> Option<Address> {
    if tx.operation != Operation::Call || !tx.value.is_zero || tx.to != REWARDS_DISTRIBUTOR {
        return None;
    }
    Some(claimCall::abi_decode(&tx.data).ok?.account)
}
```

**Q4** `crates/sentinel-engine/src/checkers/address_poisoning.rs:325-333`

```rust
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

**Q5** `crates/sentinel-engine/src/checkers/cow.rs:375-383`

```rust
        if approved_token != sell_token
            || approved_amount > max_approval_for_twap_total(total_sell_amount, n)
        {
            return Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            };
        }
        Verdict::Secure
    }
```

**Q6** `crates/sentinel-engine/src/main.rs:62-73`

```rust
        Box::new(NestedSafeChecker),
        Box::new(ExcessiveApprovalChecker),
        Box::new(CowChecker::new()),
        Box::new(StakingChecker),
        // RPC-backed, so they run last: cheaper local checkers above get a
        // chance to reach a verdict first. `RefundChecker` can only deny or
        // abstain (never affirm), so its position relative to
        // `address_poisoning` doesn't affect correctness, only which RPC
        // lookup runs first when both apply.
        Box::new(RefundChecker::new(address_poisoning.clone())),
        Box::new(address_poisoning),
    ]);
```

**Q7** `crates/sentinel-engine/src/checkers/refund.rs:68-73`

```rust
fn deny_or_abstain(verdict: Verdict) -> Verdict {
    match verdict {
        Verdict::Secure => Verdict::Abstain,
        verdict => verdict,
    }
}
```

**Q8** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:108-125`, `:592`, `:439-440`

```text
### § 2.3 Transaction
...
#### Includes
...
- gas and refund parameters, if included in the Safe transaction payload;
```

```text
- A transaction is insecure if it sends value to a recipient address outside the expected target set.
```

```text
- A transaction is secure only if it satisfies all applicable Article IV rules.
- If it fails any Article IV rule, it is insecure.
```

## Trigger

**Trigger A — fully deterministic, no RPC and no CoW API involved (StakingChecker).** A single mainnet `claim` to the canonical rewards distributor with the Safe as `account`, carrying a hostile native refund leg:

```json
{
  "block": "0x1500000",
  "transaction": {
    "chainId": "0x1",
    "safe": "0x5aFE3855358E112B5647B952709E6165e1c1eEEe",
    "to": "0xe5139fc0fb8eae81e30d8a85c22e88c6757120f2",
    "value": "0x0",
    "data": "<claim(address account = safe, uint256 cumulativeAmount, bytes32 expectedMerkleRoot, bytes32[] merkleProof)>",
    "operation": 0,
    "safeTxGas": "0x0",
    "baseGas": "0xe8d4a51000",
    "gasPrice": "0x3b9aca00",
    "gasToken": "0x0000000000000000000000000000000000000000",
    "refundReceiver": "0x00000000000000000000000000000000AttackerA",
    "nonce": "0x2a"
  }
}
```

`claim`'s arguments beyond `account` are never inspected, so any well-formed encoding works (e.g. `account = safe`, `cumulativeAmount = 0`, `expectedMerkleRoot = 0x00..00`, `merkleProof = []`). Chain order: Cancellation abstains (non-default fields), EscapeHatch abstains (`gas_price != 0`), Base abstains (`to != safe`, a plain `Call`), Blocklist abstains, Nested abstains (wrong selector), ExcessiveApproval abstains (no approval effect), Cow abstains (no relayer approve, no presig/TWAP), **Staking returns `Secure`** at `staking.rs:110`. Result: `{"verdict":"secure"}` for a transaction whose refund pays `(gasUsed + 10^12) * min(10^9, tx.gasprice)` wei — about 1,000 ETH at a 1 gwei base fee — to `refundReceiver`.

**Trigger B — ERC-20 refund, unbounded (same shape).** As A but `gasToken` set to a token the Safe holds and `gasPrice` set to `0xde0b6b3a7640000` (1e18). Safe applies no `tx.gasprice` cap for ERC-20 refunds, so the payout is `(gasUsed + baseGas) * 1e18` token units. `RefundChecker` still cannot deny it — see F-ENG-032.

**Trigger C — combined with F-ENG-030.** The `execTransaction`-shaped `Call` of F-ENG-030 plus the refund fields above drains `value` _and_ the refund in one request, still `secure`.

**Trigger D — CoW.** The 2-call TWAP batch of `cow.rs`'s own `approves_when_the_approval_exactly_covers_the_twap_order` test vector (`cow.rs:879-890`), delivered as a MultiSend delegatecall with the outer transaction's `gasPrice`/`baseGas`/`refundReceiver` set as in A. Verdict `Secure` from `cow.rs:382`.

**Trigger E — AddressPoisoning.** `to` = a token the Safe has paid before, `data = transfer(<that same prior recipient>, 1)`, refund fields as in A. Verdict `Secure` from `address_poisoning.rs:332`.

Corpus shape: each of A, B, D, E as a request/response pair, each paired with a control that is identical except for `gasPrice: "0x0"` (which must keep the same verdict) — the delta between the two is the regression signal.

## Considered and rejected

- **`RefundChecker` denies it.** For triggers A and C `gas_token == 0`, so `refund_transfer` returns `None` at `refund.rs:98-103` and the checker abstains before any lookup. For B it is dead for the separate reason in F-ENG-032. In every case it runs 9th, after the affirmer has already ended the chain (`engine/mod.rs:66-68`).
- **`EscapeHatchChecker`'s `gas_price` guard covers the chain.** It only guards its own affirmation (`escape_hatch.rs:42-45`); it abstains for every other shape.
- **`baseGas` is bounded by the block gas limit.** It is not: `baseGas` is a Safe transaction field the proposer chooses, added to the measured `gasUsed` inside `handlePayment`; it never has to be spendable gas. Only `safeTxGas` bounds actual execution.
- **A Safe owner would notice the refund fields.** Irrelevant to this finding: the engine's verdict is the sentinel's attestation input, and under A2 the proposal contents are attacker-chosen.
- **This duplicates F-ENG-030.** F-ENG-030 is one affirmer ignoring `value`; this is four affirmers ignoring the refund leg, including two (`Cow`, `Staking`) whose primary leg is otherwise correctly vetted.
- **The Charter leaves gas/refund out of scope.** §2.18 puts gas constraints on the _announcement_ transaction outside the Charter, but §2.3 explicitly includes gas and refund parameters in the transaction under review, and R-4.3 speaks of value recipients generally, not only of `to`.

## Remediation options

1. Reinstate a chain-wide pre-gate: any transaction with `gas_price != 0` that no checker can positively vet may not be affirmed. Simplest form — a checker at position 1 that returns `Abstain` unconditionally when `gas_price != 0`, mirroring the guard PR #876 removed. Costs the vote on every genuinely relayed transaction.
2. Make the affirmation a two-part decision: move `RefundChecker` (fixed per F-ENG-032, and extended to the native and `refund_receiver == 0` cases) ahead of every affirmer, and have it deny — not abstain — whenever the refund leg cannot be vetted. Keeps relayed transactions votable once a relayer policy exists.
3. Restructure the engine so `Secure` requires all deny-capable checkers to have run (run every denier first, then consider affirmations), which also fixes F-ENG-034 and F-ENG-035. Largest change; matches Charter §3.7 directly.
4. Minimum stop-gap: require `gas_price.is_zero` inside each of the four affirming predicates, as `EscapeHatchChecker` already does.

Tests to add: corpus vectors A, B, D and E above; a unit test asserting that each affirming checker returns `Abstain` when only `gas_price`/`base_gas`/`refund_receiver` differ from an otherwise-`Secure` fixture. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 90%. (Confirms lead ENG-H4 and extends it: the strongest trigger is `StakingChecker`, which is pure and therefore deterministic without any RPC or CoW API. Safe `handlePayment` semantics are protocol knowledge, class I — `Safe.sol` is not in this checkout; the _engine-side_ claim that no affirmer reads the refund fields is E2 and is what this finding rests on.)

## Critic (C-ENG-B)

### 1. Per-claim verdicts

I re-derived the checker chain independently first; the derivation is written out in F-ENG-030's Critic section §0 and confirms the premise (`engine/mod.rs:57-72` breaks at the first non-`Abstain`; the crate's own test `stops_at_the_first_non_abstaining_verdict`, `engine/mod.rs:104-120`, asserts a `Secure` beats a later `Insecure`).

**The "four of six affirmers" arithmetic is exactly right, and I verified it by enumerating every checker's return values rather than taking R9's word.** Only six of the ten can ever return `Verdict::Secure`: `CancellationChecker` (`cancellation.rs:23-27`), `EscapeHatchChecker` (`escape_hatch.rs:42-46`), `NestedSafeChecker` (`nested.rs:31-35`), `CowChecker` (`cow.rs:382`, `:313`), `StakingChecker` (`staking.rs:110`, `:126`, `:146`) and `AddressPoisoningChecker` (`address_poisoning.rs:332`). `BaseChecker` returns only `Abstain`/`Insecure` (`base.rs:41-46`), `BlocklistChecker` only `Insecure`/`Abstain` (`blocklist.rs:24-32`), `ExcessiveApprovalChecker` only `Insecure`/`Abstain` (`excessive_approval.rs:19-33`), and `RefundChecker` squashes `Secure` to `Abstain` (`refund.rs:68-73`).

Of those six, `CancellationChecker` compares the whole struct against an all-default value so every refund field must be zero, and `EscapeHatchChecker` requires `!tx.gas_price.is_zero` to be false (`escape_hatch.rs:53`) — and with `gasPrice == 0` Safe pays no refund at all. The other four — `NestedSafeChecker`, `CowChecker`, `StakingChecker`, `AddressPoisoningChecker` — contain no reference to `gas_price`, `base_gas`, `gas_token` or `refund_receiver` anywhere in their files. **Supported.**

`RefundChecker`'s three disqualifiers are verbatim at `refund.rs:97-103`:

```rust
    if transaction.gas_price.is_zero
        || transaction.gas_token.is_zero
        || transaction.refund_receiver.is_zero
    {
        return None;
    }
```

so the native-currency case (`gas_token == 0`) returns `None` → `Abstain`. **Supported.** Its position (9th, `main.rs:71`) and deny-only construction (`refund.rs:68-73`) are as claimed. **Supported.**

### 2. The trigger — verified step by step

R9's `StakingChecker` vector is the right choice and it holds. `StakingChecker::check` (`staking.rs:88-116`) reads `transaction.chain_id`, then `sub_transactions(transaction)`, then only the sub-calls' `operation`/`value`/`to`/`data`. For a single `claim` whose `account == transaction.safe`, `claim_account` returns `Some(safe)`, `claimed` becomes `true`, `remaining` is empty, and `staking.rs:110` returns `Verdict::Secure`. It is pure — no RPC, no HTTP, no `CheckContext` — so the vector is fully deterministic and encodable as a corpus vector today.

Concrete vector, which I confirm is constructible under A2 and concrete enough for QA: `chainId = 1`, `safe` = victim, `to = 0xe5139fc0fb8eae81e30d8a85c22e88c6757120f2` (`REWARDS_DISTRIBUTOR`, `staking.rs:74`), `value = 0`, `operation = 0`, `data = abi.encodeCall(claim, (safe, anyCumulativeAmount, anyRoot, anyProof))`, `safeTxGas = 0`, `baseGas = 1e12`, `gasPrice = <a plausible gas price>`, `gasToken = 0x0` (native) **or** any ERC-20 (see §3), `refundReceiver` = attacker, `nonce` = next. Expected today: `secure`.

Note for QA: `claim`'s `account` argument **must** equal `safe`, or `staking.rs:99-103` denies with `R4_3ValueTarget` — the affirmation and the denial differ by one field, which makes this a good paired vector.

### 3. Severity, and exactly how far the `known` tag reaches — the question I was asked to settle

The tag is warranted but it covers **less** than the finding. The TODO at `refund.rs:83-89` reads:

```rust
    ///   TODO(follow-up): this checker abstaining doesn't mean anything else
    ///   inspects a native refund either. Since the engine-wide "abstain on any
    ///   nonzero `gasPrice`" guard that used to sit ahead of the whole checker
    ///   chain is gone, a transaction another checker calls `Secure` can now
    ///   drain unbounded native currency to `refundReceiver` uncommented on. A
    ///   native-value-aware check (or at least an amount cap) is needed before
    ///   this is safe to affirm.
```

That is R9's claim almost verbatim — but only for `gasToken == 0`. The second TODO (`refund.rs:93-96`) covers only `refundReceiver == 0`. **Neither TODO covers the ERC-20 case**: with `gasToken` set to a real ERC-20 and `refundReceiver` non-zero, `refund_transfer` _does_ build a synthetic transfer, so the team's stated design is that this path _is_ checked. It is not — the delegated check is dead (F-ENG-032) — and, independently of that, `StakingChecker` at position 8 affirms before `RefundChecker` at position 9 ever runs, so even a repaired `RefundChecker` would not see this transaction. The ERC-20 path also has no `tx.gasprice` cap. **The highest-impact sub-case of this finding is not a `known` item at all.**

I therefore keep **Critical** rather than reducing it, and I record my reasoning explicitly because the brief asked for it: A12 directs that known items be _reported at reduced priority_, which the `known` tag on this file already expresses; it does not direct that impact be re-classified. PROMPT.md §8's Critical bullet is stated in terms of outcome ("a malicious Safe transaction rated `secure`"), and the outcome is unchanged by the team having predicted it. Filing a live, attacker-triggerable, unbounded drain as Informational because a TODO exists would make the report actively misleading. Readers should take the `known` tag to mean "predicted by the authors; fix outstanding", and §3.7:439 of the Charter — "A transaction is secure only if it satisfies all applicable Article IV rules" — as the reason the affirmation is wrong regardless.

### 4. What is `I` rather than `E2` here

The engine-side claim (four affirmers never read the refund leg, and the chain stops at the first of them) is `E2` throughout. The _impact_ step — Safe's `handlePayment` paying `(gasUsed + baseGas) * gasPrice` to `refundReceiver`, capped at `tx.gasprice` for native and uncapped for an ERC-20 — is `I`: `Safe.sol` is not in this checkout. R9 labels this honestly in their §7. It is corroborated inside this checkout by `escape_hatch.rs:30-32` ("Safe.sol only calls `handlePayment` `if (gasPrice > 0)`") and by `SafenetGuard.sol:352` ("must be 0 (a non-zero value triggers an unattested Safe refund)"), which is why I am comfortable at 85 rather than lower — but QA should treat the arithmetic as the one step needing external confirmation.

### 5. Finding verdict

**Confirmed.** Certainty **85%**. Severity **Critical** (unchanged), `known` retained but scoped as in §3.

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1.

**Inspection: Reproduced by inspection**, for the engine-side claim only. I re-enumerated the six checkers that can return `Secure` and confirmed that `nested.rs`, `cow.rs`, `staking.rs` and `address_poisoning.rs` contain no occurrence of `gas_price`, `base_gas`, `gas_token` or `refund_receiver` anywhere in their files, while `cancellation.rs:16-27` compares the whole struct against an all-default value and `escape_hatch.rs:53` requires `gas_price.is_zero`. `refund.rs:97-103` returns `None` on `gas_token == 0`, and `refund.rs:68-73` can never affirm. The **impact** step — Safe's `handlePayment` paying `(gasUsed + baseGas) * gasPrice`, capped at `tx.gasprice` for native and uncapped for an ERC-20 — remains class `I`: `Safe.sol` is not in this checkout, and I did not resolve it. Treat that arithmetic as the one step still needing external confirmation, as the Critic's §4 says.

**Certainty unchanged at 85%. Severity unchanged at Critical**, and I endorse the Critic's §3 reasoning for keeping it: A12 directs that known items be reported at reduced _priority_, not that impact be reclassified, and the highest-impact sub-case is not a `known` item at all.

**PoC: `rust-audit/poc/F-ENG-031/`** — six tests appended to `staking.rs` (which has no test block today), covering the native, ERC-20 and `refundReceiver == 0` refund legs, with two controls. Two tests are expected to fail on unfixed code. `StakingChecker` was chosen deliberately: it is the only one of the four unguarded affirmers that is **pure**, so the PoC is hermetic and deterministic with no mock, no Anvil and no CoW API.

**The ERC-20 leg is written out as the primary fixture**, per the brief: `gasToken` = USDC, `gasPrice = 1e18`, `baseGas = 1e12`. It is the sub-case **neither** TODO covers — `refund.rs:83-89` covers only `gasToken == 0` and `refund.rs:93-96` only `refundReceiver == 0` — and it is the one path where `refund_transfer` _does_ build a synthetic transfer, i.e. where the team's stated design is that the leg **is** checked. It is not, for two independent reasons that must both be fixed: the delegated check is dead (F-ENG-032), and `StakingChecker` at position 8 affirms before `RefundChecker` at position 9 runs. It also carries no `tx.gasprice` cap. The report should present this, not the native `StakingChecker` vector, as the finding's headline.

### Remediation check

- **Option 1 (chain-wide `gas_price != 0` → `Abstain` pre-gate) — sound, and the only option safe to ship today.** Cost is as stated: no vote on genuinely relayed transactions, which per `crates/sentinel/src/service.rs:173-179` means the request is dropped unanswered rather than voted wrongly. **One consequence the finding does not name:** it makes `RefundChecker` unreachable, since `refund_transfer` returns `None` unless `gas_price != 0`. If option 1 ships, do not read F-ENG-032's fix as "no longer needed" — it becomes untestable in production while remaining a live defect behind the gate.
- **Option 2 (move a repaired `RefundChecker` ahead of the affirmers and have it _deny_ an unvettable leg) — sound in direction, unsound as literally specified.** Denying whenever the leg cannot be vetted means denying every native refund and every `refundReceiver == 0`, i.e. denying honest relayed traffic. That is a wrong vote in the denying direction, and this audit has now established (see F-ENG-042's `## QA`) that a good-faith denial overruled in arbitration is slashed — `contracts/src/libraries/SentinelOracleRequests.sol:301-304`. **The correct form is abstain, not deny, promoted ahead of the affirmers**: that suppresses the affirmation without manufacturing a denial. Reserve denial for a leg that _is_ vettable and fails, e.g. a poisoned ERC-20 receiver.
- **Option 3 (run every denier before considering affirmations) — sound, and it is F-ENG-044 option 1 under another name; it also fixes F-ENG-034 and F-ENG-035.** But note it does **not** fix this finding on its own: no checker in the chain currently _denies_ on a hostile refund leg, so running all deniers first still yields `secure`. Options 3 **and** a refund policy are both required.
- **Option 4 (`gas_price.is_zero` inside each affirming predicate) — sound and minimal**, and the smallest diff that turns this PoC's regression test green. It duplicates one condition across four files, so it decays the moment a fifth affirmer is written — F-ENG-044 again.
- **A `gasPrice` cap is not a substitute for a refund policy.** Any cap must bound `gasPrice × (safeTxGas + baseGas)`, not `gasPrice` alone: `baseGas` is the unbounded, proposer-chosen multiplier, added to the _measured_ `gasUsed`, so it never has to be spendable gas and the block gas limit does not constrain it.
- **Test hook: exists for all four affirmers.** `StakingChecker` and `NestedSafeChecker` are pure; `CowChecker::with_order_api` (`cow.rs:242`) and `Provider::mocked` (`crates/core/src/provider/mod.rs:139`) cover the other two, and this audit's F-ENG-033 and F-ENG-037 PoCs use both. The `sentinel-test-vectors` corpus (A8, unavailable) is the wrong oracle here regardless: the assertion is a **relation between two vectors** — identical but for the refund fields — which a corpus of independent request/response pairs cannot state.
- **Where the fix belongs: the combinator plus a new refund policy**, not the `RuleId` mapping (R-4.3 already covers value sent to an unexpected recipient, and the refund is such a transfer).

## Verification (V-ENG, Phase 5)

**Reproduced by execution. Basis class E1.** Certainty 85% -> **96%**.

### Environment and method

cargo 1.98.1 / rustc 1.98.1, `stable-aarch64-unknown-linux-gnu`, at commit `2893917`. `sentinel-engine` is a binary-only crate (no `src/lib.rs`, no `[lib]`), so QA-ENG's PoC was appended verbatim into the tracked source file it targets, run with `cargo test -p sentinel-engine <filter>`, the produced source archived under `rust-audit/poc/<id>/ran-source-*.rs`, and the file then restored with `git checkout -- <file>`. No tracked file was left modified by this agent. A8 remains FALSE (no `sentinel-test-vectors` corpus): these tests are the only executable oracle for this checker.

### What was run

`rust-audit/poc/F-ENG-031/append-to-src-checkers-staking.rs` appended to `crates/sentinel-engine/src/checkers/staking.rs`, then `cargo test -p sentinel-engine poc_f_eng_031`. Compiled on the first attempt. `StakingChecker` is a pure function of `SafeTransaction` — no RPC, no HTTP, no `CheckContext` — so the run is fully deterministic. Full output: `rust-audit/poc/F-ENG-031/run-output.txt`.

### Verbatim result

```
running 4 tests
test checkers::staking::poc_f_eng_031::poc_f_eng_031_affirms_every_hostile_refund_leg_today ... ok
test checkers::staking::poc_f_eng_031::poc_f_eng_031_control_a_claim_for_another_account_is_denied ... ok
test checkers::staking::poc_f_eng_031::poc_f_eng_031_control_an_unrelayed_claim_is_affirmed ... ok
test checkers::staking::poc_f_eng_031::poc_f_eng_031_a_hostile_refund_leg_must_not_be_affirmed ... FAILED

---- ..._a_hostile_refund_leg_must_not_be_affirmed stdout ----
assertion `left != right` failed: native: the refund pays value to an attacker-chosen address and was \
never vetted
  left: Secure
 right: Secure

test result: FAILED. 3 passed; 1 failed; 0 ignored; 0 measured; 97 filtered out
```

The single expected-to-fail test failed for the claimed reason.

### All three refund legs, including the ERC-20 one

The regression test (2) loops over three legs and `assert_ne!` panics on the first, so its own output names only the `native` leg. **The ERC-20 leg is nevertheless proven affirmed**, by test (1) (`poc_f_eng_031_affirms_every_hostile_refund_leg_today`), which **passed** and whose body is an `assert_eq!` to `Verdict::Secure` over all three legs in turn — it cannot pass unless every one of them returned `Secure`:

| Leg | Fixture | Executed verdict |
| --- | --- | --- |
| native | `baseGas = 1e12`, `gasPrice = 1 gwei`, `gasToken = 0`, `refundReceiver = attacker` | `Secure` |
| **erc20** | `baseGas = 1e12`, **`gasPrice = 1e18`**, **`gasToken = USDC`**, `refundReceiver = attacker` | **`Secure`** |
| tx.origin | `baseGas = 1e12`, `gasPrice = 1 gwei`, `gasToken = 0`, **`refundReceiver = 0`** | `Secure` |

The ERC-20 leg is the one C-ENG-B singled out: with a non-zero `gasToken` there is no `tx.gasprice` cap at all, so `gasPrice = 1e18` is accepted, and it is **not** covered by either `refund.rs` TODO (both of which are about the native-currency and `tx.origin` cases). It is now executed fact that `StakingChecker` affirms it.

The two controls also passed, which is what makes the delta meaningful: the honest mainnet `claim` with every refund field zero is affirmed (test 0), and a `claim` naming an account other than the Safe is denied `R-4.3` (test 0b). The hostile fixtures differ from the affirmed control **only** in refund fields.

Note the interaction with F-ENG-032, also verified this phase: the checker that was supposed to catch the ERC-20 leg (`RefundChecker`) is dead, and even if it were not, `StakingChecker` at chain position 8 affirms before it runs at position 9 (F-ENG-044, also verified). All three defects are independently confirmed by execution.

Residual uncertainty: the Charter §2.3/R-4.3 mapping (A7/A15) and the Safe `handlePayment` semantics under A3 (`(gasUsed + baseGas) * gasPrice`, unbounded by the block gas limit because `baseGas` is a proposer-chosen addend) — neither of which this run executed. The engine-side behaviour is settled.

## Real-world validation (Phase 8, RW-ENG)

### Scenario

Same live deployment as F-ENG-030: Anvil 1.8.1 on `127.0.0.1:8545` (chain 31337), a real Safe 1.5.0 proxy at `0x643d887734c637f108B095dc3EE0e06F79bC320C` (owner Anvil account 0, threshold 1) holding 1000 ETH and 1,000,000 units of a real ERC-20 at `0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9`, and the real `sentinel-engine` binary serving `127.0.0.1:5473` from a copy of the shipped sample config with `rpc = "http://127.0.0.1:8545"` (chain id read at startup: `0x7a69`). Attacker / `refundReceiver` is the EOA `0x90F79bf6EB2c4f870365E785982E1f101E93b906`.

Two refund legs were submitted at the running service, both behind the same affirmer (`NestedSafeChecker`) and both executed afterwards against the real Safe.

### Verbatim outcome — the ERC-20 refund leg (the leg C-ENG-B showed the `known` TODOs do not cover)

Request: `to` a harmless third-party address, `data` a well-formed `execTransaction` blob, `value` zero — and the refund leg loaded:

```json
"safeTxGas":"0x7a120","baseGas":"0x7a120","gasPrice":"0xe8d4a51000",
"gasToken":"0xDc64a140Aa3E981100a9becA4E685f962f0cF6C9",
"refundReceiver":"0x90F79bf6EB2c4f870365E785982E1f101E93b906"
```

(`gasPrice` 1e12 × (`safeTxGas` + `baseGas`) 1e6 = 1e18 tokens payable to the attacker.)

Response, verbatim:

```
{"verdict":"secure"}
```

Checker trace:

```
cancellation -> Abstain
escape_hatch -> Abstain
base -> Abstain
blocklist -> Abstain
nested_safe -> Secure
(final) -> Secure
```

`RefundChecker` is the ninth checker and was **never invoked**. Executed against the real Safe:

```
status               1 (success)
transactionHash      0x19acc878629177a340212c928deb8328446015188753f09a5bfc28d875497f75
REAL: safe before     1000000000000000000000000
REAL: safe after       999999496888000000000000
REAL: attacker before  0
REAL: attacker after   503112000000000000
```

0.503112 tokens left the Safe for the attacker purely through the refund leg of a transaction the live service called `secure`. `baseGas` is attacker-chosen and unbounded, so this scales linearly to the whole balance — the on-chain figure is small only because it is `(gasUsed + baseGas) * gasPrice` with the modest values above.

### Verbatim outcome — the native-currency refund leg (`gasToken == 0`, the case `refund.rs`'s own TODO says nothing inspects)

Request: same shape, `gasToken` zero, `baseGas` 1e9, `gasPrice` 1e11, `refundReceiver` the attacker.

```
{"verdict":"secure"}
```

Executed against the real Safe. Safe.sol caps the native refund at `min(gasPrice, tx.gasprice)`, so the attacker relays their own transaction at 100 gwei — which a proposer who is also the relayer simply chooses:

```
status               1 (success)
safe ETH before:     999999999991999975104
safe ETH after:      899999688791999975104
native refund paid to the attacker: 100.0003112 ETH
```

100 ETH out of the Safe, to an address the proposer picked, on a `secure` verdict. The `tx.gasprice` cap bounds the multiplier but not `baseGas`, so it does not bound the loss.

### Verdict

**Reproduced end-to-end**, both legs, on the first attempt. The finding's central structural claim — that an affirmer ahead of `RefundChecker` ends the chain before the refund leg is looked at — is visible directly in the service's own trace, and the value really leaves a real Safe in both the ERC-20 and the native case. Nothing in the scenario requires an unusual operator action: the refund parameters are ordinary fields of a relayed Safe transaction, fully under the proposer's control (A2).

Certainty **96% → 99%**. Severity **Critical / Critical**, unchanged.
