# F-ENG-041 A first-time recipient with no established history only ever abstains, so a novel-address drain is never denied

| Field | Value |
| --- | --- |
| Status | QA-done |
| Crate and module | sentinel-engine, checkers/address_poisoning.rs |
| Location | crates/sentinel-engine/src/checkers/address_poisoning.rs:303-307, :334-347 |
| Severity | Low / Informational |
| Certainty | 85% (Critic C-ENG-B; E2 ceiling, read-only run) |
| Assumptions involved | A2, A3, A12, A15 |
| Tags | known, verdict-policy, charter |

## Claim

When the candidate recipient has neither a prior interaction of its own nor any established recipient to resemble, `AddressPoisoningChecker` returns `Abstain`. An ERC-20 `transfer` of the Safe's entire balance to a freshly created address therefore produces no denial from any checker — the whole chain abstains and the sentinel drops the request without voting (`crates/sentinel/src/service.rs:176-179`).

This is item `crates/sentinel-engine/src/checkers/address_poisoning.rs:303` in the codebase map's known list and is documented as a follow-up in the code itself, so it is filed tagged `known` at reduced priority.

Charter context: R-4.3's "For novel recipient addresses" clause is precisely about this case and directs the Council to weigh whether the address "has legitimate onchain history consistent with the protocol-recorded purpose", whether it "resembles a prior user address in a way consistent with address poisoning", and whether "standard users conducting comparable transactions would send value to this recipient address". The engine implements only the middle test. The abstention is the safe failure mode (no vote, so no attestation), so the impact is a coverage gap rather than a wrong answer.

## Basis

| # | Claim | Class (E1, E2, I) | Citation | Verbatim quote |
| --- | --- | --- | --- | --- |
| 1 | The TODO states the gap | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:303-307 | Q1 |
| 2 | With no lookalike among the established recipients, the verdict is `Abstain` | E2 | crates/sentinel-engine/src/checkers/address_poisoning.rs:334-347 | Q2 |
| 3 | The Charter's novel-recipient clause lists three signals; only "resembles a prior user address" is implemented | I (Charter text) | safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:599-603 | Q3 |
| 4 | An abstention means the sentinel casts no vote | E2 | crates/sentinel/src/service.rs:173-179 | Q4 |

**Q1** `crates/sentinel-engine/src/checkers/address_poisoning.rs:303-307`

```rust
    /// TODO(follow-up): a first-time-looking recipient with no established
    /// address to compare against still only ever abstains — richer
    /// recipient-quality signals (the candidate's own fund/transaction
    /// history, whether it's an EOA or a contract, and if so its deployment
    /// age) are needed before that case can be safely denied too.
```

**Q2** `crates/sentinel-engine/src/checkers/address_poisoning.rs:334-347`

```rust
            Ok(RecipientLookup::NoExactMatch {
                recipients,
                complete,
            }) => {
                let Some(established) = recipients.iter.find(|&&r| is_lookalike(r, candidate))
                else {
                    tracing::debug!(
                        token = %transaction.to,
                        %candidate,
                        rule = kind.rule.code,
                        "address-poisoning: no established recipient to compare against"
                    );
                    return Verdict::Abstain;
                };
```

**Q3** `safenet-charter@44a1e53:Safenet_Arbitration_Charter.md:599-603`

```text
#### For novel recipient addresses, Council weighs whether

- the recipient address has legitimate onchain history consistent with the protocol-recorded purpose (§ 2.11);
- the recipient address resembles a prior user address in a way consistent with address poisoning;
- standard users conducting comparable transactions would send value to this recipient address (§ 2.9).
```

**Q4** `crates/sentinel/src/service.rs:173-179`

```rust
        let (approve, reason) = match outcome {
            CheckOutcome::Approved => (true, String::new()),
            CheckOutcome::Denied(rule) => (false, rule.to_string),
            CheckOutcome::Unknown => {
                tracing::warn!(%request_id, "engine check failed; dropping request unanswered");
                return (state, Vec::new());
            }
        };
```

## Trigger

`to = <a token the Safe holds>`, `operation = 0`, `value = "0x0"`, `data = transfer(0x<a freshly generated address, no prior Transfer/Approval from this Safe on this token within address_poisoning_lookback_blocks and sharing fewer than 4 leading or 4 trailing nibbles with any address that does>, <the Safe's whole token balance>)`, `chainId` matching the provider, all gas fields zero. Response: `{"verdict":"abstain"}`.

Corpus shape: this vector, plus the two controls the checker does answer — a recipient with prior history (`secure`) and a 4+4-nibble lookalike of one (`insecure R-4.3`).

## Considered and rejected

- **Some other checker denies it.** `BlocklistChecker` only if the address is configured (`blocklist.rs:25`); `ExcessiveApprovalChecker` only for approvals at `U256::MAX` (`excessive_approval.rs:22`); `CowChecker`/`StakingChecker` only for their own shapes. Nothing else looks at a plain transfer's recipient.
- **Abstaining is wrong.** Denying every novel recipient would deny the first payment to every new counterparty, which the code's own comment identifies as the reason for the follow-up (`address_poisoning.rs:15-17`: "novelty alone isn't grounds for denial"). The finding is the gap, not the choice.
- **This is a duplicate of F-ENG-033.** F-ENG-033 is about the _affirmation_ path (`ExactMatch` → `Secure` while ignoring `value` and the provenance of `to`); this is the _abstention_ path.

## Remediation options

