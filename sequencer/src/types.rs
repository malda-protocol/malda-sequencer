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
    }
} 