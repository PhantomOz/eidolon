use alloy_consensus::{Transaction, TxEnvelope};
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::{Address, Bytes, TxKind, B256, U256, U64};
use eidolon_evm::tracer::TraceStep;
use eidolon_evm::Executor;
use jsonrpsee::core::{async_trait, RpcResult};
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::types::ErrorObject;
use parking_lot::RwLock;
use revm::primitives::Log;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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

/// A single log entry emitted during transaction execution.
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct SerializableLog {
    pub address: Address,
    pub topics: Vec<B256>,
    pub data: Bytes,
    pub block_number: U256,
    pub transaction_hash: B256,
    pub transaction_index: U64,
    pub block_hash: B256,
    pub log_index: U64,
    pub removed: bool,
}

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
    pub logs: Vec<SerializableLog>,
    pub logs_bloom: Bytes,
    pub status: U64, // 0x1 = Success, 0x0 = Failure
    pub type_: U64,
}

/// Data stored for each executed transaction.
#[derive(Debug, Clone)]
pub struct StoredTransaction {
    pub from: Address,
    pub to: Option<Address>,
    pub value: U256,
    pub block_number: U256,
    pub block_hash: B256,
    pub gas_used: u64,
    pub contract_address: Option<Address>,
    pub logs: Vec<Log>,
    pub status: bool,
}

/// Serializable transaction object returned by eth_getTransactionByHash.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct TransactionObject {
    pub hash: B256,
    pub block_hash: B256,
    pub block_number: U256,
    pub from: Address,
    pub to: Option<Address>,
    pub value: U256,
    pub gas: U256,
    pub gas_price: U256,
    pub input: Bytes,
    pub nonce: U64,
    pub transaction_index: U64,
    pub type_: U64,
}

/// Filter for eth_getLogs.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogFilter {
    pub from_block: Option<String>,
    pub to_block: Option<String>,
    pub address: Option<Address>,
    pub topics: Option<Vec<Option<B256>>>,
}

/// Fee history response for eth_feeHistory.
#[derive(Serialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct FeeHistory {
    pub oldest_block: U256,
    pub base_fee_per_gas: Vec<U256>,
    pub gas_used_ratio: Vec<f64>,
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

    #[method(name = "eth_getTransactionCount", blocking)]
    fn get_transaction_count(&self, address: Address, _block: Option<String>) -> RpcResult<U256>;

    #[method(name = "eth_getCode", blocking)]
    fn get_code(&self, address: Address, _block: Option<String>) -> RpcResult<Bytes>;

    #[method(name = "eth_gasPrice")]
    fn gas_price(&self) -> RpcResult<U256>;

    #[method(name = "eth_estimateGas", blocking)]
    fn estimate_gas(&self, request: CallRequest, _block: Option<String>) -> RpcResult<U256>;

    /// Returns the Chain ID
    #[method(name = "eth_chainId")]
    fn chain_id(&self) -> RpcResult<U64>;

    /// Returns the balance of an address
    #[method(name = "eth_getBalance", blocking)]
    fn get_balance(&self, address: Address, _block: Option<String>) -> RpcResult<U256>;

    #[method(name = "eth_call", blocking)]
    fn call(&self, request: CallRequest, _block: Option<String>) -> RpcResult<Bytes>;

    #[method(name = "eth_sendTransaction", blocking)]
    fn send_transaction(&self, request: CallRequest) -> RpcResult<B256>;

    #[method(name = "eth_sendRawTransaction", blocking)]
    fn send_raw_transaction(&self, bytes: Bytes) -> RpcResult<B256>;

    #[method(name = "debug_traceTransaction", blocking)]
    fn trace_transaction(&self, request: CallRequest) -> RpcResult<Vec<TraceStep>>;

    #[method(name = "evm_increaseTime")]
    fn increase_time(&self, seconds: U64) -> RpcResult<U64>;

    #[method(name = "tenderly_setBalance", blocking)]
    fn set_balance(&self, address: Address, amount: U256) -> RpcResult<bool>;

    #[method(name = "eth_getStorageAt", blocking)]
    fn get_storage_at(&self, address: Address, slot: U256, _block: Option<String>) -> RpcResult<U256>;

    #[method(name = "eth_getTransactionByHash")]
    fn get_transaction_by_hash(&self, hash: B256) -> RpcResult<Option<TransactionObject>>;

    #[method(name = "eth_getBlockByHash")]
    fn get_block_by_hash(&self, hash: B256, full_tx: bool) -> RpcResult<Option<MockBlock>>;

    #[method(name = "eth_getLogs")]
    fn get_logs(&self, filter: LogFilter) -> RpcResult<Vec<SerializableLog>>;

    #[method(name = "eth_feeHistory")]
    fn fee_history(&self, block_count: U64, newest_block: String, reward_percentiles: Option<Vec<f64>>) -> RpcResult<FeeHistory>;

    #[method(name = "eth_maxPriorityFeePerGas")]
    fn max_priority_fee_per_gas(&self) -> RpcResult<U256>;

    #[method(name = "eth_syncing")]
    fn syncing(&self) -> RpcResult<bool>;

    #[method(name = "web3_clientVersion")]
    fn client_version(&self) -> RpcResult<String>;

    #[method(name = "net_listening")]
    fn net_listening(&self) -> RpcResult<bool>;

    #[method(name = "eth_accounts")]
    fn accounts(&self) -> RpcResult<Vec<Address>>;

    #[method(name = "eth_mining")]
    fn mining(&self) -> RpcResult<bool>;

    #[method(name = "evm_snapshot")]
    fn evm_snapshot(&self) -> RpcResult<U64>;

    #[method(name = "evm_revert")]
    fn evm_revert(&self, id: U64) -> RpcResult<bool>;
}

