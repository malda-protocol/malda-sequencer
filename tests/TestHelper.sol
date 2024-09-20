// SPDX-License-Identifier: UNLICENSED

pragma solidity ^0.8.20;

import {Test} from "forge-std/Test.sol";

contract TestHelper is Test {
    struct ProverInput {
        bytes input;
    }

    struct ProofInputParams {
        uint256 chainId;
        uint256 blockNo;
        address user;
    }

    mapping(uint256 => mapping(uint256 => mapping(address => bytes))) public proofInputs;

    function _populateTestProofInputFromJSON() internal {
        string memory config_data = vm.readFile("tests/testProofInput.json");
        ProofInputParams[] memory params;
        bytes memory paramsRaw = vm.parseJson(config_data, ".TestProofParams");
        params = abi.decode(paramsRaw, (ProofInputParams[]));
        ProverInput[] memory proverInput;
        bytes memory proverInputRaw = vm.parseJson(config_data, ".TestProofInput");

        proverInput = abi.decode(proverInputRaw, (ProverInput[]));

        for (uint256 i; i < params.length; ++i) {
            proofInputs[params[i].blockNo][params[i].chainId][params[i].user] = proverInput[i].input;
        }
    }
}
