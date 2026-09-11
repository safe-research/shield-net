// PoC for F-ENG-002 — `ExcessiveApprovalChecker` denies every
// `setApprovalForAll(operator, true)`, a Charter-*conditional* case, so the
// reference engine deterministically votes `insecure` on routine honest
// NFT-marketplace listings.
//
// NOTE THE DIRECTION. Unlike the Criticals in this audit, this is a wrong
// vote in the DENYING direction, against honest traffic. The fixture below is
// an HONEST transaction that gets denied — the opposite shape. It is also the
// error direction that costs the engine's own operator money: see this
// directory's README on Charter § 2.15 and `SentinelOracleRequests.sol`.
//
// HOW TO APPLY: append this whole block, verbatim, to the END of
// `crates/sentinel-engine/src/checkers/excessive_approval.rs`. It is a
// sibling of the existing `#[cfg(test)] mod tests`. F-ENG-036's PoC appends
// to the same file; the two modules do not collide and may both be applied.
//
// RUN: cargo test -p sentinel-engine poc_f_eng_002
//
// NEVER COMPILED — see this directory's README.md.

#[cfg(test)]
mod poc_f_eng_002 {
    use super::*;
    use crate::{
        contracts::bindings::erc721::setApprovalForAllCall,
        engine::{Operation, SafeTransaction},
    };
    use alloy::{
        primitives::{Address, address},
        sol_types::SolCall as _,
    };

    const SAFE: Address = address!("0x5aFE3855358E112B5647B952709E6165e1c1eEEe");
    /// A canonical ERC-721 collection the Safe holds.
    const COLLECTION: Address = address!("0xBC4CA0EdA7647A8aB7C2061c2E118A18a936f13D");
    /// OpenSea's Seaport 1.6 conduit — the operator every listing flow
    /// approves. A canonical marketplace address, not an attacker's.
    const SEAPORT_CONDUIT: Address = address!("0x1E0049783F008A0085193E00003D00cd54003c71");

    /// The honest transaction: the mandatory first step of listing an NFT
    /// from a Safe on any major marketplace.
    fn marketplace_listing_approval() -> SafeTransaction {
        SafeTransaction {
            chain_id: U256::from(1u64),
            safe: SAFE,
            to: COLLECTION,
            value: U256::ZERO,
            data: setApprovalForAllCall {
                operator: SEAPORT_CONDUIT,
                approved: true,
            }
            .abi_encode()
            .into(),
            operation: Operation::Call,
            safe_tx_gas: U256::ZERO,
            base_gas: U256::ZERO,
            gas_price: U256::ZERO,
            gas_token: Address::ZERO,
            refund_receiver: Address::ZERO,
            nonce: U256::from(42u64),
        }
    }

    /// (1) PINS TODAY'S BEHAVIOUR. Passes on unfixed code.
    #[tokio::test]
    async fn poc_f_eng_002_denies_a_routine_marketplace_listing_today() {
        assert_eq!(
            ExcessiveApprovalChecker
                .check(&marketplace_listing_approval(), &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
            "unfixed behaviour: `EffectKind::OperatorApproval {{ approved }} => approved` denies on \
             the boolean alone, with no reference to the interaction the approval belongs to"
        );
    }

    /// (2) CONTROL — the ERC-20 arm is correct and must not be touched.
    ///
    /// Charter § 2.5 makes max-`uint256` ERC-20 approval *unconditionally*
    /// functionally unlimited, and R-4.5's "immediate failure" branch names
    /// exactly that case. `excessive_approval.rs:22` implements it exactly.
    /// Any fix for this finding that changes this test's outcome has
    /// over-corrected.
    #[tokio::test]
    async fn poc_f_eng_002_control_the_erc20_max_arm_stays_a_denial() {
        use crate::contracts::bindings::erc20::approveCall;

        let transaction = SafeTransaction {
            data: approveCall {
                spender: SEAPORT_CONDUIT,
                amount: U256::MAX,
            }
            .abi_encode()
            .into(),
            ..marketplace_listing_approval()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
        );
    }

    /// (3) CONTROL — revocation must stay non-denying. Passes today
    /// (implied by `excessive_approval.rs:23` but pinned by the existing
    /// suite too); restated here so the pair `approved: true` /
    /// `approved: false` is visible in one place.
    #[tokio::test]
    async fn poc_f_eng_002_control_revocation_abstains() {
        let transaction = SafeTransaction {
            data: setApprovalForAllCall {
                operator: SEAPORT_CONDUIT,
                approved: false,
            }
            .abi_encode()
            .into(),
            ..marketplace_listing_approval()
        };

        assert_eq!(
            ExcessiveApprovalChecker
                .check(&transaction, &CheckContext::default())
                .await,
            Verdict::Abstain,
        );
    }

    /// (4) THE REGRESSION TEST. **Expected to FAIL on unfixed code.**
    ///
    /// Charter § 2.5 makes an ERC-721/1155 operator approval functionally
    /// unlimited "unless plausibly required for the stated interaction",
    /// where the stated interaction is determined from onchain data (§ 2.8)
    /// and protocol-recorded purpose (§ 2.11). A *conditional* rule cannot be
    /// discharged by an unconditional denial, and R-4.5's immediate-failure
    /// branch names only the max-`uint256` ERC-20 case.
    ///
    /// The assertion is `!= Insecure` rather than `== Abstain` so that it is
    /// satisfied by remediation option 1 (abstain) **and** by options 2 and 3
    /// (deny only an unrecognised operator) — the fix must not be prejudged.
    ///
    /// Failure output on unfixed code:
    ///   assertion failed: verdict != Verdict::Insecure { rule: R4_5ExcessiveApproval }
    #[tokio::test]
    async fn poc_f_eng_002_a_conditional_rule_must_not_be_an_immediate_failure() {
        let verdict = ExcessiveApprovalChecker
            .check(&marketplace_listing_approval(), &CheckContext::default())
            .await;

        assert_ne!(
            verdict,
            Verdict::Insecure {
                rule: RuleId::R4_5ExcessiveApproval,
            },
            "§ 2.5 conditions operator approval-for-all on the stated interaction; the engine \
             has gathered no evidence about that interaction and must not deny on the boolean"
        );
    }
}
