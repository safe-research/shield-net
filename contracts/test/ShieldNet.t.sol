// SPDX-License-Identifier: UNLICENSED
pragma solidity ^0.8.13;

import {Test} from "@forge-std/Test.sol";
import {ShieldNet} from "@/ShieldNet.sol";

contract ShieldNetTest is Test {
    ShieldNet public shield;

    function setUp() public {
        shield = new ShieldNet();
    }

    function test_Nothing() public view {
        assertNotEq(address(shield), address(0));
    }
}
