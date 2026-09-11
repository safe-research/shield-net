# PoC — F-XC-052 (`decode_multi_send` zeroes `chain_id`)

**Never compiled, never run.** No Rust toolchain (A9 FALSE). Commit `2893917`.

## What it is and is not

A **characterisation test**, not an exploit. C-ENG-B confirmed the finding as a _latent_ defect by enumerating every non-test consumer of `sub_transactions` and `decode_multi_send_call` (`staking.rs:93`, `cow.rs:429`, `base.rs:206`, `target_effects.rs:47`) and showing that none reads a zeroed field on a sub-call. There is nothing to exploit today, and a PoC claiming otherwise would be dishonest.

Its value is the other thing a PoC can do: pin the current shape so the choice is deliberate and any change is visible. `crates/sentinel-engine/src/contracts/multi_send.rs` has **zero** tests today (`rust-audit/state/baseline.md` §5), and this file is the trap that `F-ENG-031`'s and `F-ENG-035`'s remediations would spring — a new refund-aware or chain-id-aware check added to a checker that runs over `sub_transactions` would silently read zeros and pass.

## Install and run

Append `append-to-crates-sentinel-engine-src-contracts-multi_send.rs` to the end of `crates/sentinel-engine/src/contracts/multi_send.rs` (it brings its own `mod tests`).

```sh
cargo test -p sentinel-engine multi_send::tests::qa_xc_052
```

## Reading the result

| Outcome | Meaning |
| --- | --- |
| **Passes** | The finding is confirmed as filed and now has an `E1` characterisation. `sub.chain_id == 0` while `tx.chain_id == 100`. |
| `…chain_id is no longer zeroed…` **fires** | Somebody fixed it. Close `F-XC-052` and delete the assertion. |
| `…a_truncated_blob…` **fails** | A separate, more serious bug: the packed-blob cursor over-reads. Investigate immediately — the blob is attacker-controlled under A2. |

## Fixtures (attacker-controlled under A2, so spelled out literally)

- Outer transaction: `chain_id = 100` (Gnosis, A10), `safe = 0x0101…01`, `to = 0x218543288004CD07832472D464648173c77D7eB7` (a canonical `V150Plus` MultiSend from the table at `multi_send.rs:27-68`), `operation = DelegateCall`, `safe_tx_gas = 100000`, `base_gas = 21000`, `gas_price = 7`, `gas_token = refund_receiver = 0x0202…02`, `nonce = 42`.
- `data` = `multiSend(bytes)` ABI-encoding of one packed entry: `00 ‖ 0x0202…02 ‖ uint256(1000) ‖ uint256(4) ‖ deadbeef`.

## Remediation check (QA-XC) — one option is unsound as written

`F-XC-052` lists three options and **option 1 as filed is wrong**, for the reason C-ENG-B gives in its scope correction:

- **Option 1 ("propagate the outer values": `chain_id`, `nonce`, and the four refund fields) is sound only for `chain_id`.** A MultiSend sub-call has no refund leg and no nonce of its own — both are properties of the enclosing `execTransaction`. Copying `gas_price` and `refund_receiver` onto five sub-calls would make one refund look like five, which is a worse bug than the zeroing and would break any future refund-summing check in the opposite direction. **Take the `chain_id` half only**, and document the other five as deliberately not-applicable.
- **Option 2 (a distinct `SubCall { to, value, data, operation }` type) is sound and is the fix that removes the class rather than the instance.** Its stated cost is accurate — about a dozen call sites in `staking.rs`, `cow.rs` and `base.rs` take `&SafeTransaction`. If the team is willing to pay it, this is the better fix and it makes option 1 unnecessary.
- **Option 3 (a comment) is the zero-cost floor** and is honestly described as such.

A fourth option neither file lists, and the cheapest defence in depth: have `AddressPoisoningChecker` treat `chain_id == 0` as a malformed input rather than as a mismatch to abstain on. No real transaction has `chain_id == 0`, so the only thing that reaches that branch is a synthesised one — which is exactly the bug in both `F-ENG-032` and this finding. That does not fix the synthesis, but it converts a silent dead checker into something a `warn!`-level line (and, with `F-XC-010`'s counter, a metric) would surface.
