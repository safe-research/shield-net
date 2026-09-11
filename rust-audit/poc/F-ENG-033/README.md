# PoC — F-ENG-033 (Critical, 84%)

**`AddressPoisoningChecker` affirms `secure` from event history on an attacker-chosen `to`, and never
inspects `transaction.value`.**

> **Never compiled.** No Rust toolchain on the audit host (`rust-audit/state/baseline.md` §1).

## Apply and run

```bash
cat rust-audit/poc/F-ENG-033/append-to-src-checkers-address_poisoning.rs \
  >> crates/sentinel-engine/src/checkers/address_poisoning.rs
cargo test -p sentinel-engine poc_f_eng_033
git checkout -- crates/sentinel-engine/src/checkers/address_poisoning.rs
```

Hermetic. The finding's own Trigger B calls for an Anvil deployment of a contract `T`; that is
unnecessary in-process, because **the only thing `T` contributes is one log**, and a mocked provider can
supply it directly. Keep the Anvil version for the external corpus; use this one for CI.

## What a run means

| Test | Unfixed | Fixed | Reading |
| --- | --- | --- | --- |
| `..._affirms_a_value_bearing_call_today` | **passes** | fails | Pins variant (a): `value` is never read. |
| `..._affirms_from_self_emitted_history_today` | **passes** | fails | Pins variant (b): the evidence pool is attacker-supplied. |
| `..._control_a_zero_value_transfer_to_an_established_recipient` | passes | **must still pass** | Guards against over-correction: an exact-match recipient on a zero-value call must not become a denial. |
| `..._a_value_bearing_call_must_not_be_affirmed` | **fails** | passes | Variant (a). |
| `..._a_self_emitted_history_must_not_affirm` | **fails** | passes | Variant (b). **Not closed by remediation option 1** — see below. |

## The fixtures, written out

Mocked provider on chain **`0x5afe` (23294)**; `address_poisoning_lookback_blocks = 50000`;
`address_poisoning_max_block_range = None` (one `eth_getLogs` call, hence one queued response).
Request `block = 22020096`; the seeded log sits at `22020095`, inside `[block - 50000, block]`.

### Variant (a) — a real token, `value` unread

| Field | Value | Why |
| --- | --- | --- |
| `chainId` | `0x5afe` | Must equal the provider's, or `address_poisoning.rs:312-319` abstains. |
| `safe` | `0x5aFE3855358E112B5647B952709E6165e1c1eEEe` | |
| `to` | `0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48` (USDC) | The "token". The evidence filter is `address = transaction.to`. |
| `value` | `0x3635c9adc5dea00000` (1000 ETH) | **Never read.** |
| `data` | `transfer(0x1111…1111, 1)` | Amount must be non-zero, or `decode_target` returns `None` (`address_poisoning.rs:122-124`). |
| `operation` | `0` (`Call`) | `DelegateCall` is excluded (`address_poisoning.rs:118-120`). |
| gas/refund fields | all zero | Isolates the `value` defect from F-ENG-031's. |
| `nonce` | `0x2a` | |

Seeded evidence: one `Transfer` log, `address = USDC`, `from = safe`, `to = 0x1111…1111`, `amount = 1000`.

**Verdict today:** `secure`. Whether the 1000 ETH actually leaves the Safe depends on USDC's `transfer`
being `payable` — for a canonical ERC-20 it is not, so this variant is best read as *affirmation without
evidence about `value`*, and as the setup for (b).

### Variant (b) — the evidence pool is manufactured; a real drain

Identical, except:

| Field | Value |
| --- | --- |
| `to` | `0x7777…7777` — the attacker's own contract `T` |
| `data` | `transfer(0x8888…8888, 1)` |

Seeded evidence: one `Transfer` log, `address = T`, `from = safe`, `to = 0x8888…8888`, `amount = 1`.

`T`, for the Anvil/corpus version:

```solidity
contract T {
    event Transfer(address indexed from, address indexed to, uint256 amount);
    function seed(address safe, address x) external { emit Transfer(safe, x, 1); }
    function transfer(address, uint256) external payable returns (bool) { return true; }
}
```

Call `T.seed(SAFE, 0x8888…8888)` once at any block inside the lookback window, then propose. `T::transfer`
is **`payable`**, so `value` genuinely moves and the Safe's whole native balance goes to `T`.

Four guards that do *not* stop this, each checked:

