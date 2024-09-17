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

import {IRiscZeroVerifier} from "risc0/IRiscZeroVerifier.sol";
import {Steel} from "risc0/steel/Steel.sol";
import {ImageID} from "./ImageID.sol"; // auto-generated contract after running `cargo build`.

/// @title A starter application using RISC Zero.
/// @notice This basic application holds a number, guaranteed to be even.
/// @dev This contract demonstrates one pattern for offloading the computation of an expensive
///      or difficult to implement function to a RISC Zero guest running on Bonsai.
contract UserLiquidity {
    /// @notice RISC Zero verifier contract address.
    IRiscZeroVerifier public immutable verifier;
    /// @notice Image ID of the only zkVM binary to accept verification from.
    ///         The image ID is similar to the address of a smart contract.
    ///         It uniquely represents the logic of that guest program,
    ///         ensuring that only proofs generated from a pre-defined guest program
    ///         (in this case, checking if a number is even) are considered valid.
    bytes32 public constant imageId = ImageID.CHECK_LIQUIDITY_ID;

    /// @notice A number that is guaranteed, by the RISC Zero zkVM, to be even.
    ///         It can be set by calling the `set` function.
    mapping(address user => bool hasLiquidity) public userHasLiquidity;

    /// @notice Journal that is committed to by the guest.
    struct Journal {
        Steel.Commitment commitment;
        uint256 liquidity;
        address user;
    }

    /// @notice Initialize the contract, binding it to a specified RISC Zero verifier.
    constructor(IRiscZeroVerifier _verifier) {
        verifier = _verifier;
    }

    /// @notice Set the even number stored on the contract. Requires a RISC Zero proof that the number is even.
    function set(bytes calldata journalData, bytes calldata seal) public {
        // Decode and validate the journal data
        Journal memory journal = abi.decode(journalData, (Journal));
        require(Steel.validateCommitment(journal.commitment), "Invalid commitment");

        // Verify the proof
        verifier.verify(seal, imageId, sha256(journalData));

        userHasLiquidity[journal.user] = true;
        // here we just set liquidity to true, for real logic use journal.liquidity
    }

    /// @notice Returns the number stored.
    function get(address user) public view returns (bool) {
        return userHasLiquidity[user];
    }
}
