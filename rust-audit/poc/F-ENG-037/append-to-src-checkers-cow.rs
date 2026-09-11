// PoC for F-ENG-037 — the CoW TWAP approval tolerance is sized by an
// attacker-chosen `n`, so a near-unlimited relayer approval is rated `secure`.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/cow.rs`. It is a SIBLING of that
// file's `#[cfg(test)] mod tests`, not nested inside it, so it cannot reach
// that module's private helpers (`multisend`, `pack`, `twap_create_data`) —
// they are re-declared here. Constants such as `GP_V2_VAULT_RELAYER` and
// `TWAP_HANDLER` are file-level items of `cow.rs` and DO come through
// `use super::*`.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_037
//
// No HTTP is issued: the presignature path needs a `setPreSignature` call in
// the batch (`cow.rs:283-293`) and this batch has none, so `CowChecker::new()`
// is safe to use offline.
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_037 {
    use super::*;
    use crate::contracts::bindings::{cow::ConditionalOrderParams, multi_send};

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    const TOKEN: Address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    /// A canonical MultiSend deployment (`multi_send.rs:28-32`).
    const MULTI_SEND: Address = address!("0x218543288004CD07832472D464648173c77D7eB7");

    /// `uint8 operation | address to | uint256 value | uint256 dataLength |
    /// bytes data` (`multi_send.rs:83-89`).
    fn pack(operation: Operation, to: Address, data: &[u8]) -> Vec<u8> {
        let mut out = vec![operation as u8];
        out.extend_from_slice(to.as_slice());
        out.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());
        out.extend_from_slice(&U256::from(data.len()).to_be_bytes::<32>());
        out.extend_from_slice(data);
        out
    }

    fn multisend_calldata(sub_txs: &[Vec<u8>]) -> Bytes {
        let transactions: Vec<u8> = sub_txs.iter().flatten().copied().collect();
        Bytes::from(
            multi_send::multiSendCall {
                transactions: Bytes::from(transactions),
            }
            .abi_encode(),
        )
    }

    fn approve_calldata(amount: U256) -> Vec<u8> {
        approveCall {
            spender: GP_V2_VAULT_RELAYER,
            amount,
        }
        .abi_encode()
    }

    /// A TWAP `createWithContext` using the canonical handler and factory —
    /// both are enforced (`cow.rs:536`), so neither may be substituted.
    fn twap_create_calldata(part_sell_amount: U256, n: U256) -> Vec<u8> {
        createWithContextCall {
            params: ConditionalOrderParams {
                handler: TWAP_HANDLER,
                salt: B256::ZERO,
                staticInput: Bytes::from(
                    TwapData {
                        sellToken: TOKEN,
                        buyToken: address!("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"),
                        receiver: SAFE,
                        partSellAmount: part_sell_amount,
                        minPartLimit: U256::ZERO,
                        t0: U256::ZERO,
                        n,
                        t: U256::ZERO,
                        span: U256::ZERO,
                        appData: B256::ZERO,
                    }
                    .abi_encode(),
                ),
            },
            factory: CURRENT_BLOCK_TIMESTAMP_FACTORY,
            data: Bytes::new(),
            dispatch: true,
        }
        .abi_encode()
    }

    /// The 2-call batch, as a MultiSend delegatecall.
    fn twap_batch(approved: U256, part_sell_amount: U256, n: U256) -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: MULTI_SEND,
            value: U256::ZERO,
            data: multisend_calldata(&[
                pack(Operation::Call, TOKEN, &approve_calldata(approved)),
                pack(
                    Operation::Call,
                    COMPOSABLE_COW,
                    &twap_create_calldata(part_sell_amount, n),
                ),
            ]),
            operation: Operation::DelegateCall,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    async fn check(transaction: &SafeTransaction) -> Verdict {
        CowChecker::new()
            .check(transaction, &CheckContext::default())
            .await
    }

    /// The degenerate order: `partSellAmount = 0` makes
    /// `total = 0.checked_mul(n) = Some(0)`, while `n = U256::MAX` makes the
    /// tolerance `max_approval_for_twap_total(0, MAX) = MAX - 1`. An approval
    /// of exactly `MAX - 1` then fails the `>` comparison and is affirmed.
    fn degenerate_order_batch() -> SafeTransaction {
        twap_batch(U256::MAX - U256::from(1u64), U256::ZERO, U256::MAX)
    }

    /// The plausible-looking variant, for a corpus that wants an order that
    /// does not look degenerate: `partSellAmount = 1`, `n = 2^128` gives
    /// `total = 2^128` and a ceiling of `2^129 - 1` — functionally unlimited
    /// for any 18-decimal token, while every field still reads like a real
    /// TWAP.
    fn plausible_order_batch() -> SafeTransaction {
        let n = U256::from(1u64) << 128;
        twap_batch((n << 1) - U256::from(1u64), U256::from(1u64), n)
    }

    /// (0) CONTROL — the crate's own expectation, restated. A real 3-part
    /// order selling 30 units, approved for exactly 30, is affirmed; approved
    /// for 1000 it is denied (`cow.rs:879-943`). Must keep passing.
    #[tokio::test]
    async fn poc_f_eng_037_control_a_real_order_behaves_as_documented() {
        assert_eq!(
            check(&twap_batch(U256::from(30u64), U256::from(10u64), U256::from(3u64))).await,
            Verdict::Secure,
        );
        assert_eq!(
            check(&twap_batch(U256::from(1_000u64), U256::from(10u64), U256::from(3u64))).await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
        );
    }

    /// (1) THE ARITHMETIC, ISOLATED. Passes on unfixed code; the clearest
    /// single statement of the defect, with no calldata involved.
    #[test]
    fn poc_f_eng_037_the_tolerance_is_sized_by_n() {
        assert_eq!(
            max_approval_for_twap_total(U256::ZERO, U256::MAX),
            U256::MAX - U256::from(1u64),
            "an order selling zero tokens tolerates an approval of 2^256 - 2"
        );
    }

    /// (2) PINS TODAY'S BEHAVIOUR.
    #[tokio::test]
    async fn poc_f_eng_037_affirms_a_near_unlimited_relayer_approval_today() {
        assert_eq!(
            check(&degenerate_order_batch()).await,
            Verdict::Secure,
            "unfixed behaviour: total = 0, ceiling = 2^256 - 2, approved = 2^256 - 2"
        );
        assert_eq!(check(&plausible_order_batch()).await, Verdict::Secure);
    }

    /// (3) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// Charter § 2.5 / R-4.5: an approval of `2^256 - 2` on a real token
    /// materially exceeds what an order selling **zero** tokens plausibly
    /// needs. Satisfied by remediation options 1, 2 or 3.
    #[tokio::test]
    async fn poc_f_eng_037_a_degenerate_order_must_not_size_the_tolerance() {
        assert_eq!(
            check(&degenerate_order_batch()).await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
        );
    }

    /// (4) THE SAME, WITHOUT A DEGENERATE ORDER. **Expected to FAIL on
    /// unfixed code.**
    ///
    /// Remediation option 2 alone (reject `partSellAmount == 0`) closes test
    /// (3) but **not** this one: here `partSellAmount = 1` and `n = 2^128`
    /// are both non-zero and internally consistent. Only a bound on `n`
    /// itself (option 1 or 3) closes it.
    #[tokio::test]
    async fn poc_f_eng_037_a_large_part_count_must_not_size_the_tolerance() {
        assert_ne!(check(&plausible_order_batch()).await, Verdict::Secure);
    }
}
