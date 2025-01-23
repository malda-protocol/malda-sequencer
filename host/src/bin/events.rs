use alloy::primitives::{Address, U256};
use serde::{Deserialize, Serialize};

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

// Event signatures as constants
pub const HOST_LIQUIDATE_EXTERNAL_SIG: &str = "mErc20Host_LiquidateExternal(address,address,address,address,address,uint32,uint256)";
pub const HOST_MINT_EXTERNAL_SIG: &str = "mErc20Host_MintExternal(address,address,address,uint32,uint256)";
pub const HOST_BORROW_EXTERNAL_SIG: &str = "mErc20Host_BorrowExternal(address,address,uint32,uint256)";
pub const HOST_REPAY_EXTERNAL_SIG: &str = "mErc20Host_RepayExternal(address,address,address,uint32,uint256)";
pub const HOST_WITHDRAW_EXTERNAL_SIG: &str = "mErc20Host_WithdrawExternal(address,address,uint32,uint256)";
pub const HOST_BORROW_ON_EXTENSION_CHAIN_SIG: &str = "mErc20Host_BorrowOnExternsionChain(address,uint32,uint256)";
pub const HOST_WITHDRAW_ON_EXTENSION_CHAIN_SIG: &str = "mErc20Host_WithdrawOnExtensionChain(address,uint32,uint256)";
pub const EXTENSION_SUPPLIED_SIG: &str = "mTokenGateway_Supplied(address,uint256,uint256,uint256,uint32,uint32,bytes4)";
pub const EXTENSION_EXTRACTED_SIG: &str = "mTokenGateway_Extracted(address,address,address,uint256,uint256,uint256,uint32,uint32)";

pub const MINT_EXTERNAL_SELECTOR: &str = "6f8be464";
pub const REPAY_EXTERNAL_SELECTOR: &str = "d7afddc5";

