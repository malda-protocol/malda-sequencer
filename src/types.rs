//! # Types Module
//!
//! This module defines the core smart contract interfaces and data structures
//! used throughout the sequencer system for interacting with blockchain contracts.
//!
//! ## Key Features:
//! - **Market Contract Interface**: Defines interactions with Malda market contracts
//! - **Bridge Contract Interface**: Defines Accross bridge protocol interactions
//! - **Batch Processing Interface**: Defines batch submission contract interactions
//! - **L1 Block Interface**: Defines L1 block number retrieval for L2 chains
//! - **Data Structures**: Defines message structures for batch processing
//!
//! ## Architecture:
//! ```
//! Smart Contract Interfaces
//! ├── IMaldaMarket: Market operations (mint, repay, withdraw)
//! ├── IAccross: Cross-chain bridge operations
//! ├── IBatchSubmitter: Batch transaction submission
//! ├── IL1Block: L1 block number retrieval
//! └── BatchProcessMsg: Batch processing data structure
//! ```
//!
//! ## Contract Interactions:
//!
//! ### Market Operations (IMaldaMarket):
//! - **mintExternal**: Mint tokens with proof data
//! - **repayExternal**: Repay debt with proof data
//! - **outHere**: External operation with proof data
//! - **mint**: Simple token minting
//! - **withdrawGasFees**: Withdrawal of accumulated gas fees
//!
//! ### Bridge Operations (IAccross):
//! - **depositV3**: Cross-chain token deposit
//! - **getCurrentTime**: Get current timestamp
//! - **depositQuoteTimeBuffer**: Get quote time buffer
//!
//! ### Batch Operations (IBatchSubmitter):
//! - **batchProcess**: Submit batch of transactions
//!
//! ### L1 Block Operations (IL1Block):
//! - **number**: Get L1 block number (for L2 chains)

