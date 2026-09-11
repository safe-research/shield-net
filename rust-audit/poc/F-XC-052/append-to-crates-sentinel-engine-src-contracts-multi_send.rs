// PoC for F-XC-052 — `decode_multi_send` synthesises sub-transactions with
// `chain_id` (and `nonce`, and every refund field) zeroed.
//
// NOT COMPILED, NOT RUN — no Rust toolchain on the audit machine.
//
// Append to the end of `crates/sentinel-engine/src/contracts/multi_send.rs`.
// That file currently has ZERO tests (rust-audit/state/baseline.md §5), so
// this adds the whole `mod tests` block.
//
// Run:  cargo test -p sentinel-engine multi_send::tests::qa_xc_052
//
// This is a characterisation test, not an exploit: C-ENG-B established by
// exhaustive enumeration of every non-test consumer that the defect is LATENT
// today — no checker reads a zeroed field on a sub-call. Its value is that it
// pins which fields are populated and which are not, so the choice becomes
// deliberate and any change to it is visible in a diff. It is also the trap
// that F-ENG-031's and F-ENG-035's remediations would spring: a new refund or
// chain-id check added to a checker that runs over `sub_transactions` would
// silently read zeros.

#[cfg(test)]
mod tests {
    use super::*;

    const SAFE: Address = Address::new([1u8; 20]);
    const RECIPIENT: Address = Address::new([2u8; 20]);
    /// A canonical MultiSend deployment from the table at
    /// `crates/sentinel-engine/src/contracts/multi_send.rs:27-68`
    /// (`V150Plus`, delegate calls allowed).
    const MULTI_SEND: Address = address!("218543288004CD07832472D464648173c77D7eB7");
    /// Gnosis Chain, the deployment target per assumption A10.
    const CHAIN_ID: u64 = 100;

    /// One packed MultiSend entry: `uint8 operation ‖ address to ‖
    /// uint256 value ‖ uint256 dataLength ‖ bytes data`
    /// (the layout documented at `multi_send.rs:80-90`).
    fn packed(operation: u8, to: Address, value: U256, data: &[u8]) -> Vec<u8> {
        let mut out = vec![operation];
        out.extend_from_slice(to.as_slice());
        out.extend_from_slice(&value.to_be_bytes::<32>());
        out.extend_from_slice(&U256::from(data.len()).to_be_bytes::<32>());
        out.extend_from_slice(data);
        out
    }

    /// A fully populated outer transaction — every field a real
    /// `execTransaction` carries, including a live refund leg.
    fn outer(transactions: Vec<u8>) -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(CHAIN_ID),
            safe: SAFE,
            to: MULTI_SEND,
            value: U256::ZERO,
            data: multi_send::multiSendCall {
                transactions: transactions.into(),
            }
            .abi_encode()
            .into(),
            operation: Operation::DelegateCall,
            safe_tx_gas: U256::from(100_000u64),
            base_gas: U256::from(21_000u64),
            gas_price: U256::from(7u64),
            gas_token: RECIPIENT,
            refund_receiver: RECIPIENT,
            nonce: U256::from(42u64),
        }
    }

    #[test]
    fn qa_xc_052_sub_calls_carry_a_chain_id_no_real_transaction_could_have() {
        let blob = packed(0, RECIPIENT, U256::from(1_000u64), &[0xde, 0xad, 0xbe, 0xef]);
        let tx = outer(blob);

        let subs = sub_transactions(&tx);
        assert_eq!(subs.len(), 1, "the batch decoded as one sub-call");
        let sub = &subs[0];

        // THE DEFECT. The outer transaction is on Gnosis; its sub-call claims
        // to be on chain 0. `AddressPoisoningChecker`'s guard
        // (crates/sentinel-engine/src/checkers/address_poisoning.rs:311-319)
        // abstains on exactly this mismatch, which is what made
        // `RefundChecker` dead code (F-ENG-032). Nothing routes a sub-call to
        // that checker today (`main.rs:72` registers it against the top-level
        // transaction only), which is why this is latent rather than live.
        assert_eq!(
            sub.chain_id,
            U256::ZERO,
            "chain_id is no longer zeroed — F-XC-052 has been fixed; delete this assertion"
        );
        assert_ne!(
            sub.chain_id, tx.chain_id,
            "a sub-call executes on the same chain as its enclosing transaction; \
             this inequality IS the finding"
        );

        // The four refund fields and the nonce, likewise zeroed. C-ENG-B's
        // scope correction applies here and should be respected by any fix:
        // these have no per-sub-call meaning (refund and nonce are properties
        // of the enclosing execTransaction), so they should be documented as
        // deliberately not-applicable rather than propagated. Propagating them
        // would make five sub-calls look like five separate refunds.
        assert_eq!(sub.safe_tx_gas, U256::ZERO);
        assert_eq!(sub.base_gas, U256::ZERO);
        assert_eq!(sub.gas_price, U256::ZERO);
        assert_eq!(sub.gas_token, Address::ZERO);
        assert_eq!(sub.refund_receiver, Address::ZERO);
        assert_eq!(sub.nonce, U256::ZERO);

        // The fields that ARE propagated, and the reason the current
        // consumers stay safe: `value` is not zeroed (multi_send.rs:120), so
        // `staking.rs`'s and `cow.rs`'s `!tx.value.is_zero()` guards remain
        // meaningful on sub-calls. Had `value` been zeroed too, this would be
        // live today and Critical.
        assert_eq!(sub.safe, SAFE);
        assert_eq!(sub.to, RECIPIENT);
        assert_eq!(sub.value, U256::from(1_000u64));
        assert_eq!(sub.data.as_ref(), &[0xde, 0xad, 0xbe, 0xef]);
        assert_eq!(sub.operation, Operation::Call);
    }

    #[test]
    fn qa_xc_052_a_v150plus_self_call_resolves_to_the_safe() {
        // Guards the one piece of version-dependent behaviour in the same
        // function, so a fix to the zeroing cannot silently break it:
        // `to == address(0)` means a self-call on v1.5.0+ (multi_send.rs:113-116).
        let blob = packed(0, Address::ZERO, U256::ZERO, &[]);
        let subs = decode_multi_send(SAFE, &blob, MultiSendVersion::V150Plus).unwrap();

        assert_eq!(subs[0].to, SAFE);

        let subs = decode_multi_send(SAFE, &blob, MultiSendVersion::Legacy).unwrap();
        assert_eq!(subs[0].to, Address::ZERO);
    }

    #[test]
    fn qa_xc_052_a_truncated_blob_is_rejected_rather_than_partially_decoded() {
        // Not part of the finding; included because `multi_send.rs` has no
        // tests at all and this is the other thing a reader of a fix needs
        // pinned. A `dataLength` that overruns the buffer must yield `None`,
        // not a short read (the `Cursor::read` at multi_send.rs:180+).
        let mut blob = packed(0, RECIPIENT, U256::ZERO, &[0x01, 0x02, 0x03, 0x04]);
        blob.truncate(blob.len() - 2);

        assert_eq!(decode_multi_send(SAFE, &blob, MultiSendVersion::Legacy), None);
    }
}
