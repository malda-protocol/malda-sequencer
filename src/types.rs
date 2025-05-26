alloy::sol! {
    #![sol(rpc, all_derives)]
    interface IMaldaMarket {
        function mintExternal(
            bytes calldata journalData,
            bytes calldata seal,
            uint256[] calldata amount,
            address receiver
        ) external;

        function repayExternal(
            bytes calldata journalData,
            bytes calldata seal,
            uint256[] calldata repayAmount,
            address receiver
        ) external;

        function outHere(bytes calldata journalData, bytes calldata seal, uint256[] memory amounts, address receiver)
        external;

        function mint(uint256 amount) external;

        function withdrawGasFees(address receiver) external;
    }

    interface IAccross {
        function depositV3(
            address depositor,
            address recipient,
            address inputToken,
            address outputToken,
            uint256 inputAmount,
            uint256 outputAmount,
            uint256 destinationChainId,
            address exclusiveRelayer,
            uint32 quoteTimestamp,
            uint32 fillDeadline,
            uint32 exclusivityDeadline,
            bytes calldata message
        ) external payable;

        function getCurrentTime() external view returns (uint32);

        function depositQuoteTimeBuffer() external view returns (uint32);
    }

    struct BatchProcessMsg {
        address[] receivers;
        bytes journalData;
        bytes seal;
        address[] mTokens;
        uint256[] amounts;
        uint256[] minAmountsOut;
        bytes4[] selectors;
        bytes32[] initHashes;
        uint256 startIndex;
    }

    interface IBatchSubmitter {
        function batchProcess(BatchProcessMsg memory msg) external;
    }

    interface IL1Block {
        function number() external view returns (uint64);
    }
}
