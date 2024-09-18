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
import {Test} from "forge-std/Test.sol";
import {IRiscZeroVerifier, Receipt as RiscZeroReceipt, VerificationFailed} from "risc0/IRiscZeroVerifier.sol";
import {UserLiquidity} from "../contracts/UserLiquidity.sol";
import {Elf} from "./Elf.sol"; // auto-generated contract after running `cargo build`.
import {UserLiquidity} from "../contracts/UserLiquidity.sol";
import {Steel, Beacon, Encoding} from "risc0/steel/Steel.sol";

contract UserLiquidityTest is RiscZeroCheats, Test {
    UserLiquidity public userLiquidity;
    RiscZeroMockVerifier public mockVerifier;
    IRiscZeroVerifier public verifier;
    bytes32 public imageId;

    // take same example values as for guest tests, irrelevant since just mock proof for now but with bonsai test real values
    string public constant mainnetRpcUrl = "https://eth-mainnet.g.alchemy.com/v2/scFv-881VOeTp7qHT88HEZ_EmsJqrGQ0";
    uint256 public constant blockNo = 20770922; // number used in cargo tests for the method
    address public constant userWithLiquidity = 0xa66d568cD146C01ac44034A01272C69C2d9e4BaB;
    address public constant userWithoutLiquidity = address(0x0);
    uint256 public constant liquidity = 16853630641732729601194;
    uint256 public constant ethereumId = 1;

    struct ProverInput {
        bytes input;
    }

    struct ProofInputParams {
        uint256 chainId;
        uint256 blockNo;
        address user;
    }

    mapping(uint256 => mapping(uint256 => mapping(address => bytes))) public proofInputs;

    function setUp() public {
        vm.createSelectFork(mainnetRpcUrl, blockNo + 1); // + 1 because verification on chain can earliest happen 1 block later
        mockVerifier = RiscZeroMockVerifier(address(deployRiscZeroVerifier()));
        verifier = deployRiscZeroVerifier();
        userLiquidity = devMode() ? new UserLiquidity(mockVerifier) : new UserLiquidity(verifier);
        imageId = userLiquidity.imageId();
        assertEq(userLiquidity.get(userWithLiquidity), false);
        assertEq(userLiquidity.get(userWithoutLiquidity), false);
        _populateTestProofInputFromJSON();
    }

    function test_Set_WhenLiquidityIsNonZero() public {
        // get the hash of block to prove
        uint240 blockNumber = uint240(blockNo);
        bytes32 blockHash = blockhash(blockNumber);

        bytes memory journal;
        bytes memory seal;
        // create a mock proof
        if (devMode()) {
            // mock the Journal
            journal = abi.encode(
                UserLiquidity.Journal({
                    commitment: Steel.Commitment(Encoding.encodeVersionedID(blockNumber, 0), blockHash),
                    liquidity: liquidity,
                    user: userWithLiquidity
                })
            );
            RiscZeroReceipt memory receipt = mockVerifier.mockProve(imageId, sha256(journal));
            seal = receipt.seal;
        } else {
            (journal, seal) = prove(Elf.CHECK_LIQUIDITY_PATH, hex"12345678");
        }

        userLiquidity.set(journal, seal);

        // check that liquidity has been set
        assert(userLiquidity.get(userWithLiquidity) == true);
    }

    // // Some test where user without liquidity tries to set - not sure if possible here since this would fail on the prover side
    // // there is the Risc0Cheatcode prove() which we can integrate, but need bonsai API for that
    // // For now exclude test since it would not revert
    // function test_Set_WhenLiquidityIsZero_Reverts() public {
    //     // get the hash of the previous block
    //     uint240 blockNumber = uint240(block.number - 1);
    //     bytes32 blockHash = blockhash(blockNumber);

    //     // mock the Journal
    //     UserLiquidity.Journal memory journal = UserLiquidity.Journal({
    //         commitment: Steel.Commitment(Encoding.encodeVersionedID(blockNumber, 0), blockHash),
    //         liquidity: 0,
    //         user: userWithoutLiquidity
    //     });

    //     // create a mock proof
    //     RiscZeroReceipt memory receipt = verifier.mockProve(imageId, sha256(abi.encode(journal)));

    //     // try to set the liquidity - this will still pass and the test fail because we create a mock proof
    //     // that userWithoutLiquidity has liquidity. In reality this proof cannot exist.
    //     vm.expectRevert();
    //     userLiquidity.set(abi.encode(journal), receipt.seal);

    //     // check that liquidity was not been set
    //     assert(userLiquidity.get(userWithoutLiquidity) == false);
    // }

    // function test_Set_WhenLiquidityIsZero_ManipulatedProof_Reverts() public {
    //     // get the hash of the previous block
    //     uint240 blockNumber = uint240(block.number - 1);
    //     bytes32 blockHash = blockhash(blockNumber);

    //     // mock the Journal
    //     UserLiquidity.Journal memory journal = UserLiquidity.Journal({
    //         commitment: Steel.Commitment(Encoding.encodeVersionedID(blockNumber, 0), blockHash),
    //         liquidity: 0,
    //         user: userWithoutLiquidity
    //     });

    //     // create a mock proof
    //     RiscZeroReceipt memory receipt = verifier.mockProve(imageId, sha256(abi.encode(journal)));

    //     // Malicious depositor without liquidity requests a proof and then changes the liquidity to more than zero in the journal
    //     journal.liquidity = 1;
    //     vm.expectRevert(abi.encodeWithSelector(VerificationFailed.selector));
    //     userLiquidity.set(abi.encode(journal), receipt.seal);

    //     // check that liquidity was not been set
    //     assert(userLiquidity.get(userWithoutLiquidity) == false);
    // }

    function _populateTestProofInputFromJSON() internal {
        string memory config_data = vm.readFile("tests/testProofInput.json");
        ProofInputParams[] memory params;
        bytes memory paramsRaw = vm.parseJson(config_data, ".TestProofParams");
        params = abi.decode(paramsRaw, (ProofInputParams[]));
        ProverInput[] memory prooverInput;
        bytes memory prooverInputRaw = vm.parseJson(config_data, ".TestProofInput");

        prooverInput = abi.decode(prooverInputRaw, (ProverInput[]));

        for (uint256 i; i < params.length; ++i) {
            proofInputs[params[i].blockNo][params[i].chainId][params[i].user] = prooverInput[i].input;
        }
    }

    // function test_JSON_read_correctly() public view {
    //     assertEq(proofInputs[1][18000000][0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2], hex"12345678");
    // }
}
