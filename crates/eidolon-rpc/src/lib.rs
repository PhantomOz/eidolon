use alloy_consensus::{Transaction, TxEnvelope};
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::{Address, Bytes, TxKind, B256, U256, U64};
use eidolon_evm::tracer::TraceStep;
use eidolon_evm::Executor;
use jsonrpsee::core::{async_trait, RpcResult};
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
    pub to: Option<Address>,
    pub value: Option<U256>,
    pub data: Option<Bytes>,
}

use eidolon_types::MockBlock;

#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    pub transaction_hash: B256,
    pub transaction_index: U64,
    pub block_hash: B256,
    pub block_number: U256,
    pub from: Address,
    pub to: Option<Address>,
    pub cumulative_gas_used: U256,
    pub gas_used: U256,
    pub contract_address: Option<Address>,
    pub logs: Vec<()>,
    pub logs_bloom: Bytes,
    pub status: U64, // 0x1 = Success, 0x0 = Failure
    pub type_: U64,
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

    #[method(name = "eth_getTransactionReceipt")]
    fn get_transaction_receipt(&self, hash: B256) -> RpcResult<Option<TransactionReceipt>>;

    #[method(name = "eth_getTransactionCount")]
    fn get_transaction_count(&self, address: Address, _block: Option<String>) -> RpcResult<U256>;

    #[method(name = "eth_getCode")]
    fn get_code(&self, address: Address, _block: Option<String>) -> RpcResult<Bytes>;

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

    #[method(name = "eth_sendRawTransaction")]
    fn send_raw_transaction(&self, bytes: Bytes) -> RpcResult<B256>;

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

    fn get_transaction_receipt(&self, hash: B256) -> RpcResult<Option<TransactionReceipt>> {
        let executor = self.executor.read();
        let env = &executor.block_env;
        let hash_seed = env.number.to_be_bytes::<32>();
        let block_hash = B256::from(hash_seed);

        // We assume ANY receipt requested is a success for the current block.
        // In a full implementation, we would store executed TX hashes in a map.
        let receipt = TransactionReceipt {
            transaction_hash: hash,
            transaction_index: U64::ZERO,
            block_hash,
            block_number: env.number, // The current virtual block
            from: Address::ZERO, // We can't recover 'from' without storing the tx, but MM often ignores this
            to: None,
            cumulative_gas_used: U256::from(21000),
            gas_used: U256::from(21000),
            contract_address: None,
            logs: vec![],
            logs_bloom: Bytes::from_static(&[0u8; 256]),
            status: U64::from(1), // Success!
            type_: U64::from(2),  // EIP-1559
        };

        Ok(Some(receipt))
    }

    fn get_transaction_count(&self, address: Address, _block: Option<String>) -> RpcResult<U256> {
        let mut executor = self.executor.write();
        match executor.get_nonce(address) {
            Ok(nonce) => Ok(U256::from(nonce)),
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Internal Error: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn get_code(&self, address: Address, _block: Option<String>) -> RpcResult<Bytes> {
        let mut executor = self.executor.write();
        match executor.get_code(address) {
            Ok(code) => Ok(code),
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Internal Error: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn gas_price(&self) -> RpcResult<U256> {
        //TODO: Cheap gas for testing (1 wei)
        Ok(U256::from(1))
    }

    fn estimate_gas(&self, request: CallRequest, _block: Option<String>) -> RpcResult<U256> {
        // Run the call to see how much gas it actually uses
        let mut executor = self.executor.write();
        let caller = request.from.unwrap_or(Address::ZERO);
        let to = request.to;
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        match executor.estimate_gas(caller, to, value, data) {
            Ok(gas_used) => {
                // Return gas used + 20% buffer
                let gas_estimate = gas_used as u128 * 120 / 100;
                Ok(U256::from(gas_estimate))
            }
            Err(e) => {
                // It failed (e.g. Insufficient Funds). Log it!
                error!("❌ eth_estimateGas Failed: {:?}", e);
                // Return the error to MetaMask
                Err(ErrorObject::owned(
                    -32000,
                    format!("Estimate failed: {:?}", e),
                    None::<()>,
                ))
            }
        }
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

        info!(
            "📞 eth_call: from={:?} to={:?} value={:?} data={:?}",
            caller, request.to, value, data
        );

        match executor.call(caller, request.to, value, data) {
            Ok(output) => {
                info!("✅ Call Succeeded output={:?}", output);
                Ok(output)
            }
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
            Ok(result) => {
                if !result.is_success() {
                    error!("❌ EVM Reverted: {:?}", result);
                    return Err(ErrorObject::owned(-32000, "Execution Reverted", None::<()>));
                }
                // FIX: Auto-mine a block so MetaMask sees the change!
                executor.block_env.number += U256::from(1);
                executor.block_env.timestamp += U256::from(12); // Add 12 seconds

                info!("⛏️ Mined Virtual Block: {}", executor.block_env.number);

                // Return a pseudo-random hash based on time
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos();
                let mut hash_bytes = [0u8; 32];
                hash_bytes[0..4].copy_from_slice(&nanos.to_be_bytes());
                Ok(B256::from(hash_bytes))
            }
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Tx Failed: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn send_raw_transaction(&self, bytes: Bytes) -> RpcResult<B256> {
        let tx_hash = alloy_primitives::keccak256(&bytes);
        let mut b = bytes.as_ref();

        // 1. Decode
        let tx = match TxEnvelope::decode_2718(&mut b) {
            Ok(t) => t,
            Err(e) => {
                error!("❌ Failed to decode Raw Transaction: {:?}", e);
                return Err(ErrorObject::owned(-32000, "Invalid RLP", None::<()>));
            }
        };

        // 2. Recover Signer
        let caller = match tx.recover_signer() {
            Ok(addr) => addr,
            Err(e) => {
                error!("❌ Signature Recovery Failed: {:?}", e);
                return Err(ErrorObject::owned(-32000, "Invalid Signature", None::<()>));
            }
        };

        let to = match tx.kind() {
            TxKind::Call(addr) => Some(addr),
            TxKind::Create => None,
        };
        let value = tx.value();
        let data = tx.input().clone();

        info!("📝 eth_sendRawTransaction: from={:?} to={:?} hash={:?}", caller, to, tx_hash);

        let mut executor = self.executor.write();
        match executor.transact(caller, to, value, data) {
            Ok(result) => {
                // FIX: Check for REVERT here!
                if !result.is_success() {
                    error!("❌ Transaction Reverted: {:?}", result);
                    // This error message will show up in MetaMask!
                    return Err(ErrorObject::owned(
                        -3,
                        "Execution Reverted: Check Token Balance",
                        None::<()>,
                    ));
                }

                executor.block_env.number += U256::from(1);
                executor.block_env.timestamp += U256::from(12);
                info!("⛏️ Mined Virtual Block: {}", executor.block_env.number);
                Ok(tx_hash)
            }
            Err(e) => {
                error!("❌ Execution Error: {:?}", e);
                Err(ErrorObject::owned(
                    -32000,
                    format!("Tx Execution Error: {:?}", e),
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
