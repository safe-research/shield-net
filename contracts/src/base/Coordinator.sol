// SPDX-License-Identifier: GPL-3.0-only
pragma solidity ^0.8.30;

import {Secp256k1} from "@/lib/Secp256k1.sol";

contract Coordinator {
    struct KeyGenParticipant {
        Secp256k1.Point share;
    }

    struct KeyGenProof {
        Secp256k1.Point r;
        uint256 mu;
    }

    struct KeyGen {
        uint256 count;
        uint256 threshold;
        Secp256k1.Point key;
        KeyGenParticipant[] participants;
    }

    error InvalidKeyGenParameters();

    mapping(uint256 id => KeyGen) private $keygen;

    /// @dev Initiate a distributed key generation ceremony.
    function _keygen(uint256 id, uint256 count, uint256 threshold) internal {
        KeyGen storage keygen = $keygen[id];
        require(keygen.count == 0 && count >= threshold && threshold > 1, InvalidKeyGenParameters());

        keygen.count = count;
        keygen.threshold = threshold;
    }

    /// @dev Submit a proof and commitment for a key generation participant.
    ///      This corresponds to Round 1 of the FROST key generation protocol.
    function _keygen1(uint256 id, KeyGenProof memory proof, Secp256k1.Point[] memory commitment)
        internal
        returns (uint256 i)
    {}
}
