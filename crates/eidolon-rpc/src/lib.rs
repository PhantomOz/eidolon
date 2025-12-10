use alloy_primitives::{Address, U64, U256};
use eidolon_evm::Executor;
use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::info;

/// 1. Define the JSON-RPC API Contract
/// These are the methods Metamask will try to call.
#[rpc(server)]
pub trait EidolonApi {
    /// Returns the Chain ID (We use 31337 for local dev)
    #[method(name = "eth_chainId")]
    fn chain_id(&self) -> U64;

    /// Returns the balance of an address
    #[method(name = "eth_getBalance")]
    fn get_balance(&self, address: Address, _block: Option<String>) -> U256;

    /// A custom "God Mode" method to set balance
    #[method(name = "tenderly_setBalance")]
    fn set_balance(&self, address: Address, amount: U256) -> bool;
}