- Nothing verifies `to` has code, a `totalSupply`, or any deployment age — **no `eth_call` is made
  anywhere in the crate** (`address_poisoning.rs:116-139`).
- `decode_target_and_amount` checks only `topics[0]` against the two event signature hashes
  (`address_poisoning.rs:275-285`), and the filter's `address` *is* `transaction.to`, so a self-emitted
  log is indistinguishable from a genuine one.
- Only `amount == 0` is skipped (`:215-217`); `amount = 1` costs one unit of a token the attacker invented.
- The window is `[block - lookback, block]` and the attacker seeds `T` **before** proposing, so the log
  is inside it by construction.

This is stronger than the `transferFrom` forgery the module docs already acknowledge
(`address_poisoning.rs:26-39`): that one assumes a *real* token and an existing allowance. This needs
neither.

**Full-chain walk for both variants** (`main.rs:57-73`): Cancellation abstains → EscapeHatch abstains
(wrong selector) → Base abstains (`to != safe`, plain `Call`) → Blocklist abstains (fresh address) →
Nested abstains → ExcessiveApproval abstains (a `ValueTransfer` effect is not an approval,
`excessive_approval.rs:20-31`) → Cow abstains → Staking abstains → Refund abstains (`gas_price == 0`) →
**AddressPoisoning affirms.** Position 10 of 10, so nothing follows to correct it.

## Remediation check (QA-ENG)

- **Option 1 (`require tx.value.is_zero` in `decode_target`) — sound for variant (a), and it closes
  nothing else.** Set `value` to zero in variant (b) and the affirmation still stands on forged evidence.
  Test 5 in this PoC exists precisely to stop option 1 being mistaken for a fix; do not close the
  finding on it.
- **Option 2 (downgrade `ExactMatch` from `Secure` to `Abstain` — make the checker deny-only) — sound,
  closes both variants, and is the option I would take.** Prior history is evidence that a recipient is
  *not* a poisoned lookalike; it is not evidence that the rest of the transaction is safe. This is
  verbatim the argument `refund.rs:60-67` already makes about the *same delegate*, so the codebase
  already contains the reasoning — applied to one caller and not the other. Cost: the sentinel abstains
  on transfers to established recipients, i.e. loses the vote on a class it currently votes on. Under
  `sentinel/src/service.rs:173-179` that drops the request unanswered rather than voting wrongly.
- **Option 3 (require `transaction.to` to have independent standing) — sound but expensive and
  incomplete.** A code-size / deployment-age probe costs an `eth_call` per request on a checker that is
  already the RPC hot spot (see F-ENG-009: the lookback fan-out has no bound), and a determined attacker
  can age a contract. A token allow-list is cheaper and stricter, at the cost of per-chain maintenance —
  it is what `CowChecker` and `StakingChecker` already do for their own addresses, so it is consistent
  with the crate.
- **Option 4 (per-log provenance: count only `Transfer` logs whose originating transaction was sent by
  the Safe) — sound and the real fix, as the module's own docs say (`address_poisoning.rs:38-39`).** It
  closes variant (b) and the documented `transferFrom` forgery at once. Cost: an `eth_getTransactionByHash`
  (or receipt fetch) per candidate log, which multiplies the RPC fan-out by the log count — unacceptable
  until F-ENG-009's missing bound is fixed. Sequence it after that.
- **Recommended combination: option 2 now, option 4 later.** Option 2 is a one-line change that closes
  both variants immediately and costs only affirmations the engine is not entitled to make; option 4
  restores the affirmation safely once the RPC budget exists.
- **Missing test hook — the hook exists (`Provider::mocked`) and the existing suite deliberately does not
  use it.** `address_poisoning.rs:383-386` states the policy in a comment: "Only the chain-independent
  pure helpers below are unit-tested here — `AGENTS.md`'s 'no unit tests for checkers' rule is about a
  checker's verdicts, which the `sentinel-test-vectors` corpus covers." That corpus is unavailable (A8),
  and for variant (b) it is also the *wrong* oracle: reproducing it in a corpus needs a live chain with
  an attacker contract deployed, whereas in-process it needs one queued log. Recommend the rule be
  narrowed to "no unit tests for verdicts that depend on real chain history", which still permits
  everything in this file.
- **Where the fix belongs: the checker.** The `RuleId` mapping is fine — R-4.3 is the right rule and
  `TargetKind::rule` already selects it correctly. The combinator (F-ENG-044) is what makes an
  affirmation here decide the whole transaction, and remains a separate fix.
