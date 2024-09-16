// Copyright 2024 RISC Zero, Inc.
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// SPDX-License-Identifier: Apache-2.0

pragma solidity ^0.8.20;

import {RiscZeroCheats} from "risc0/test/RiscZeroCheats.sol";
import {console2} from "forge-std/console2.sol";
import {Test} from "forge-std/Test.sol";
import {IRiscZeroVerifier} from "risc0/IRiscZeroVerifier.sol";
import {UserLiquidity} from "../contracts/UserLiquidity.sol";
import {Elf} from "./Elf.sol"; // auto-generated contract after running `cargo build`.

contract UserLiquidityTest is RiscZeroCheats, Test {
    UserLiquidity public userLiquidity;
    address public constant user = address(0);

    function setUp() public {
        IRiscZeroVerifier verifier = deployRiscZeroVerifier();
        userLiquidity = new UserLiquidity(verifier);
        assertEq(userLiquidity.get(user), false);
    }

    function test_Set_WhenLiquidityIsNonZero() public {
        console2.log("Path: ", Elf.CHECK_LIQUIDITY_PATH);

        (bytes memory journal, bytes memory seal) = prove(
            Elf.CHECK_LIQUIDITY_PATH,
            abi.encode(user)
        );

        userLiquidity.set(abi.decode(journal, (address)), seal);
        assertEq(userLiquidity.get(address(user)), true);
    }

    function test_Set_WhenLiquidityIsZero() public {
        address userZero = address(0);
        (bytes memory journal, bytes memory seal) = prove(
            Elf.CHECK_LIQUIDITY_PATH,
            abi.encode(userZero)
        );

        userLiquidity.set(abi.decode(journal, (address)), seal);
        assertEq(userLiquidity.get(userZero), false);
    }
}
