// SPDX-License-Identifier: GPL-3.0-only
pragma solidity ^0.8.30;

library Secp256k1 {
    struct Point {
        uint256 x;
        uint256 y;
    }

    function add(Point memory p, Point memory q) internal pure returns (Point memory r) {
        // TODO
        r.x = p.x + q.x;
        r.y = p.y + q.y;
    }
}
