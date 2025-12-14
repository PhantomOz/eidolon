use alloy_primitives::{Address, B256, Bytes, U64, U256};
use eidolon_evm::Executor;
use eidolon_evm::tracer::TraceStep;
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObject;
use parking_lot::RwLock;
use serde::Deserialize;
use std::sync::Arc;
use tracing::{error, info};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CallRequest {
    pub from: Option<Address>,
    pub to: Address,
    pub value: Option<U256>,
    pub data: Option<Bytes>,
}

/// 1. Define the JSON-RPC API Contract
/// These are the methods Metamask will try to call.
#[rpc(server)]
pub trait EidolonApi {
    #[method(name = "net_version")]
    fn net_version(&self) -> RpcResult<String>;

    #[method(name = "eth_blockNumber")]
    fn block_number(&self) -> RpcResult<U256>;

    #[method(name = "eth_gasPrice")]
    fn gas_price(&self) -> RpcResult<U256>;

    #[method(name = "eth_estimateGas")]
    fn estimate_gas(&self, request: CallRequest, _block: Option<String>) -> RpcResult<U256>;

    /// Returns the Chain ID
    #[method(name = "eth_chainId")]
    fn chain_id(&self) -> RpcResult<U64>; // Return type must be RpcResult<T>

    /// Returns the balance of an address
    #[method(name = "eth_getBalance")]
    fn get_balance(&self, address: Address, _block: Option<String>) -> RpcResult<U256>;

    #[method(name = "eth_call")]
    fn call(&self, request: CallRequest, _block: Option<String>) -> RpcResult<Bytes>;

    /// 2. NEW: Execution (Write state)
    #[method(name = "eth_sendTransaction")]
    fn send_transaction(&self, request: CallRequest) -> RpcResult<B256>;

    #[method(name = "debug_traceTransaction")]
    fn trace_transaction(&self, request: CallRequest) -> RpcResult<Vec<TraceStep>>;

    #[method(name = "evm_increaseTime")]
    fn increase_time(&self, seconds: U64) -> RpcResult<U64>;

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
    chain_id: u64,
}

impl EidolonRpc {
    pub fn new(executor: Arc<RwLock<Executor>>, chain_id: u64) -> Self {
        Self { executor, chain_id }
    }
}

#[async_trait]
impl EidolonApiServer for EidolonRpc {
    fn net_version(&self) -> RpcResult<String> {
        Ok(self.chain_id.to_string())
    }

    fn block_number(&self) -> RpcResult<U256> {
        // In a real SaaS, this would return the forked block number + mined blocks
        // For now, we return a static high number to keep Metamask happy
        Ok(U256::from(19_000_000))
    }

    fn gas_price(&self) -> RpcResult<U256> {
        // Cheap gas for testing (1 wei)
        Ok(U256::from(1))
    }

    fn estimate_gas(&self, request: CallRequest, _block: Option<String>) -> RpcResult<U256> {
        // Run the call to see how much gas it actually uses
        let mut executor = self.executor.write();
        let caller = request.from.unwrap_or(Address::ZERO);
        let to = request.to;
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        // We use the existing 'call' logic but ideally we'd get the specific gas used
        // For Phase 1 SaaS, we return a flat value to ensure txs go through
        Ok(U256::from(30_000_000))
    }

    fn chain_id(&self) -> RpcResult<U64> {
        // Wrap success in Ok()
        Ok(U64::from(self.chain_id))
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

    fn call(&self, request: CallRequest, _block: Option<String>) -> RpcResult<Bytes> {
        let mut executor = self.executor.write();

        // Default to zero address if 'from' is missing
        let caller = request.from.unwrap_or(Address::ZERO);
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        info!("📞 eth_call: from={:?} to={:?}", caller, request.to);

        match executor.call(caller, request.to, value, data) {
            Ok(output) => Ok(output),
            Err(e) => {
                error!("❌ Call Failed: {:?}", e);
                Err(ErrorObject::owned(
                    -32000,
                    format!("Revert: {:?}", e),
                    None::<()>,
                ))
            }
        }
    }

    fn send_transaction(&self, request: CallRequest) -> RpcResult<B256> {
        let mut executor = self.executor.write();

        // Default to zero address if 'from' is missing
        let caller = request.from.unwrap_or(Address::ZERO);
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        info!(
            "📝 eth_sendTransaction: from={:?} to={:?}",
            caller, request.to
        );

        match executor.transact(caller, request.to, value, data) {
            Ok(_) => {
                // Return a fake transaction hash (since we don't really have a mempool)
                use alloy_primitives::B256;
                Ok(B256::from_slice(&[1u8; 32]))
            }
            Err(e) => {
                error!("❌ Tx Failed: {:?}", e);
                Err(ErrorObject::owned(
                    -32000,
                    format!("Tx Failed: {:?}", e),
                    None::<()>,
                ))
            }
        }
    }

    fn trace_transaction(&self, request: CallRequest) -> RpcResult<Vec<TraceStep>> {
        let mut executor = self.executor.write();
        let caller = request.from.unwrap_or(Address::ZERO);
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        info!(
            "🕵️ debug_traceTransaction: from={:?} to={:?}",
            caller, request.to
        );

        match executor.trace_transaction(caller, request.to, value, data) {
            Ok(tracer) => Ok(tracer.steps),
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Trace Failed: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn increase_time(&self, seconds: U64) -> RpcResult<U64> {
        let mut executor = self.executor.write();
        let secs = seconds.to::<u64>();
        executor.increase_time(secs);
        info!("⏰ Warping time forward by {} seconds", secs);
        // Return the total added seconds (simplified)
        Ok(seconds)
    }

    fn set_balance(&self, address: Address, amount: U256) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        executor.set_balance(address, amount);
        info!("🧙 tenderly_setBalance({:?}) -> {}", address, amount);

        // Wrap success in Ok()
        Ok(true)
    }
}