pub struct EidolonRpc {
    executor: Arc<RwLock<Executor>>,
    chain_id: u64,
    transactions: Arc<RwLock<HashMap<B256, StoredTransaction>>>,
}

impl EidolonRpc {
    pub fn new(executor: Arc<RwLock<Executor>>, chain_id: u64) -> Self {
        Self {
            executor,
            chain_id,
            transactions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store a transaction and build its receipt data after execution.
    fn store_transaction(
        &self,
        tx_hash: B256,
        from: Address,
        to: Option<Address>,
        value: U256,
        result: &revm::primitives::ExecutionResult,
        block_number: U256,
    ) {
        let (gas_used, logs, contract_address, status) = match result {
            revm::primitives::ExecutionResult::Success {
                gas_used,
                logs,
                output,
                ..
            } => {
                let contract_addr = match output {
                    revm::primitives::Output::Create(_, addr) => *addr,
                    _ => None,
                };
                (*gas_used, logs.clone(), contract_addr, true)
            }
            revm::primitives::ExecutionResult::Revert { gas_used, .. } => {
                (*gas_used, vec![], None, false)
            }
            revm::primitives::ExecutionResult::Halt { gas_used, .. } => {
                (*gas_used, vec![], None, false)
            }
        };

        let block_hash = B256::from(block_number.to_be_bytes::<32>());

        let stored = StoredTransaction {
            from,
            to,
            value,
            block_number,
            block_hash,
            gas_used,
            contract_address,
            logs,
            status,
        };

        self.transactions.write().insert(tx_hash, stored);
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
        let txs = self.transactions.read();
        let stored = match txs.get(&hash) {
            Some(tx) => tx,
            None => return Ok(None), // Unknown transaction hash
        };

        let logs: Vec<SerializableLog> = stored
            .logs
            .iter()
            .enumerate()
            .map(|(i, log)| SerializableLog {
                address: log.address,
                topics: log.topics().to_vec(),
                data: log.data.data.clone(),
                block_number: stored.block_number,
                transaction_hash: hash,
                transaction_index: U64::ZERO,
                block_hash: stored.block_hash,
                log_index: U64::from(i as u64),
                removed: false,
            })
            .collect();

        let receipt = TransactionReceipt {
            transaction_hash: hash,
            transaction_index: U64::ZERO,
            block_hash: stored.block_hash,
            block_number: stored.block_number,
            from: stored.from,
            to: stored.to,
            cumulative_gas_used: U256::from(stored.gas_used),
            gas_used: U256::from(stored.gas_used),
            contract_address: stored.contract_address,
            logs,
            logs_bloom: Bytes::from_static(&[0u8; 256]),
            status: if stored.status { U64::from(1) } else { U64::ZERO },
            type_: U64::from(2), // EIP-1559
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
                // Auto-mine a block
                executor.block_env.number += U256::from(1);
                executor.block_env.timestamp += U256::from(12);
                let block_number = executor.block_env.number;

                info!("⛏️ Mined Virtual Block: {}", block_number);

                // Generate tx hash from time
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .subsec_nanos();
                let mut hash_bytes = [0u8; 32];
                hash_bytes[0..4].copy_from_slice(&nanos.to_be_bytes());
                let tx_hash = B256::from(hash_bytes);

                // Store the transaction
                drop(executor); // Release write lock before storing
                self.store_transaction(
                    tx_hash,
                    caller,
                    request.to,
                    value,
                    &result,
                    block_number,
                );

                Ok(tx_hash)
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
                if !result.is_success() {
                    error!("❌ Transaction Reverted: {:?}", result);
                    return Err(ErrorObject::owned(
                        -3,
                        "Execution Reverted: Check Token Balance",
                        None::<()>,
                    ));
                }

                executor.block_env.number += U256::from(1);
                executor.block_env.timestamp += U256::from(12);
                let block_number = executor.block_env.number;
                info!("⛏️ Mined Virtual Block: {}", block_number);

                // Store the transaction
                drop(executor); // Release write lock before storing
                self.store_transaction(tx_hash, caller, to, value, &result, block_number);

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
        Ok(true)
    }

    fn get_storage_at(&self, address: Address, slot: U256, _block: Option<String>) -> RpcResult<U256> {
        let mut executor = self.executor.write();
        match executor.get_storage_at(address, slot) {
            Ok(val) => Ok(val),
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Internal Error: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn get_transaction_by_hash(&self, hash: B256) -> RpcResult<Option<TransactionObject>> {
        let txs = self.transactions.read();
        match txs.get(&hash) {
            Some(stored) => {
                let obj = TransactionObject {
                    hash,
                    block_hash: stored.block_hash,
                    block_number: stored.block_number,
                    from: stored.from,
                    to: stored.to,
                    value: stored.value,
                    gas: U256::from(stored.gas_used),
                    gas_price: U256::from(1),
                    input: Bytes::default(),
                    nonce: U64::ZERO,
                    transaction_index: U64::ZERO,
                    type_: U64::from(2),
                };
                Ok(Some(obj))
            }
            None => Ok(None),
        }
    }

    fn get_block_by_hash(&self, _hash: B256, _full_tx: bool) -> RpcResult<Option<MockBlock>> {
        // Return the current virtual block regardless of hash
        // A full implementation would maintain block history
        let executor = self.executor.read();
        let env = &executor.block_env;

        let block = MockBlock {
            number: env.number,
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

    fn get_logs(&self, filter: LogFilter) -> RpcResult<Vec<SerializableLog>> {
        let txs = self.transactions.read();
        let mut result_logs = Vec::new();

        for (tx_hash, stored) in txs.iter() {
            // Filter by block range if specified
            if let Some(ref from_str) = filter.from_block {
                if from_str != "latest" && from_str != "pending" {
                    if let Ok(from_num) = u64::from_str_radix(from_str.trim_start_matches("0x"), 16) {
                        let block_num: u64 = stored.block_number.to::<u64>();
                        if block_num < from_num {
                            continue;
                        }
                    }
                }
            }
            if let Some(ref to_str) = filter.to_block {
                if to_str != "latest" && to_str != "pending" {
                    if let Ok(to_num) = u64::from_str_radix(to_str.trim_start_matches("0x"), 16) {
                        let block_num: u64 = stored.block_number.to::<u64>();
                        if block_num > to_num {
                            continue;
                        }
                    }
                }
            }

            for (i, log) in stored.logs.iter().enumerate() {
                // Filter by address if specified
                if let Some(filter_addr) = filter.address {
                    if log.address != filter_addr {
                        continue;
                    }
                }

                // Filter by topics if specified
                if let Some(ref topics) = filter.topics {
                    let mut matches = true;
                    for (j, topic_filter) in topics.iter().enumerate() {
                        if let Some(expected) = topic_filter {
                            if j >= log.topics().len() || log.topics()[j] != *expected {
                                matches = false;
                                break;
                            }
                        }
                    }
                    if !matches {
                        continue;
                    }
                }

                result_logs.push(SerializableLog {
                    address: log.address,
                    topics: log.topics().to_vec(),
                    data: log.data.data.clone(),
                    block_number: stored.block_number,
                    transaction_hash: *tx_hash,
                    transaction_index: U64::ZERO,
                    block_hash: stored.block_hash,
                    log_index: U64::from(i as u64),
                    removed: false,
                });
            }
        }

        Ok(result_logs)
    }

    fn fee_history(&self, block_count: U64, _newest_block: String, _reward_percentiles: Option<Vec<f64>>) -> RpcResult<FeeHistory> {
        let executor = self.executor.read();
        let current_block = executor.block_env.number;
        let count = block_count.to::<u64>().min(1024) as usize;

        // Return minimal fee history with 1 wei base fee
        let base_fees = vec![U256::from(1); count + 1];
        let ratios = vec![0.5_f64; count];

        let oldest = if current_block > U256::from(count) {
            current_block - U256::from(count)
        } else {
            U256::ZERO
        };

        Ok(FeeHistory {
            oldest_block: oldest,
            base_fee_per_gas: base_fees,
            gas_used_ratio: ratios,
        })
    }

    fn max_priority_fee_per_gas(&self) -> RpcResult<U256> {
        Ok(U256::from(1))
    }

    fn syncing(&self) -> RpcResult<bool> {
        Ok(false)
    }

    fn client_version(&self) -> RpcResult<String> {
        Ok("Eidolon/0.1.0".to_string())
    }

    fn net_listening(&self) -> RpcResult<bool> {
        Ok(true)
    }

    fn accounts(&self) -> RpcResult<Vec<Address>> {
        Ok(vec![])
    }

    fn mining(&self) -> RpcResult<bool> {
        Ok(true)
    }

    fn evm_snapshot(&self) -> RpcResult<U64> {
        let mut executor = self.executor.write();
        let id = executor.take_snapshot();
        info!("📸 Snapshot taken: id={}", id);
        Ok(U64::from(id))
    }

    fn evm_revert(&self, id: U64) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        let snapshot_id = id.to::<u64>();
        let success = executor.revert_snapshot(snapshot_id);
        if success {
            info!("⏪ Reverted to snapshot: id={}", snapshot_id);
        } else {
            error!("❌ Failed to revert to snapshot: id={}", snapshot_id);
        }
        Ok(success)
    }
}
