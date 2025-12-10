use alloy_primitives::{Address, U64, U256};
use eidolon_evm::Executor;
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObject;
use parking_lot::RwLock;
use std::sync::Arc;
use tracing::{error, info};

/// 1. Define the JSON-RPC API Contract
/// These are the methods Metamask will try to call.
#[rpc(server)]
pub trait EidolonApi {
    /// Returns the Chain ID
    #[method(name = "eth_chainId")]
    fn chain_id(&self) -> RpcResult<U64>; // Return type must be RpcResult<T>

    /// Returns the balance of an address
    #[method(name = "eth_getBalance")]
    fn get_balance(&self, address: Address, _block: Option<String>) -> RpcResult<U256>;

    /// A custom "God Mode" method to set balance
    #[method(name = "tenderly_setBalance")]
    fn set_balance(&self, address: Address, amount: U256) -> RpcResult<bool>;
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
    fn chain_id(&self) -> RpcResult<U64> {
        // Wrap success in Ok()
        Ok(U64::from(31337))
    }

    fn get_balance(&self, address: Address, _block: Option<String>) -> RpcResult<U256> {
        let mut executor = self.executor.write();
        match executor.get_balance(address) {
            Ok(bal) => {
                info!("🔍 eth_getBalance({:?}) -> {}", address, bal);
                Ok(bal)
            }
            Err(e) => {
                // Log the real error to your terminal
                error!("❌ Fetch Failed: {:?}", e);

                // Return the error to the user/curl
                Err(ErrorObject::owned(
                    -32000,
                    format!("Internal Error: {:?}", e),
                    None::<()>,
                ))
            }
        }
    }

    fn set_balance(&self, address: Address, amount: U256) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        executor.set_balance(address, amount);
        info!("🧙 tenderly_setBalance({:?}) -> {}", address, amount);

        // Wrap success in Ok()
        Ok(true)
    }
}
