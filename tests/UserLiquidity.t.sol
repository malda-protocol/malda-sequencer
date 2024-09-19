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
import {RiscZeroMockVerifier} from "risc0/test/RiscZeroMockVerifier.sol";
import {console2} from "forge-std/console2.sol";
import {IRiscZeroVerifier, VerificationFailed} from "risc0/IRiscZeroVerifier.sol";
import {Elf} from "./Elf.sol"; // auto-generated contract after running `cargo build`.
import {UserLiquidity, Journal} from "../contracts/UserLiquidity.sol";
import {TestHelper} from "./TestHelper.sol";

contract UserLiquidityTest is RiscZeroCheats, TestHelper {
    UserLiquidity public userLiquidity;
    IRiscZeroVerifier public verifier;
    bytes32 public imageId;

    // take same example values as for guest tests, irrelevant since just mock proof for now but with bonsai test real values
    string public constant mainnetRpcUrl = "https://eth-mainnet.g.alchemy.com/v2/scFv-881VOeTp7qHT88HEZ_EmsJqrGQ0";
    uint256 public constant blockNo = 20770922; // number used in cargo tests for the method
    address public constant userWithLiquidity = 0xa66d568cD146C01ac44034A01272C69C2d9e4BaB;
    address public constant userWithoutLiquidity = address(0x0);
    uint256 public constant liquidity = 16853630641732729601194;
    uint256 public constant ethereumId = 1;

    function setUp() public {
        vm.createSelectFork(mainnetRpcUrl, blockNo + 1); // + 1 because verification on chain can earliest happen 1 block later
        verifier = deployRiscZeroVerifier();
        userLiquidity = new UserLiquidity(verifier);
        imageId = userLiquidity.imageId();
        assertEq(userLiquidity.get(userWithLiquidity), false);
        assertEq(userLiquidity.get(userWithoutLiquidity), false);
        _populateTestProofInputFromJSON();
    }

    function test_Set_WhenLiquidityIsNonZero() public {
        (bytes memory journal, bytes memory seal) =
            prove(Elf.CHECK_LIQUIDITY_PATH, proofInputs[ethereumId][blockNo][userWithLiquidity]);

        userLiquidity.set(journal, seal);

        // check that liquidity has been set
        assert(userLiquidity.get(userWithLiquidity) == true);
    }

    function test_Set_ProofRevertsWhenLiquidityIsZero() public {
        (bytes memory journal, bytes memory seal) =
            prove(Elf.CHECK_LIQUIDITY_PATH, proofInputs[ethereumId][blockNo][userWithoutLiquidity]);

        userLiquidity.set(journal, seal);

        // since user doesnt have liquidty its still false
        assert(userLiquidity.get(userWithLiquidity) == false);
    }

    function test_Set_ProofRevertsWhenJournalIsModified() public {
        (bytes memory journal, bytes memory seal) =
            prove(Elf.CHECK_LIQUIDITY_PATH, proofInputs[ethereumId][blockNo][userWithoutLiquidity]);

        Journal memory journalDecoded = abi.decode(journal, (Journal));
        journalDecoded.liquidity = 1000;
        journal = abi.encode(journalDecoded);

        vm.expectRevert(abi.encodeWithSelector(VerificationFailed.selector));
        userLiquidity.set(journal, seal);
    }
}