1. Implement the two remaining R-4.3 novel-recipient signals: the candidate's own on-chain history (an `eth_getLogs`/`eth_getBalance`/`eth_getCode` probe) and a deployment-age heuristic for contracts, denying only when the candidate has no history at all _and_ the amount is a material fraction of the Safe's balance.
2. Add an amount-relative rule that does not need new RPC: deny when the transfer moves the Safe's entire balance of that token to an address with no in-window history.
3. Leave the verdict as `Abstain` but surface it distinguishably (see the abstain-ambiguity observation in `rust-audit/state/agents/R9.md`), so operators can see how often the engine has no opinion.

Tests to add: the corpus triple above. No code is committed.

## Trail

- Reviewer R9: drafted, self-estimate 95% (fact) / Low severity. Known item per A12.

## Critic (C-ENG-B)

### 1. Per-claim verdicts — all Supported

The TODO at `address_poisoning.rs:303-307` states the gap; `:334-347` returns `Verdict::Abstain` when `recipients.iter.find(|&&r| is_lookalike(r, candidate))` yields `None`; `crates/sentinel/src/service.rs: 173-179` confirms an abstention costs the sentinel its vote entirely (the request is "dropped unanswered"). The `known` tag is correct — this is codebase-map item `address_poisoning.rs:303`.

I re-read Charter R-4.3's novel-recipient clause and R9's characterisation is accurate: three signals are listed, and only "resembles a prior user address in a way consistent with address poisoning" is implemented.

### 2. Severity — corrected from Low to Informational

The finding's own Claim concedes the decisive point: "The abstention is the safe failure mode (no vote, so no attestation), so the impact is a coverage gap rather than a wrong answer." I verified that independently — an `Abstain` produces no attestation and the Guard therefore refuses the transaction, so the drain R9 describes does not execute. Combined with the `known` tag, PROMPT.md §8's Informational row ("hardening, documentation, test gaps, and `known` items") is the correct placement. **Informational**.

One thing worth preserving when the report is compiled: this finding and F-ENG-033 describe opposite failures of the same function — F-ENG-033 is a _false_ `ExactMatch` affirming a drain (Critical), this one is the _absence_ of any evidence abstaining on one (Informational). A reader who sees only the second may conclude the checker fails safe in general. It does not.

### 3. Finding verdict

**Confirmed.** Certainty **85%**. Severity **Informational** (corrected from Low).

## QA (QA-ENG)

**Execution: Not attempted (no toolchain)** — `rust-audit/state/baseline.md` §1. **No PoC written**: the finding is a _coverage gap_ — the engine abstains where the Charter arguably permits a denial — so there is no wrong verdict to reproduce and no assertion a regression test could make until a policy is chosen. The harness for the corpus triple the finding names is in `rust-audit/poc/F-ENG-033/append-to-src-checkers-address_poisoning.rs` (`Provider::mocked` + a queued log), and the "novel recipient" case is simply that file's fixture with an **empty** log page pushed instead of a populated one.

**Certainty unchanged at 85%. Severity unchanged (Low reviewer / Informational final).**

**One thing the report should say plainly.** The behaviour this finding calls a gap is documented and deliberate — `address_poisoning.rs` states that "A novel candidate, with nothing to compare it against, returns `Verdict::Abstain`: novelty alone isn't grounds for denial", and carries a TODO naming the signals that would be needed. That is a defensible position and it is the _opposite_ error direction from the Criticals: abstaining costs a vote, denying wrongly costs bond (`contracts/src/libraries/SentinelOracleRequests.sol:301-304`; see F-ENG-042's `## QA`). Informational is right.

### Remediation check

- **Option 1 (implement the remaining R-4.3 novel-recipient signals) — sound in direction, and the conjunction it proposes is the load-bearing part.** Denying only when the candidate has **no** history at all _and_ the amount is a material fraction of the Safe's balance is what keeps it from becoming a first-payment-to-anyone denial. Cost: `eth_getLogs`/`eth_getBalance`/`eth_getCode` probes per request on the checker that is already the RPC hot spot — **blocked behind F-ENG-009's missing fan-out bound**, same as F-ENG-033 option 4. Sequence accordingly.
- **Option 2 (an amount-relative rule needing no new RPC) — sound, and the best value here.** "Deny when the transfer moves the Safe's **entire balance of that token** to an address with no in-window history" needs one additional datum. Note the honest caveat: the Safe's token balance is _not_ free — it is an `eth_call` to `balanceOf`, which the crate makes nowhere today — so "no new RPC" holds only if the rule is expressed against something already in hand. If it must be a balance, price it with option 1.
- **Option 3 (keep `Abstain` but make it distinguishable) — sound, cheap, and I would ship it first regardless of 1 and 2.** Today "no ERC-20 target to decode", "chain-id mismatch", "lookup failed" and "novel recipient, no evidence" are the same `Abstain` on the wire, so an operator cannot see how often the engine has no opinion or why. That is the same abstain-ambiguity F-ENG-038 option 3 raises for `CowChecker`; fix them together and the engine becomes measurable, which is the precondition for deciding options 1 and 2 on evidence rather than on intuition.
- **Do not take option 1 or 2 before F-ENG-042 is understood.** They add denials to a checker whose _existing_ denial path can already fire on incomplete evidence (F-ENG-042). Widening the denial surface first would compound that.
- **Test hook: exists** (`Provider::mocked`), and this audit's F-ENG-033 PoC demonstrates it end to end.
- **Where the fix belongs: the checker**, with the observability half in the verdict type or the logging layer. Not the combinator or the `RuleId` mapping.
