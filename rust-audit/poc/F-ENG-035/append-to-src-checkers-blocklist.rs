// PoC for F-ENG-035 — the blocklist is applied only to the top-level `to`, so
// R-4.6 misses token recipients, approval spenders, batch sub-calls and the
// refund receiver.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/blocklist.rs`. It is a sibling of the
// existing `#[cfg(test)] mod tests` (three tests, all on the top-level `to`).
//
// RUN: cargo test -p sentinel-engine poc_f_eng_035
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_035 {
    use super::*;
    use crate::{
        contracts::bindings::{erc20::{approveCall, transferCall}, multi_send},
        engine::Operation,
    };
    use alloy::{
        primitives::{Bytes, U256, address},
        sol_types::SolCall as _,
    };

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    const USDC: Address = address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
    /// A canonical **call-only** MultiSend deployment (`multi_send.rs:53-57`).
    const MULTI_SEND: Address = address!("0x40A2aCCbd92BCA938b02010E17A5b8929b49130D");
    /// The operator's configured R-4.6 target.
    const FLAGGED: Address = address!("0xBadBadBadBadBadBadBadBadBadBadBadBadBad0");

    fn checker() -> BlocklistChecker {
        BlocklistChecker::new([FLAGGED])
    }

    fn base() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: USDC,
            value: U256::ZERO,
            data: Bytes::new(),
            operation: Operation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    /// Position A — the flagged address is the ERC-20 *recipient*; `to` is
    /// the token contract.
    fn erc20_recipient() -> SafeTransaction {
        SafeTransaction {
            data: transferCall {
                to: FLAGGED,
                amount: U256::from(1_000_000_000u64),
            }
            .abi_encode()
            .into(),
            ..base()
        }
    }

    /// Position B — the flagged address is the `approve` *spender*.
    fn approval_spender() -> SafeTransaction {
        SafeTransaction {
            data: approveCall {
                spender: FLAGGED,
                amount: U256::from(1_000_000_000u64),
            }
            .abi_encode()
            .into(),
            ..base()
        }
    }

    /// Position C — the flagged address is a MultiSend *sub-call*
    /// destination; `to` is the MultiSend deployment.
    ///
    /// The packed entry layout is `uint8 operation | address to |
    /// uint256 value | uint256 dataLength | bytes data`
    /// (`multi_send.rs:83-89`). One entry: a plain `Call` sending 1 wei to
    /// the flagged address with empty calldata.
    fn multisend_sub_call() -> SafeTransaction {
        let mut entry = vec![Operation::Call as u8];
        entry.extend_from_slice(FLAGGED.as_slice());
        entry.extend_from_slice(&U256::from(1u64).to_be_bytes::<32>());
        entry.extend_from_slice(&U256::ZERO.to_be_bytes::<32>());

        SafeTransaction {
            to: MULTI_SEND,
            operation: Operation::DelegateCall,
            data: multi_send::multiSendCall {
                transactions: Bytes::from(entry),
            }
            .abi_encode()
            .into(),
            ..base()
        }
    }

    /// Position D — the flagged address is the `refundReceiver`.
    fn refund_receiver() -> SafeTransaction {
        SafeTransaction {
            safe_tx_gas: U256::from(100_000u64),
            base_gas: U256::from(21_000u64),
            gas_price: U256::from(1u64),
            gas_token: USDC,
            refund_receiver: FLAGGED,
            ..base()
        }
    }

    /// (0) CONTROL — the one position that *is* covered today. Must keep
    /// passing after any fix.
    #[tokio::test]
    async fn poc_f_eng_035_control_the_top_level_to_is_denied() {
        let transaction = SafeTransaction {
            to: FLAGGED,
            ..base()
        };

        assert_eq!(
            checker()
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_6KnownMaliciousTarget,
            },
        );
    }

    /// (1) PINS TODAY'S BEHAVIOUR. Passes on unfixed code: all four
    /// positions abstain.
    #[tokio::test]
    async fn poc_f_eng_035_every_other_position_abstains_today() {
        for (position, transaction) in [
            ("erc20 recipient", erc20_recipient()),
            ("approve spender", approval_spender()),
            ("multisend sub-call", multisend_sub_call()),
            ("refund receiver", refund_receiver()),
        ] {
            assert_eq!(
                checker()
                    .check(&transaction, &CheckContext::default())
                    .await,
                Verdict::Abstain,
                "unfixed behaviour ({position}): only `transaction.to` is compared"
            );
        }
    }

    /// (2) THE REGRESSION TEST. **Expected to FAIL on unfixed code**, on the
    /// first iteration (`erc20 recipient`).
    ///
    /// Charter § 2.4 defines the target address as "the address that
    /// receives value or tokens, is granted approvals or permissions, or
    /// otherwise receives economically relevant effects from the
    /// transaction; not merely an intermediate contract address called by
    /// the Safe transaction". All four positions above are that address;
    /// none of them is `transaction.to`.
    #[tokio::test]
    async fn poc_f_eng_035_every_address_the_transaction_reaches_must_be_checked() {
        for (position, transaction) in [
            ("erc20 recipient", erc20_recipient()),
            ("approve spender", approval_spender()),
            ("multisend sub-call", multisend_sub_call()),
            ("refund receiver", refund_receiver()),
        ] {
            assert_eq!(
                checker()
                    .check(&transaction, &CheckContext::default())
                    .await,
                Verdict::Insecure {
                    rule: RuleId::R4_6KnownMaliciousTarget,
                },
                "{position}: § 2.4's target address is not `transaction.to`"
            );
        }
    }
}