alloy::sol! {
    #![sol(rpc, all_derives)]

    /// Malda Market Contract Interface
    ///
    /// This interface defines the core operations available on Malda market contracts,
    /// including minting, repaying, and gas fee withdrawal operations. It supports
    /// both simple operations and complex operations with proof data.
    ///
    /// ## Key Operations:
    ///
    /// - **External Operations**: Operations that require proof data (journal and seal)
    /// - **Simple Operations**: Basic token operations without proof requirements
    /// - **Gas Fee Management**: Withdrawal of accumulated gas fees
    ///
    /// ## Proof-Based Operations:
    ///
    /// External operations require proof data (journal and seal) to verify
    /// the legitimacy of the operation before execution. This ensures
    /// security and prevents unauthorized operations.
    interface IMaldaMarket {
        /// Mint tokens with proof data
        ///
        /// This function allows minting tokens using proof data for verification.
        /// The proof data includes journal and seal information that proves
        /// the legitimacy of the minting operation.
        ///
        /// # Arguments
        /// * `journalData` - Journal data for proof verification
        /// * `seal` - Seal data for proof verification
        /// * `amount` - Array of amounts to mint
        /// * `receiver` - Address to receive the minted tokens
        function mintExternal(
            bytes calldata journalData,
            bytes calldata seal,
            uint256[] calldata amount,
            address receiver
        ) external;

        /// Repay debt with proof data
        ///
        /// This function allows repaying debt using proof data for verification.
        /// The proof data includes journal and seal information that proves
        /// the legitimacy of the repayment operation.
        ///
        /// # Arguments
        /// * `journalData` - Journal data for proof verification
        /// * `seal` - Seal data for proof verification
        /// * `repayAmount` - Array of amounts to repay
        /// * `receiver` - Address to receive the repayment
        function repayExternal(
            bytes calldata journalData,
            bytes calldata seal,
            uint256[] calldata repayAmount,
            address receiver
        ) external;

        /// External operation with proof data
        ///
        /// This function performs external operations using proof data for verification.
        /// The proof data includes journal and seal information that proves
        /// the legitimacy of the operation.
        ///
        /// # Arguments
        /// * `journalData` - Journal data for proof verification
        /// * `seal` - Seal data for proof verification
        /// * `amounts` - Array of amounts for the operation
        /// * `receiver` - Address to receive the operation result
        function outHere(bytes calldata journalData, bytes calldata seal, uint256[] memory amounts, address receiver)
        external;

        /// Simple token minting
        ///
        /// This function performs simple token minting without proof requirements.
        /// It's used for basic minting operations that don't require
        /// complex verification.
        ///
        /// # Arguments
        /// * `amount` - Amount of tokens to mint
        function mint(uint256 amount) external;

        /// Withdraw accumulated gas fees
        ///
        /// This function allows withdrawing accumulated gas fees from the market
        /// contract. Gas fees are collected from various operations and can
        /// be withdrawn by authorized parties.
        ///
        /// # Arguments
        /// * `receiver` - Address to receive the withdrawn gas fees
        function withdrawGasFees(address receiver) external;
    }

    /// Accross Bridge Contract Interface
    ///
    /// This interface defines the operations available on Accross bridge contracts
    /// for cross-chain token transfers. It supports the latest V3 deposit protocol
    /// with enhanced security and efficiency features.
    ///
    /// ## Key Operations:
    ///
    /// - **Cross-Chain Deposits**: Transfer tokens between different chains
    /// - **Time Management**: Handle quote timestamps and deadlines
    /// - **Security Features**: Exclusive relayer and deadline management
    ///
    /// ## Bridge Protocol:
    ///
    /// The Accross bridge protocol enables secure cross-chain token transfers
    /// with built-in security features like deadline management and exclusive
    /// relayer support.
    interface IAccross {
        /// Cross-chain token deposit (V3)
        ///
        /// This function deposits tokens for cross-chain transfer using the V3
        /// protocol. It includes enhanced security features and deadline management.
        ///
        /// # Arguments
        /// * `depositor` - Address making the deposit
        /// * `recipient` - Address to receive tokens on destination chain
        /// * `inputToken` - Token address on source chain
        /// * `outputToken` - Token address on destination chain
        /// * `inputAmount` - Amount of tokens to deposit
        /// * `outputAmount` - Expected amount on destination chain
        /// * `destinationChainId` - ID of destination chain
        /// * `exclusiveRelayer` - Exclusive relayer address (or zero)
        /// * `quoteTimestamp` - Timestamp for quote validation
        /// * `fillDeadline` - Deadline for filling the deposit
        /// * `exclusivityDeadline` - Deadline for exclusive relayer
        /// * `message` - Additional message data
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

        /// Get current timestamp
        ///
        /// This function returns the current timestamp for quote validation
        /// and deadline calculations.
        ///
        /// # Returns
        /// * `uint32` - Current timestamp
        function getCurrentTime() external view returns (uint32);

        /// Get deposit quote time buffer
        ///
        /// This function returns the time buffer used for deposit quote
        /// validation to ensure quotes are fresh and valid.
        ///
        /// # Returns
        /// * `uint32` - Quote time buffer in seconds
        function depositQuoteTimeBuffer() external view returns (uint32);
    }

    /// Batch processing message structure
    ///
    /// This struct defines the data structure used for batch processing
    /// operations. It contains all necessary information for processing
    /// multiple transactions in a single batch.
    ///
    /// ## Batch Processing:
    ///
    /// Batch processing allows multiple transactions to be submitted together,
    /// improving efficiency and reducing gas costs. This structure contains
    /// all the data needed for a batch operation.
    ///
    /// ## Data Organization:
    ///
    /// - **Receivers**: Array of addresses to receive the operations
    /// - **Proof Data**: Journal and seal data for verification
    /// - **Token Data**: Arrays of token addresses and amounts
    /// - **Function Data**: Selectors and initialization hashes
    /// - **Indexing**: Start index for batch processing
    struct BatchProcessMsg {
        /// Array of receiver addresses
        address[] receivers;
        /// Journal data for proof verification
        bytes journalData;
        /// Seal data for proof verification
        bytes seal;
        /// Array of market token addresses
        address[] mTokens;
        /// Array of amounts for each operation
        uint256[] amounts;
        /// Array of minimum output amounts
        uint256[] minAmountsOut;
        /// Array of function selectors
        bytes4[] selectors;
        /// Array of initialization hashes
        bytes32[] initHashes;
        /// Start index for batch processing
        uint256 startIndex;
        /// Array of users to liquidate (for liquidate events)
        address[] userToLiquidate;
        /// Array of collateral addresses (for liquidate events)
        address[] collateral;
    }

    /// Batch Submitter Contract Interface
    ///
    /// This interface defines the operations available on batch submitter contracts
    /// for submitting batches of transactions. It provides efficient batch processing
    /// capabilities for multiple operations.
    ///
    /// ## Key Operations:
    ///
    /// - **Batch Processing**: Submit multiple transactions in a single batch
    /// - **Efficiency**: Reduce gas costs by batching operations
    /// - **Verification**: Include proof data for batch verification
    ///
    /// ## Batch Efficiency:
    ///
    /// Batch processing allows multiple operations to be submitted together,
    /// significantly reducing gas costs and improving transaction throughput.
    interface IBatchSubmitter {
        /// Submit batch of transactions
        ///
        /// This function submits a batch of transactions for processing.
        /// The batch includes multiple operations with proof data for
        /// verification and efficiency.
        ///
        /// # Arguments
        /// * `msg` - Batch processing message containing all operation data
        function batchProcess(BatchProcessMsg memory msg) external;
    }

    /// L1 Block Contract Interface
    ///
    /// This interface defines the operations available on L1 block contracts
    /// for retrieving L1 block numbers on L2 chains. It's used by L2 chains
    /// to access L1 block information for various purposes.
    ///
    /// ## Key Operations:
    ///
    /// - **Block Number Retrieval**: Get current L1 block number
    /// - **L2 Integration**: Enable L2 chains to access L1 data
    ///
    /// ## L2 Chain Usage:
    ///
    /// L2 chains use L1 block contracts to access L1 block information
    /// for various purposes like fee calculations and security mechanisms.
    interface IL1Block {
        /// Get L1 block number
        ///
        /// This function returns the current L1 block number.
        /// It's used by L2 chains to access L1 block information.
        ///
        /// # Returns
        /// * `uint64` - Current L1 block number
        function number() external view returns (uint64);
    }
}
