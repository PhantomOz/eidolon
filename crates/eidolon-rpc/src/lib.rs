use alloy_primitives::{Address, B256, Bytes, U64, U256};
use eidolon_evm::Executor;
use eidolon_evm::tracer::TraceStep;
use jsonrpsee::core::{RpcResult, async_trait};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObject;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
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

/// A Fake Block to satisfy MetaMask
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MockBlock {
    pub number: U256,
    pub hash: B256,
    pub parent_hash: B256,
    pub nonce: U64,
    pub sha3_uncles: B256,
    pub logs_bloom: Bytes,
    pub transactions_root: B256,
    pub state_root: B256,
    pub receipts_root: B256,
    pub miner: Address,
    pub difficulty: U256,
    pub total_difficulty: U256,
    pub extra_data: Bytes,
    pub size: U256,
    pub gas_limit: U256,
    pub gas_used: U256,
    pub timestamp: U256,
    // We return empty transactions for now to keep it simple
    pub transactions: Vec<B256>,
    pub uncles: Vec<B256>,
}

#[rpc(server)]
pub trait EidolonApi {
    #[method(name = "net_version")]
    fn net_version(&self) -> RpcResult<String>;

    #[method(name = "eth_blockNumber")]
    fn block_number(&self) -> RpcResult<U256>;

    #[method(name = "eth_getBlockByNumber")]
    fn get_block_by_number(&self, block_tag: String, full_tx: bool)
    -> RpcResult<Option<MockBlock>>;

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

    #[method(name = "eth_sendTransaction")]
    fn send_transaction(&self, request: CallRequest) -> RpcResult<B256>;

    #[method(name = "debug_traceTransaction")]
    fn trace_transaction(&self, request: CallRequest) -> RpcResult<Vec<TraceStep>>;

    #[method(name = "evm_increaseTime")]
    fn increase_time(&self, seconds: U64) -> RpcResult<U64>;

    #[method(name = "tenderly_setBalance")]
    fn set_balance(&self, address: Address, amount: U256) -> RpcResult<bool>;
}

pub struct EidolonRpc {
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
        let executor = self.executor.read();
        let num = executor.block_env.number;
        Ok(num)
    }

    fn get_block_by_number(
        &self,
        _block_tag: String,
        _full_tx: bool,
    ) -> RpcResult<Option<MockBlock>> {
        // We act as if every request is for the "Current Virtual Block"
        let executor = self.executor.read();
        let env = &executor.block_env;

        let block = MockBlock {
            number: env.number,
            // We fake a hash. In a real node this is calculated from data.
            hash: B256::repeat_byte(0xaa),
            parent_hash: B256::repeat_byte(0xbb),
            nonce: U64::ZERO,
            sha3_uncles: B256::ZERO,
            logs_bloom: Bytes::from_static(&[0u8; 256]),
            transactions_root: B256::ZERO,
            state_root: B256::ZERO,
            receipts_root: B256::ZERO,
            miner: Address::ZERO,
            difficulty: U256::ZERO,
            total_difficulty: U256::ZERO,
            extra_data: Bytes::default(),
            size: U256::from(1000),
            gas_limit: U256::from(30_000_000),
            gas_used: U256::ZERO,
            timestamp: env.timestamp,
            transactions: vec![],
            uncles: vec![],
        };

        Ok(Some(block))
    }

    fn gas_price(&self) -> RpcResult<U256> {
        //TODO: Cheap gas for testing (1 wei)
        Ok(U256::from(1))
    }

    fn estimate_gas(&self, request: CallRequest, _block: Option<String>) -> RpcResult<U256> {
        // Run the call to see how much gas it actually uses
        let mut _executor = self.executor.write();
        let _caller = request.from.unwrap_or(Address::ZERO);
        let _to = request.to;
        let _value = request.value.unwrap_or(U256::ZERO);
        let _data = request.data.unwrap_or_default();

        // TODO: We use the existing 'call' logic but ideally we'd get the specific gas used
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
                error!("❌ Fetch Failed: {:?}", e);

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
                // FIX: Auto-mine a block so MetaMask sees the change!
                executor.block_env.number += U256::from(1);
                executor.block_env.timestamp += U256::from(12); // Add 12 seconds

                info!("⛏️ Mined Virtual Block: {}", executor.block_env.number);
                Ok(B256::from_slice(&[1u8; 32]))
            }
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Tx Failed: {:?}", e),
                None::<()>,
            )),
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

        executor.block_env.number += U256::from(1);
        executor.block_env.timestamp += U256::from(12);

        info!(
            "🧙 tenderly_setBalance -> {} (Mined Block {})",
            amount, executor.block_env.number
        );
        // Wrap success in Ok()
        Ok(true)
    }
}
