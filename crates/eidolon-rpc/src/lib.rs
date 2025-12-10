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

/// 2. The Implementation
/// Holds the shared state of the EVM.
pub struct EidolonRpc {
    // Arc = Atomic Reference Count (Shared ownership)
    // RwLock = Read/Write Lock (Safe mutability across threads)
    executor: Arc<RwLock<Executor>>,
}

impl EidolonRpc {
    pub fn new(executor: Arc<RwLock<Executor>>) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl EidolonApiServer for EidolonRpc {
    fn chain_id(&self) -> U64 {
        // 31337 is the standard "Anvil/Hardhat" dev chain ID
        U64::from(31337)
    }

    fn get_balance(&self, address: Address, _block: Option<String>) -> U256 {
        // Acquire a READ lock (multiple readers allowed)
        let mut executor = self.executor.write(); // Using write lock for simplicity due to cache DB mutable needs
        let bal = executor.get_balance(address).unwrap_or(U256::ZERO);
        info!("🔍 eth_getBalance({:?}) -> {}", address, bal);
        bal
    }

    fn set_balance(&self, address: Address, amount: U256) -> bool {
        // Acquire a WRITE lock (exclusive access)
        let mut executor = self.executor.write();
        executor.set_balance(address, amount);
        info!("🧙 tenderly_setBalance({:?}) -> {}", address, amount);
        true
    }
}
