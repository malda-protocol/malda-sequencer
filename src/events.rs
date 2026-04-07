use alloy::{
    primitives::{Address, Bytes, FixedBytes, U256},
    rpc::types::Log,
};
use hex;
use serde::{Deserialize, Serialize};

// Mark as used since it might be used elsewhere through imports
#[allow(dead_code)]
type Bytes4 = FixedBytes<4>;

type Bytes32 = FixedBytes<32>;

#[derive(Debug, Serialize, Deserialize)]
pub struct LiquidateExternalEvent {
    pub msg_sender: Address,
    pub src_sender: Address,
    pub user_to_liquidate: Address,
    pub receiver: Address,
    pub collateral: Address,
    pub src_chain_id: u32,
    pub amount: U256,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MintExternalEvent {
    pub msg_sender: Address,
    pub src_sender: Address,
    pub receiver: Address,
    pub chain_id: u32,
    pub amount: U256,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BorrowExternalEvent {
    pub msg_sender: Address,
    pub src_sender: Address,
    pub chain_id: u32,
    pub amount: U256,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RepayExternalEvent {
    pub msg_sender: Address,
    pub src_sender: Address,
    pub position: Address,
    pub chain_id: u32,
    pub amount: U256,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WithdrawExternalEvent {
    pub msg_sender: Address,
    pub src_sender: Address,
    pub chain_id: u32,
    pub amount: U256,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct WithdrawOnExtensionChainEvent {
    pub sender: Address,
    pub dst_chain_id: u32,
    pub amount: U256,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SuppliedEvent {
    pub from: Address,
    pub acc_amount_in: U256,
    pub acc_amount_out: U256,
    pub amount: U256,
    pub src_chain_id: u32,
    pub dst_chain_id: u32,
    pub linea_method_selector: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ExtractedEvent {
    pub msg_sender: Address,
    pub src_sender: Address,
    pub receiver: Address,
    pub acc_amount_in: U256,
    pub acc_amount_out: U256,
    pub amount: U256,
    pub src_chain_id: u32,
    pub dst_chain_id: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LiquidateEvent {
    pub from: Address,
    pub receiver: Address,
    pub amount: U256,
    pub src_chain_id: u32,
    pub dst_chain_id: u32,
    pub user_to_liquidate: Address,
    pub collateral: Address,
}

// Event signatures as constants
pub const HOST_LIQUIDATE_EXTERNAL_SIG: &str =
    "mErc20Host_LiquidateExternal(address,address,address,address,address,uint32,uint256)";
pub const HOST_MINT_EXTERNAL_SIG: &str =
    "mErc20Host_MintExternal(address,address,address,uint32,uint256)";
pub const HOST_BORROW_EXTERNAL_SIG: &str =
    "mErc20Host_BorrowExternal(address,address,uint32,uint256)";
pub const HOST_REPAY_EXTERNAL_SIG: &str =
    "mErc20Host_RepayExternal(address,address,address,uint32,uint256)";
pub const HOST_WITHDRAW_EXTERNAL_SIG: &str =
    "mErc20Host_WithdrawExternal(address,address,uint32,uint256)";
pub const HOST_BORROW_ON_EXTENSION_CHAIN_SIG: &str =
    "mErc20Host_BorrowOnExtensionChain(address,uint32,uint256)";
pub const HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG: &str =
    "mErc20Host_WithdrawOnExtensionChain(address,uint32,uint256)";
pub const EXTENSION_SUPPLIED_SIG: &str =
    "mTokenGateway_Supplied(address,address,uint256,uint256,uint256,uint32,uint32,bytes4)";
pub const EXTENSION_EXTRACTED_SIG: &str =
    "mTokenGateway_Extracted(address,address,address,uint256,uint256,uint256,uint32,uint32)";
pub const EXTENSION_LIQUIDATE_SIG: &str =
    "mTokenGateway_Liquidate(address,address,uint256,uint32,uint32,address,address)";

pub const MINT_EXTERNAL_SELECTOR: &str = "05dbe8a7";
pub const REPAY_EXTERNAL_SELECTOR: &str = "08fee263";
pub const OUT_HERE_SELECTOR: &str = "b511d3b1";
pub const LIQUIDATE_EXTERNAL_SELECTOR: &str = "liquidateExternal"; // This will need to be updated with the actual selector

pub const MINT_EXTERNAL_SELECTOR_FB4: &[u8] = &[0x05, 0xdb, 0xe8, 0xa7];
pub const REPAY_EXTERNAL_SELECTOR_FB4: &[u8] = &[0x08, 0xfe, 0xe2, 0x63];
pub const OUT_HERE_SELECTOR_FB4: &[u8] = &[0xb5, 0x11, 0xd3, 0xb1];
pub const LIQUIDATE_EXTERNAL_SELECTOR_FB4: &[u8] = &[0x00, 0x00, 0x00, 0x00]; // This will need to be updated with the actual selector

// Unified event processing system
#[derive(Debug, Clone, PartialEq)]
pub enum EventType {
    HostBorrow,
    HostWithdraw,
    ExtensionLiquidate,
    ExtensionMint,
    ExtensionRepay,
    Unknown,
}

impl EventType {
    pub fn from_signature(signature: &[u8]) -> Self {
        use alloy::primitives::keccak256;

        match signature {
            sig if sig == keccak256(HOST_BORROW_ON_EXTENSION_CHAIN_SIG.as_bytes()) => {
                EventType::HostBorrow
            }
            sig if sig == keccak256(HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG.as_bytes()) => {
                EventType::HostWithdraw
            }
            sig if sig == keccak256(EXTENSION_LIQUIDATE_SIG.as_bytes()) => {
                EventType::ExtensionLiquidate
            }
            _ => EventType::Unknown,
        }
    }

    pub fn from_method_selector(selector: &str) -> Self {
        match selector {
            MINT_EXTERNAL_SELECTOR => EventType::ExtensionMint,
            REPAY_EXTERNAL_SELECTOR => EventType::ExtensionRepay,
            LIQUIDATE_EXTERNAL_SELECTOR => EventType::ExtensionLiquidate,
            _ => EventType::Unknown,
        }
    }

    pub fn to_string(&self) -> &'static str {
        match self {
            EventType::HostBorrow => "HostBorrow",
            EventType::HostWithdraw => "HostWithdraw",
            EventType::ExtensionLiquidate => "ExtensionLiquidate",
            EventType::ExtensionMint => "ExtensionSupply",
            EventType::ExtensionRepay => "ExtensionSupply",
            EventType::Unknown => "Unknown",
        }
    }

    pub fn target_function(&self) -> &'static str {
        match self {
            EventType::HostBorrow | EventType::HostWithdraw => "outHere",
            EventType::ExtensionLiquidate => "liquidateExternal",
            EventType::ExtensionMint => "mintExternal",
            EventType::ExtensionRepay => "repayExternal",
            EventType::Unknown => "unknown",
        }
    }
}

// Add the parsing functions here
pub fn parse_supplied_event(log: &Log) -> SuppliedEvent {
    let from = Address::from_slice(&log.topics()[1][12..]);
    let _receiver = Address::from_slice(&log.topics()[2][12..]);

    // The non-indexed parameters are packed in the data field
    let data = log.data().data.clone();

    SuppliedEvent {
        from,
        acc_amount_in: U256::from_be_slice(&data[0..32]),
        acc_amount_out: U256::from_be_slice(&data[32..64]),
        amount: U256::from_be_slice(&data[64..96]),
        src_chain_id: u32::from_be_bytes(data[124..128].try_into().unwrap()),
        dst_chain_id: u32::from_be_bytes(data[156..160].try_into().unwrap()),
        linea_method_selector: hex::encode(&data[160..164]),
    }
}

pub fn parse_withdraw_on_extension_chain_event(log: &Log) -> WithdrawOnExtensionChainEvent {
    WithdrawOnExtensionChainEvent {
        sender: Address::from_slice(&log.topics()[1][12..]),
        // Chain ID is padded to 32 bytes, we want the last 4 bytes
        dst_chain_id: u32::from_be_bytes(log.data().data[28..32].try_into().unwrap()),
        amount: U256::from_be_slice(&log.data().data[32..64]),
    }
}

pub fn parse_liquidate_event(log: &Log) -> LiquidateEvent {
    let from = Address::from_slice(&log.topics()[1][12..]);
    let receiver = Address::from_slice(&log.topics()[2][12..]);
    
    // The non-indexed parameters are packed in the data field
    let data = log.data().data.clone();
    
    LiquidateEvent {
        from,
        receiver,
        amount: U256::from_be_slice(&data[0..32]),
        src_chain_id: u32::from_be_bytes(data[32..36].try_into().unwrap()),
        dst_chain_id: u32::from_be_bytes(data[36..40].try_into().unwrap()),
        user_to_liquidate: Address::from_slice(&data[40..60]),
        collateral: Address::from_slice(&data[60..80]),
    }
}

pub const BATCH_PROCESS_FAILED_SIG: &str =
    "BatchProcessFailed(bytes32,address,address,uint256,uint256,bytes4,bytes)";
pub const BATCH_PROCESS_SUCCESS_SIG: &str =
    "BatchProcessSuccess(bytes32,address,address,uint256,uint256,bytes4)";

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchProcessFailedEvent {
    pub init_hash: Bytes32,
    pub reason: Bytes,
    pub block_number: u64,
}

pub fn parse_batch_process_failed_event(log: &Log) -> BatchProcessFailedEvent {
    // For non-indexed events, all data is in the data field
    let data = log.data().data.clone();

    BatchProcessFailedEvent {
        init_hash: Bytes32::from_slice(data[0..32].into()),
        reason: Bytes::from(data[32..].to_vec()),
        block_number: log.block_number.unwrap_or_default(),
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BatchProcessSuccessEvent {
    pub init_hash: Bytes32,
    pub receiver: Address,
    pub m_token: Address,
    pub amount: U256,
    pub min_amount_out: U256,
    pub selector: FixedBytes<4>,
    pub block_number: u64,
}

pub fn parse_batch_process_success_event(log: &Log) -> BatchProcessSuccessEvent {
    let data = log.data().data.clone();
    
    BatchProcessSuccessEvent {
        init_hash: Bytes32::from_slice(data[0..32].into()),
        receiver: Address::from_slice(&data[32..52]),
        m_token: Address::from_slice(&data[52..72]),
        amount: U256::from_be_slice(&data[72..104]),
        min_amount_out: U256::from_be_slice(&data[104..136]),
        selector: FixedBytes::from_slice(&data[136..140]),
        block_number: log.block_number.unwrap_or_default(),
    }
}
