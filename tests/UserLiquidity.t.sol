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
import {UserLiquidity} from "../contracts/UserLiquidity.sol";
import {Steel, Beacon, Encoding} from "risc0/steel/Steel.sol";

contract UserLiquidityTest is RiscZeroCheats, Test {
    
    UserLiquidity public userLiquidity;

    // take same example values as for guest tests, irrelevant since just mock proof for now but with bonsai test real values
    string public constant mainnetRpcUrl = "https://eth-mainnet.g.alchemy.com/v2/scFv-881VOeTp7qHT88HEZ_EmsJqrGQ0";
    uint256 public constant blockNo = 20770922;
    address public constant userWithLiquidity = 0xa66d568cD146C01ac44034A01272C69C2d9e4BaB;
    address public constant userWithoutLiquidity = address(0x0);
    uint256 public constant liquidity = 16853630641732729601194; 

    function setUp() public {
        vm.createSelectFork(mainnetRpcUrl, blockNo);
        IRiscZeroVerifier verifier = deployRiscZeroVerifier();
        userLiquidity = new UserLiquidity(verifier);
        assertEq(userLiquidity.get(userWithLiquidity), false);
        assertEq(userLiquidity.get(userWithoutLiquidity), false);
    }

    function test_Set_WhenLiquidityIsNonZero() public {
        // get the hash of the previous block
        uint240 blockNumber = uint240(block.number - 1);
        bytes32 blockHash = blockhash(blockNumber);

        // mock the Journal
        UserLiquidity.Journal memory journal = UserLiquidity.Journal({
            commitment: Steel.Commitment(Encoding.encodeVersionedID(blockNumber, 0), blockHash),
            tokenContract: address(token)
        });
        // create a mock proof
        RiscZeroReceipt memory receipt = verifier.mockProve(imageId, sha256(abi.encode(journal)));

        uint256 previous_count = counter.get();

        counter.increment(abi.encode(journal), receipt.seal);

        // check that the counter was incremented
        assert(counter.get() == previous_count + 1);
    }

    function test_Set_WhenLiquidityIsZero() public {
        address userZero = address(0);
        (bytes memory journal, bytes memory seal) = prove(Elf.CHECK_LIQUIDITY_PATH, abi.encode(userZero));

        userLiquidity.set(abi.decode(journal, (address)), seal);
        assertEq(userLiquidity.get(userZero), false);
    }
}
