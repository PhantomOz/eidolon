use alloy_consensus::{Transaction, TxEnvelope};
use alloy_eips::eip2718::Decodable2718;
use alloy_primitives::{Address, Bytes, TxKind, B256, U256, U64, keccak256};
use eidolon_evm::tracer::TraceStep;
use eidolon_evm::{BundleSimulationResult, Executor, SimulationResult};
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
    pub input: Bytes,
    pub nonce: u64,
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

/// Address filter: supports single address or array of addresses.
#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum AddressFilter {
    Single(Address),
    Multiple(Vec<Address>),
}

impl AddressFilter {
    pub fn matches(&self, addr: &Address) -> bool {
        match self {
            AddressFilter::Single(a) => a == addr,
            AddressFilter::Multiple(addrs) => addrs.contains(addr),
        }
    }
}

/// Filter for eth_getLogs.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct LogFilter {
    pub from_block: Option<String>,
    pub to_block: Option<String>,
    pub address: Option<AddressFilter>,
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

/// Parameters for eidolon_reset.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ResetParams {
    pub forking: Option<ForkingParams>,
}

/// Forking parameters for eidolon_reset.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ForkingParams {
    pub json_rpc_url: Option<String>,
    pub block_number: Option<u64>,
}

/// A virtual block produced by the testnet.
#[derive(Debug, Clone, Serialize)]
pub struct VirtualBlock {
    pub number: U256,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: U256,
    pub transactions: Vec<B256>,
    pub gas_used: U256,
    pub gas_limit: U256,
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
    fn trace_transaction(&self, hash: B256) -> RpcResult<Vec<TraceStep>>;

    #[method(name = "debug_traceCall", blocking)]
    fn trace_call(&self, request: CallRequest) -> RpcResult<Vec<TraceStep>>;

    #[method(name = "evm_increaseTime")]
    fn increase_time(&self, seconds: U64) -> RpcResult<U64>;

    #[method(name = "eidolon_setBalance", blocking)]
    fn set_balance(&self, address: Address, amount: U256) -> RpcResult<bool>;

    #[method(name = "eidolon_setErc20Balance", blocking)]
    fn set_erc20_balance(&self, token: Address, target: Address, amount: U256) -> RpcResult<bool>;

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

    // --- Cheatcodes: Block manipulation ---

    #[method(name = "evm_mine")]
    fn evm_mine(&self) -> RpcResult<U64>;

    #[method(name = "evm_setNextBlockTimestamp")]
    fn evm_set_next_block_timestamp(&self, timestamp: U64) -> RpcResult<U64>;

    #[method(name = "evm_setAutomine")]
    fn evm_set_automine(&self, enabled: bool) -> RpcResult<bool>;

    #[method(name = "evm_setBlockGasLimit")]
    fn evm_set_block_gas_limit(&self, gas_limit: U64) -> RpcResult<bool>;

    // --- Cheatcodes: State manipulation ---

    #[method(name = "eidolon_setCode", blocking)]
    fn eidolon_set_code(&self, address: Address, code: Bytes) -> RpcResult<bool>;

    #[method(name = "eidolon_setNonce", blocking)]
    fn eidolon_set_nonce(&self, address: Address, nonce: U64) -> RpcResult<bool>;

    #[method(name = "eidolon_setStorageAt", blocking)]
    fn eidolon_set_storage_at(&self, address: Address, slot: U256, value: U256) -> RpcResult<bool>;

    // --- Cheatcodes: Impersonation ---

    #[method(name = "eidolon_impersonateAccount")]
    fn eidolon_impersonate_account(&self, address: Address) -> RpcResult<bool>;

    #[method(name = "eidolon_stopImpersonatingAccount")]
    fn eidolon_stop_impersonating_account(&self, address: Address) -> RpcResult<bool>;

    // --- Cheatcodes: Fork management ---

    #[method(name = "eidolon_reset", blocking)]
    fn eidolon_reset(&self, params: Option<ResetParams>) -> RpcResult<bool>;

    #[method(name = "eidolon_simulateTransaction", blocking)]
    fn simulate_transaction(&self, request: CallRequest) -> RpcResult<SimulationResult>;

    #[method(name = "eidolon_simulateBundle", blocking)]
    fn simulate_bundle(&self, transactions: Vec<CallRequest>) -> RpcResult<BundleSimulationResult>;

    // --- Cheatcodes: Anvil-compatible aliases ---

    #[method(name = "anvil_setBalance", blocking)]
    fn anvil_set_balance(&self, address: Address, balance: U256) -> RpcResult<bool>;

    #[method(name = "anvil_setCode", blocking)]
    fn anvil_set_code(&self, address: Address, code: Bytes) -> RpcResult<bool>;

    #[method(name = "anvil_setNonce", blocking)]
    fn anvil_set_nonce(&self, address: Address, nonce: U64) -> RpcResult<bool>;

    #[method(name = "anvil_setStorageAt", blocking)]
    fn anvil_set_storage_at(&self, address: Address, slot: U256, value: U256) -> RpcResult<bool>;

    #[method(name = "anvil_impersonateAccount")]
    fn anvil_impersonate_account(&self, address: Address) -> RpcResult<bool>;

    #[method(name = "anvil_stopImpersonatingAccount")]
    fn anvil_stop_impersonating_account(&self, address: Address) -> RpcResult<bool>;

    #[method(name = "anvil_mine")]
    fn anvil_mine(&self, count: Option<U64>, interval: Option<U64>) -> RpcResult<bool>;

    #[method(name = "anvil_setBlockTimestampInterval")]
    fn anvil_set_block_timestamp_interval(&self, seconds: U64) -> RpcResult<bool>;

    #[method(name = "anvil_autoImpersonateAccount")]
    fn anvil_auto_impersonate_account(&self, enabled: bool) -> RpcResult<bool>;
}

pub struct EidolonRpc {
    executor: Arc<RwLock<Executor>>,
    chain_id: u64,
    transactions: Arc<RwLock<HashMap<B256, StoredTransaction>>>,
    auto_impersonate: Arc<RwLock<bool>>,
    block_timestamp_interval: Arc<RwLock<Option<u64>>>,
    blocks: Arc<RwLock<Vec<VirtualBlock>>>,
    block_hashes: Arc<RwLock<HashMap<B256, usize>>>,
}

impl EidolonRpc {
    pub fn new(executor: Arc<RwLock<Executor>>, chain_id: u64) -> Self {
        let (genesis_number, genesis_timestamp) = {
            let exec = executor.read();
            (exec.block_env.number, exec.block_env.timestamp)
        };

        let genesis_hash = Self::compute_block_hash(genesis_number, genesis_timestamp, B256::ZERO);
        let genesis = VirtualBlock {
            number: genesis_number,
            hash: genesis_hash,
            parent_hash: B256::ZERO,
            timestamp: genesis_timestamp,
            transactions: vec![],
            gas_used: U256::ZERO,
            gas_limit: U256::from(30_000_000),
        };

        let mut block_hash_map = HashMap::new();
        block_hash_map.insert(genesis_hash, 0);

        Self {
            executor,
            chain_id,
            transactions: Arc::new(RwLock::new(HashMap::new())),
            auto_impersonate: Arc::new(RwLock::new(false)),
            block_timestamp_interval: Arc::new(RwLock::new(None)),
            blocks: Arc::new(RwLock::new(vec![genesis])),
            block_hashes: Arc::new(RwLock::new(block_hash_map)),
        }
    }

    /// Expose transactions for the REST API
    pub fn expose_transactions(&self) -> Arc<RwLock<HashMap<B256, StoredTransaction>>> {
        self.transactions.clone()
    }

    /// Expose blocks for the REST API
    pub fn expose_blocks(&self) -> Arc<RwLock<Vec<VirtualBlock>>> {
        self.blocks.clone()
    }

    /// Compute a deterministic block hash from block metadata.
    fn compute_block_hash(number: U256, timestamp: U256, parent_hash: B256) -> B256 {
        let mut input = Vec::with_capacity(96);
        input.extend_from_slice(&number.to_be_bytes::<32>());
        input.extend_from_slice(&timestamp.to_be_bytes::<32>());
        input.extend_from_slice(parent_hash.as_slice());
        keccak256(&input)
    }

    /// Record a new virtual block in history.
    fn record_block(&self, number: U256, timestamp: U256, transactions: Vec<B256>, gas_used: U256) {
        let parent_hash = {
            let blocks = self.blocks.read();
            blocks.last().map(|b| b.hash).unwrap_or(B256::ZERO)
        };

        let hash = Self::compute_block_hash(number, timestamp, parent_hash);

        let block = VirtualBlock {
            number,
            hash,
            parent_hash,
            timestamp,
            transactions,
            gas_used,
            gas_limit: U256::from(30_000_000),
        };

        let mut blocks = self.blocks.write();
        let idx = blocks.len();
        blocks.push(block);
        drop(blocks);
        self.block_hashes.write().insert(hash, idx);
    }

    /// Convert a VirtualBlock to MockBlock for RPC responses.
    fn block_to_mock(vblock: &VirtualBlock) -> MockBlock {
        MockBlock {
            number: vblock.number,
            hash: vblock.hash,
            parent_hash: vblock.parent_hash,
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
            gas_limit: vblock.gas_limit,
            gas_used: vblock.gas_used,
            timestamp: vblock.timestamp,
            transactions: vblock.transactions.clone(),
            uncles: vec![],
        }
    }

    /// Record a synthetic cheatcode transaction and mine a block automatically.
    fn record_cheatcode_tx(&self, from: Address, input: String) {
        let mut executor = self.executor.write();
        executor.block_env.number += U256::from(1);
        executor.block_env.timestamp += U256::from(12); // advance time exactly one block
        let block_num = executor.block_env.number;
        let timestamp = executor.block_env.timestamp;
        drop(executor);

        let parent_hash = {
            let blocks = self.blocks.read();
            blocks.last().map(|b| b.hash).unwrap_or(B256::ZERO)
        };
        let block_hash = Self::compute_block_hash(block_num, timestamp, parent_hash);

        let mut buf = Vec::new();
        buf.extend_from_slice(&block_num.to_be_bytes::<32>());
        buf.extend_from_slice(input.as_bytes());
        let tx_hash = alloy_primitives::keccak256(&buf);

        let stored = StoredTransaction {
            from,
            to: None,
            value: U256::ZERO,
            input: Bytes::from(input.as_bytes().to_vec()),
            nonce: 0,
            block_number: block_num,
            block_hash,
            gas_used: 21000,
            contract_address: None,
            logs: vec![],
            status: true,
        };

        self.transactions.write().insert(tx_hash, stored);
        self.record_block(block_num, timestamp, vec![tx_hash], U256::from(21000));
    }

    /// Store a transaction and build its receipt data after execution.
    fn store_transaction(
        &self,
        tx_hash: B256,
        from: Address,
        to: Option<Address>,
        value: U256,
        input: Bytes,
        nonce: u64,
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

        // Look up block hash from recorded blocks
        let block_hash = {
            let blocks = self.blocks.read();
            blocks.iter().rev().find(|b| b.number == block_number)
                .map(|b| b.hash)
                .unwrap_or_else(|| B256::from(block_number.to_be_bytes::<32>()))
        };

        let stored = StoredTransaction {
            from,
            to,
            value,
            input,
            nonce,
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
        block_tag: String,
        _full_tx: bool,
    ) -> RpcResult<Option<MockBlock>> {
        let blocks = self.blocks.read();

        if block_tag == "latest" || block_tag == "pending" {
            return Ok(blocks.last().map(|b| Self::block_to_mock(b)));
        }

        if block_tag == "earliest" {
            return Ok(blocks.first().map(|b| Self::block_to_mock(b)));
        }

        // Parse hex block number
        if let Ok(num) = u64::from_str_radix(block_tag.trim_start_matches("0x"), 16) {
            let target = U256::from(num);
            let block = blocks.iter().find(|b| b.number == target);
            return Ok(block.map(|b| Self::block_to_mock(b)));
        }

        // Fallback: return latest
        Ok(blocks.last().map(|b| Self::block_to_mock(b)))
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

        let caller = request.from.unwrap_or(Address::ZERO);
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        info!(
            "📝 eth_sendTransaction: from={:?} to={:?}",
            caller, request.to
        );

        // Get nonce before transaction for deterministic hashing
        let nonce = executor.get_nonce(caller).unwrap_or(0);

        match executor.transact(caller, request.to, value, data.clone()) {
            Ok(result) => {
                if !result.is_success() {
                    error!("❌ EVM Reverted: {:?}", result);
                    return Err(ErrorObject::owned(-32000, "Execution Reverted", None::<()>));
                }

                let should_mine = executor.automine;
                if should_mine {
                    executor.mine_one_block();
                }
                let block_number = executor.block_env.number;
                let block_timestamp = executor.block_env.timestamp;
                let gas_used = match &result {
                    revm::primitives::ExecutionResult::Success { gas_used, .. } => *gas_used,
                    _ => 0,
                };

                info!("⛏️ Mined Virtual Block: {}", block_number);

                // Deterministic tx hash: keccak256(caller ++ nonce ++ to ++ value ++ data)
                let mut hash_input = Vec::new();
                hash_input.extend_from_slice(caller.as_slice());
                hash_input.extend_from_slice(&nonce.to_be_bytes());
                if let Some(to) = request.to {
                    hash_input.extend_from_slice(to.as_slice());
                }
                hash_input.extend_from_slice(&value.to_be_bytes::<32>());
                hash_input.extend_from_slice(&data);
                let tx_hash = keccak256(&hash_input);

                drop(executor);

                if should_mine {
                    self.record_block(block_number, block_timestamp, vec![tx_hash], U256::from(gas_used));
                }
                self.store_transaction(tx_hash, caller, request.to, value, data, nonce, &result, block_number);

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
        let tx_nonce = tx.nonce();

        info!("📝 eth_sendRawTransaction: from={:?} to={:?} hash={:?}", caller, to, tx_hash);

        let mut executor = self.executor.write();
        match executor.transact(caller, to, value, data.clone()) {
            Ok(result) => {
                if !result.is_success() {
                    error!("❌ Transaction Reverted: {:?}", result);
                    return Err(ErrorObject::owned(
                        -3,
                        "Execution Reverted: Check Token Balance",
                        None::<()>,
                    ));
                }

                let should_mine = executor.automine;
                if should_mine {
                    executor.mine_one_block();
                }
                let block_number = executor.block_env.number;
                let block_timestamp = executor.block_env.timestamp;
                let gas_used = match &result {
                    revm::primitives::ExecutionResult::Success { gas_used, .. } => *gas_used,
                    _ => 0,
                };
                info!("⛏️ Mined Virtual Block: {}", block_number);

                drop(executor);

                if should_mine {
                    self.record_block(block_number, block_timestamp, vec![tx_hash], U256::from(gas_used));
                }
                self.store_transaction(tx_hash, caller, to, value, data, tx_nonce, &result, block_number);

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

    fn trace_transaction(&self, hash: B256) -> RpcResult<Vec<TraceStep>> {
        let stored = {
            let txs = self.transactions.read();
            match txs.get(&hash) {
                Some(tx) => tx.clone(),
                None => return Err(ErrorObject::owned(-32000, "Transaction not found", None::<()>)),
            }
        };

        info!("🕵️ debug_traceTransaction: hash={:?}", hash);

        let mut executor = self.executor.write();
        match executor.trace_transaction(stored.from, stored.to, stored.value, stored.input) {
            Ok(tracer) => Ok(tracer.steps),
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Trace Failed: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn trace_call(&self, request: CallRequest) -> RpcResult<Vec<TraceStep>> {
        let mut executor = self.executor.write();
        let caller = request.from.unwrap_or(Address::ZERO);
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        info!(
            "🕵️ debug_traceCall: from={:?} to={:?}",
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
        drop(executor);
        self.record_cheatcode_tx(address, format!("eidolon_setBalance({:?}, {})", address, amount));
        info!("🧙 eidolon_setBalance({:?}) -> {}", address, amount);
        Ok(true)
    }

    fn set_erc20_balance(&self, token: Address, target: Address, amount: U256) -> RpcResult<bool> {
        let mut executor = self.executor.write();

        let mut slot_found = None;

        // Try slot 0 to 50
        for slot in 0..50 {
            let slot_u256 = U256::from(slot);
            
            // keystring: keccak256(abi.encode(target, slot_u256))
            let mut buf = [0u8; 64];
            buf[12..32].copy_from_slice(target.as_slice());
            buf[32..64].copy_from_slice(&slot_u256.to_be_bytes::<32>());
            let storage_key = alloy_primitives::keccak256(&buf);
            
            let key_u256 = U256::from_be_bytes(storage_key.0);
            
            // Backup old
            let old_val = executor.get_storage_at(token, key_u256).unwrap_or_default();
            
            // Dummy write magic
            let magic = U256::from(0xDEADBEEF_u64);
            let _ = executor.set_storage_at(token, key_u256, magic);
            
            // Check balanceOf
            let mut calldata = Vec::with_capacity(36);
            calldata.extend_from_slice(&[0x70, 0xa0, 0x82, 0x31]);
            let mut padded = [0u8; 32];
            padded[12..32].copy_from_slice(target.as_slice());
            calldata.extend_from_slice(&padded);
            
            let res = executor.call(Address::ZERO, Some(token), U256::ZERO, Bytes::from(calldata));
            
            // Restore immediately!
            let _ = executor.set_storage_at(token, key_u256, old_val);
            
            if let Ok(ret) = res {
                if ret.len() >= 32 {
                    let bal = U256::from_be_slice(&ret[..32]);
                    if bal == magic {
                        slot_found = Some(key_u256);
                        break;
                    }
                }
            }
        }

        if let Some(key) = slot_found {
            let _ = executor.set_storage_at(token, key, amount);
            drop(executor);
            self.record_cheatcode_tx(target, format!("eidolon_setErc20Balance({:?}, target={:?}, amount={})", token, target, amount));
            info!("🧙 eidolon_setErc20Balance({:?}, user={:?}) -> {}", token, target, amount);
            Ok(true)
        } else {
            Err(ErrorObject::owned(
                -32000,
                "Could not discover ERC20 balance storage slot.".to_string(),
                None::<()>,
            ))
        }
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
                    input: stored.input.clone(),
                    nonce: U64::from(stored.nonce),
                    transaction_index: U64::ZERO,
                    type_: U64::from(2),
                };
                Ok(Some(obj))
            }
            None => Ok(None),
        }
    }

    fn get_block_by_hash(&self, hash: B256, _full_tx: bool) -> RpcResult<Option<MockBlock>> {
        let idx = {
            let hashes = self.block_hashes.read();
            hashes.get(&hash).copied()
        };
        if let Some(idx) = idx {
            let blocks = self.blocks.read();
            if let Some(block) = blocks.get(idx) {
                return Ok(Some(Self::block_to_mock(block)));
            }
        }
        Ok(None)
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
                // Filter by address (supports single or multiple)
                if let Some(ref addr_filter) = filter.address {
                    if !addr_filter.matches(&log.address) {
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
            let reverted_block_number = executor.block_env.number;
            drop(executor);

            // Trim block history to match reverted state
            let mut blocks = self.blocks.write();
            blocks.retain(|b| b.number <= reverted_block_number);
            let mut hashes = self.block_hashes.write();
            hashes.clear();
            for (i, block) in blocks.iter().enumerate() {
                hashes.insert(block.hash, i);
            }

            info!("⏪ Reverted to snapshot: id={}", snapshot_id);
        } else {
            error!("❌ Failed to revert to snapshot: id={}", snapshot_id);
        }
        Ok(success)
    }

    // --- Cheatcode implementations ---

    fn evm_mine(&self) -> RpcResult<U64> {
        let (block_number, block_timestamp) = {
            let mut executor = self.executor.write();
            executor.mine_one_block();
            (executor.block_env.number, executor.block_env.timestamp)
        };

        self.record_block(block_number, block_timestamp, vec![], U256::ZERO);

        info!("⛏️ evm_mine: mined block {}", block_number);
        Ok(U64::from(block_number.to::<u64>()))
    }

    fn evm_set_next_block_timestamp(&self, timestamp: U64) -> RpcResult<U64> {
        let mut executor = self.executor.write();
        let ts = timestamp.to::<u64>();
        executor.set_next_block_timestamp(ts);
        info!("🕐 evm_setNextBlockTimestamp: {}", ts);
        Ok(timestamp)
    }

    fn evm_set_automine(&self, enabled: bool) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        executor.set_automine(enabled);
        info!("⚙️ evm_setAutomine: {}", enabled);
        Ok(true)
    }

    fn evm_set_block_gas_limit(&self, gas_limit: U64) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        let limit = gas_limit.to::<u64>();
        executor.set_block_gas_limit(limit);
        info!("⛽ evm_setBlockGasLimit: {}", limit);
        Ok(true)
    }

    fn eidolon_set_code(&self, address: Address, code: Bytes) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        executor.set_code(address, code);
        info!("🔧 eidolon_setCode({:?})", address);
        Ok(true)
    }

    fn eidolon_set_nonce(&self, address: Address, nonce: U64) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        let n = nonce.to::<u64>();
        executor.set_nonce(address, n);
        info!("🔧 eidolon_setNonce({:?}) -> {}", address, n);
        Ok(true)
    }

    fn eidolon_set_storage_at(&self, address: Address, slot: U256, value: U256) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        match executor.set_storage_at(address, slot, value) {
            Ok(()) => {
                info!("🔧 eidolon_setStorageAt({:?}, {}, {})", address, slot, value);
                Ok(true)
            }
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Internal Error: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn eidolon_impersonate_account(&self, address: Address) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        executor.impersonate_account(address);
        drop(executor);
        self.record_cheatcode_tx(address, format!("eidolon_impersonateAccount({:?})", address));
        info!("🎭 eidolon_impersonateAccount({:?})", address);
        Ok(true)
    }

    fn eidolon_stop_impersonating_account(&self, address: Address) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        executor.stop_impersonating_account(address);
        drop(executor);
        self.record_cheatcode_tx(address, format!("eidolon_stopImpersonatingAccount({:?})", address));
        info!("🎭 eidolon_stopImpersonatingAccount({:?})", address);
        Ok(true)
    }

    fn eidolon_reset(&self, params: Option<ResetParams>) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        let (rpc_url, block_number) = match params {
            Some(p) => match p.forking {
                Some(f) => (f.json_rpc_url, f.block_number),
                None => (None, None),
            },
            None => (None, None),
        };
        executor.reset_fork(rpc_url, block_number);

        let genesis_number = executor.block_env.number;
        let genesis_timestamp = executor.block_env.timestamp;
        drop(executor);

        // Clear stored transactions
        self.transactions.write().clear();

        // Reset block history with new genesis
        let genesis_hash = Self::compute_block_hash(genesis_number, genesis_timestamp, B256::ZERO);
        let genesis = VirtualBlock {
            number: genesis_number,
            hash: genesis_hash,
            parent_hash: B256::ZERO,
            timestamp: genesis_timestamp,
            transactions: vec![],
            gas_used: U256::ZERO,
            gas_limit: U256::from(30_000_000),
        };
        *self.blocks.write() = vec![genesis];
        let mut hashes = self.block_hashes.write();
        hashes.clear();
        hashes.insert(genesis_hash, 0);

        info!("🔄 eidolon_reset");
        Ok(true)
    }

    fn simulate_transaction(&self, request: CallRequest) -> RpcResult<SimulationResult> {
        let mut executor = self.executor.write();
        let caller = request.from.unwrap_or(Address::ZERO);
        let value = request.value.unwrap_or(U256::ZERO);
        let data = request.data.unwrap_or_default();

        info!(
            "🔬 eidolon_simulateTransaction: from={:?} to={:?}",
            caller, request.to
        );

        match executor.simulate_transaction(caller, request.to, value, data) {
            Ok(result) => Ok(result),
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Simulation Error: {:?}", e),
                None::<()>,
            )),
        }
    }

    fn simulate_bundle(&self, transactions: Vec<CallRequest>) -> RpcResult<BundleSimulationResult> {
        let mut executor = self.executor.write();

        info!(
            "📦 eidolon_simulateBundle: {} transactions",
            transactions.len()
        );

        let txs: Vec<_> = transactions
            .into_iter()
            .map(|req| {
                (
                    req.from.unwrap_or(Address::ZERO),
                    req.to,
                    req.value.unwrap_or(U256::ZERO),
                    req.data.unwrap_or_default(),
                )
            })
            .collect();

        match executor.simulate_bundle(txs) {
            Ok(result) => Ok(result),
            Err(e) => Err(ErrorObject::owned(
                -32000,
                format!("Bundle Simulation Error: {:?}", e),
                None::<()>,
            )),
        }
    }

    // --- Anvil-compatible aliases ---

    fn anvil_set_balance(&self, address: Address, balance: U256) -> RpcResult<bool> {
        self.set_balance(address, balance)
    }

    fn anvil_set_code(&self, address: Address, code: Bytes) -> RpcResult<bool> {
        self.eidolon_set_code(address, code)
    }

    fn anvil_set_nonce(&self, address: Address, nonce: U64) -> RpcResult<bool> {
        self.eidolon_set_nonce(address, nonce)
    }

    fn anvil_set_storage_at(&self, address: Address, slot: U256, value: U256) -> RpcResult<bool> {
        self.eidolon_set_storage_at(address, slot, value)
    }

    fn anvil_impersonate_account(&self, address: Address) -> RpcResult<bool> {
        self.eidolon_impersonate_account(address)
    }

    fn anvil_stop_impersonating_account(&self, address: Address) -> RpcResult<bool> {
        self.eidolon_stop_impersonating_account(address)
    }

    fn anvil_mine(&self, count: Option<U64>, interval: Option<U64>) -> RpcResult<bool> {
        let mut executor = self.executor.write();
        let c = count.map(|v| v.to::<u64>()).unwrap_or(1);
        let time_step = interval.map(|v| v.to::<u64>()).unwrap_or(12);

        let mut block_infos = Vec::with_capacity(c as usize);
        for _ in 0..c {
            executor.block_env.number += U256::from(1);
            executor.block_env.timestamp += U256::from(time_step);
            block_infos.push((executor.block_env.number, executor.block_env.timestamp));
        }
        drop(executor);

        for (number, timestamp) in block_infos {
            self.record_block(number, timestamp, vec![], U256::ZERO);
        }

        info!("⛏️ anvil_mine: {} blocks", c);
        Ok(true)
    }

    fn anvil_set_block_timestamp_interval(&self, seconds: U64) -> RpcResult<bool> {
        let secs = seconds.to::<u64>();
        *self.block_timestamp_interval.write() = Some(secs);
        info!("⏱️ anvil_setBlockTimestampInterval: {}s", secs);
        Ok(true)
    }

    fn anvil_auto_impersonate_account(&self, enabled: bool) -> RpcResult<bool> {
        *self.auto_impersonate.write() = enabled;
        info!("🎭 anvil_autoImpersonateAccount: {}", enabled);
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rpc() -> EidolonRpc {
        let executor = Executor::new(
            "http://localhost:8545".to_string(),
            1,
            Some(1),
        );
        // Pre-populate coinbase so EVM execution doesn't hit the network
        let mut exec = executor;
        exec.db.insert_account_info(Address::ZERO, revm::primitives::AccountInfo::default());
        EidolonRpc::new(Arc::new(RwLock::new(exec)), 1)
    }

    fn alice() -> Address {
        Address::repeat_byte(0xAA)
    }
    fn bob() -> Address {
        Address::repeat_byte(0xBB)
    }

    // --- Pure methods (no state needed) ---

    #[test]
    fn net_version_returns_chain_id() {
        let rpc = test_rpc();
        assert_eq!(rpc.net_version().unwrap(), "1");
    }

    #[test]
    fn chain_id_returns_correct_value() {
        let rpc = EidolonRpc::new(
            Arc::new(RwLock::new(Executor::new(
                "http://localhost:8545".to_string(),
                137,
                Some(1),
            ))),
            137,
        );
        assert_eq!(rpc.chain_id().unwrap(), U64::from(137));
    }

    #[test]
    fn gas_price_returns_one_wei() {
        let rpc = test_rpc();
        assert_eq!(rpc.gas_price().unwrap(), U256::from(1));
    }

    #[test]
    fn syncing_returns_false() {
        let rpc = test_rpc();
        assert!(!rpc.syncing().unwrap());
    }

    #[test]
    fn client_version_contains_eidolon() {
        let rpc = test_rpc();
        assert!(rpc.client_version().unwrap().contains("Eidolon"));
    }

    #[test]
    fn net_listening_returns_true() {
        let rpc = test_rpc();
        assert!(rpc.net_listening().unwrap());
    }

    #[test]
    fn accounts_returns_empty() {
        let rpc = test_rpc();
        assert!(rpc.accounts().unwrap().is_empty());
    }

    #[test]
    fn mining_returns_true() {
        let rpc = test_rpc();
        assert!(rpc.mining().unwrap());
    }

    #[test]
    fn max_priority_fee_returns_one_wei() {
        let rpc = test_rpc();
        assert_eq!(rpc.max_priority_fee_per_gas().unwrap(), U256::from(1));
    }

    // --- Block methods ---

    #[test]
    fn block_number_returns_pinned() {
        let rpc = test_rpc();
        let num = rpc.block_number().unwrap();
        assert_eq!(num, U256::from(1));
    }

    #[test]
    fn get_block_by_number_returns_some() {
        let rpc = test_rpc();
        let block = rpc.get_block_by_number("latest".to_string(), false).unwrap();
        assert!(block.is_some());
        let block = block.unwrap();
        assert_eq!(block.gas_limit, U256::from(30_000_000));
    }

    #[test]
    fn get_block_by_hash_returns_some() {
        let rpc = test_rpc();
        // Get the genesis block hash from block history
        let genesis_hash = rpc.blocks.read()[0].hash;
        let block = rpc.get_block_by_hash(genesis_hash, false).unwrap();
        assert!(block.is_some());
    }

    #[test]
    fn get_block_by_hash_unknown_returns_none() {
        let rpc = test_rpc();
        let block = rpc.get_block_by_hash(B256::ZERO, false).unwrap();
        assert!(block.is_none());
    }

    // --- Transaction receipt ---

    #[test]
    fn receipt_for_unknown_hash_returns_none() {
        let rpc = test_rpc();
        let receipt = rpc.get_transaction_receipt(B256::repeat_byte(0xFF)).unwrap();
        assert!(receipt.is_none());
    }

    #[test]
    fn transaction_by_hash_unknown_returns_none() {
        let rpc = test_rpc();
        let tx = rpc.get_transaction_by_hash(B256::repeat_byte(0xFF)).unwrap();
        assert!(tx.is_none());
    }

    // --- State methods (pre-populate executor) ---

    #[test]
    fn get_balance_from_rpc() {
        let rpc = test_rpc();
        // Pre-populate
        rpc.executor.write().set_balance(alice(), U256::from(5000));

        let bal = rpc.get_balance(alice(), None).unwrap();
        assert_eq!(bal, U256::from(5000));
    }

    #[test]
    fn get_transaction_count_from_rpc() {
        let rpc = test_rpc();
        rpc.executor.write().db.insert_account_info(
            alice(),
            revm::primitives::AccountInfo {
                nonce: 7,
                ..Default::default()
            },
        );

        let count = rpc.get_transaction_count(alice(), None).unwrap();
        assert_eq!(count, U256::from(7));
    }

    #[test]
    fn get_storage_at_from_rpc() {
        let rpc = test_rpc();
        {
            let mut exec = rpc.executor.write();
            exec.db.insert_account_info(alice(), revm::primitives::AccountInfo::default());
            exec.set_storage_at(alice(), U256::from(0), U256::from(42)).unwrap();
        }

        let val = rpc.get_storage_at(alice(), U256::from(0), None).unwrap();
        assert_eq!(val, U256::from(42));
    }

    // --- Send transaction + receipt round trip ---

    #[test]
    fn send_transaction_and_get_receipt() {
        let rpc = test_rpc();
        {
            let mut exec = rpc.executor.write();
            exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));
            exec.db.insert_account_info(bob(), revm::primitives::AccountInfo::default());
        }

        let request = CallRequest {
            from: Some(alice()),
            to: Some(bob()),
            value: Some(U256::from(1000)),
            data: None,
        };

        let tx_hash = rpc.send_transaction(request).unwrap();

        // Receipt should exist
        let receipt = rpc.get_transaction_receipt(tx_hash).unwrap();
        assert!(receipt.is_some());
        let receipt = receipt.unwrap();
        assert_eq!(receipt.from, alice());
        assert_eq!(receipt.to, Some(bob()));
        assert_eq!(receipt.status, U64::from(1)); // success

        // Transaction by hash should also exist
        let tx_obj = rpc.get_transaction_by_hash(tx_hash).unwrap();
        assert!(tx_obj.is_some());
        let tx_obj = tx_obj.unwrap();
        assert_eq!(tx_obj.from, alice());
        assert_eq!(tx_obj.to, Some(bob()));
    }

    #[test]
    fn send_transaction_auto_mines_block() {
        let rpc = test_rpc();
        {
            let mut exec = rpc.executor.write();
            exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));
            exec.db.insert_account_info(bob(), revm::primitives::AccountInfo::default());
        }

        let block_before = rpc.block_number().unwrap();

        let request = CallRequest {
            from: Some(alice()),
            to: Some(bob()),
            value: Some(U256::from(1000)),
            data: None,
        };
        rpc.send_transaction(request).unwrap();

        let block_after = rpc.block_number().unwrap();
        assert_eq!(block_after, block_before + U256::from(1));
    }

    // --- Snapshot/revert via RPC ---

    #[test]
    fn evm_snapshot_and_revert() {
        let rpc = test_rpc();
        rpc.executor.write().set_balance(alice(), U256::from(1000));

        let snap_id = rpc.evm_snapshot().unwrap();

        rpc.executor.write().set_balance(alice(), U256::from(9999));
        assert_eq!(rpc.get_balance(alice(), None).unwrap(), U256::from(9999));

        let success = rpc.evm_revert(snap_id).unwrap();
        assert!(success);

        assert_eq!(rpc.get_balance(alice(), None).unwrap(), U256::from(1000));
    }

    #[test]
    fn evm_revert_invalid_id() {
        let rpc = test_rpc();
        let success = rpc.evm_revert(U64::from(999)).unwrap();
        assert!(!success);
    }

    // --- Fee history ---

    #[test]
    fn fee_history_returns_correct_length() {
        let rpc = test_rpc();
        rpc.executor.write().set_block_number(100);

        let history = rpc.fee_history(U64::from(5), "latest".to_string(), None).unwrap();
        assert_eq!(history.base_fee_per_gas.len(), 6); // count + 1
        assert_eq!(history.gas_used_ratio.len(), 5);
    }

    // --- Increase time ---

    #[test]
    fn increase_time_via_rpc() {
        let rpc = test_rpc();
        let ts_before = rpc.executor.read().block_env.timestamp;

        rpc.increase_time(U64::from(3600)).unwrap();

        let ts_after = rpc.executor.read().block_env.timestamp;
        assert_eq!(ts_after, ts_before + U256::from(3600));
    }

    // --- set_balance via RPC ---

    #[test]
    fn eidolon_set_balance() {
        let rpc = test_rpc();
        let result = rpc.set_balance(alice(), U256::from(999_999)).unwrap();
        assert!(result);

        let bal = rpc.get_balance(alice(), None).unwrap();
        assert_eq!(bal, U256::from(999_999));
    }

    // --- Get logs (empty case) ---

    #[test]
    fn get_logs_empty_when_no_transactions() {
        let rpc = test_rpc();
        let filter = LogFilter {
            from_block: None,
            to_block: None,
            address: None,
            topics: None,
        };
        let logs = rpc.get_logs(filter).unwrap();
        assert!(logs.is_empty());
    }

    // --- Cheatcode: evm_mine ---

    #[test]
    fn evm_mine_advances_block() {
        let rpc = test_rpc();
        let block_before = rpc.block_number().unwrap();

        rpc.evm_mine().unwrap();

        let block_after = rpc.block_number().unwrap();
        assert_eq!(block_after, block_before + U256::from(1));
    }

    // --- Cheatcode: evm_setNextBlockTimestamp ---

    #[test]
    fn evm_set_next_block_timestamp_and_mine() {
        let rpc = test_rpc();

        rpc.evm_set_next_block_timestamp(U64::from(1_700_000_000u64)).unwrap();
        rpc.evm_mine().unwrap();

        let executor = rpc.executor.read();
        assert_eq!(executor.block_env.timestamp, U256::from(1_700_000_000u64));
    }

    // --- Cheatcode: evm_setAutomine ---

    #[test]
    fn evm_set_automine() {
        let rpc = test_rpc();
        rpc.evm_set_automine(false).unwrap();
        assert!(!rpc.executor.read().automine);

        rpc.evm_set_automine(true).unwrap();
        assert!(rpc.executor.read().automine);
    }

    // --- Cheatcode: evm_setBlockGasLimit ---

    #[test]
    fn evm_set_block_gas_limit() {
        let rpc = test_rpc();
        rpc.evm_set_block_gas_limit(U64::from(50_000_000u64)).unwrap();
        assert_eq!(rpc.executor.read().block_gas_limit, 50_000_000);
    }

    // --- Cheatcode: eidolon_setBalance mines synthetic ---

    #[test]
    fn eidolon_set_balance_mines_synthetic() {
        let rpc = test_rpc();
        let block_before = rpc.block_number().unwrap();

        rpc.set_balance(alice(), U256::from(12345)).unwrap();

        let block_after = rpc.block_number().unwrap();
        assert_eq!(block_after, block_before + U256::from(1)); // block mined for cheatcode
        let bal = rpc.get_balance(alice(), None).unwrap();
        assert_eq!(bal, U256::from(12345));
    }

    // --- Cheatcode: eidolon_setCode ---

    #[test]
    fn eidolon_set_code() {
        let rpc = test_rpc();
        let bytecode = Bytes::from_static(&[0x60, 0x00, 0x60, 0x00, 0xFD]);
        rpc.eidolon_set_code(alice(), bytecode.clone()).unwrap();

        let code = rpc.get_code(alice(), None).unwrap();
        assert_eq!(code, bytecode);
    }

    // --- Cheatcode: eidolon_setNonce ---

    #[test]
    fn eidolon_set_nonce() {
        let rpc = test_rpc();
        rpc.eidolon_set_nonce(alice(), U64::from(77)).unwrap();

        let count = rpc.get_transaction_count(alice(), None).unwrap();
        assert_eq!(count, U256::from(77));
    }

    // --- Cheatcode: eidolon_setStorageAt ---

    #[test]
    fn eidolon_set_storage_at() {
        let rpc = test_rpc();
        // Pre-populate account so storage reads don't hit the network
        rpc.executor.write().db.insert_account_info(alice(), revm::primitives::AccountInfo::default());
        rpc.eidolon_set_storage_at(alice(), U256::from(5), U256::from(999)).unwrap();

        let val = rpc.get_storage_at(alice(), U256::from(5), None).unwrap();
        assert_eq!(val, U256::from(999));
    }

    // --- Cheatcode: impersonation ---

    #[test]
    fn eidolon_impersonate_and_stop() {
        let rpc = test_rpc();
        rpc.eidolon_impersonate_account(alice()).unwrap();
        assert!(rpc.executor.read().is_impersonated(&alice()));

        rpc.eidolon_stop_impersonating_account(alice()).unwrap();
        assert!(!rpc.executor.read().is_impersonated(&alice()));
    }

    // --- Cheatcode: anvil_mine ---

    #[test]
    fn anvil_mine_multiple_blocks() {
        let rpc = test_rpc();
        let block_before = rpc.block_number().unwrap();

        rpc.anvil_mine(Some(U64::from(10)), None).unwrap();

        let block_after = rpc.block_number().unwrap();
        assert_eq!(block_after, block_before + U256::from(10));
    }

    // --- Cheatcode: anvil aliases delegate correctly ---

    #[test]
    fn anvil_set_balance_alias() {
        let rpc = test_rpc();
        rpc.anvil_set_balance(alice(), U256::from(7777)).unwrap();

        let bal = rpc.get_balance(alice(), None).unwrap();
        assert_eq!(bal, U256::from(7777));
    }

    // --- Cheatcode: eidolon_reset ---

    #[test]
    fn eidolon_reset_clears_transactions() {
        let rpc = test_rpc();
        // Add a fake transaction
        rpc.transactions.write().insert(
            B256::repeat_byte(0x01),
            StoredTransaction {
                from: alice(),
                to: Some(bob()),
                value: U256::from(100),
                input: Bytes::default(),
                nonce: 0,
                block_number: U256::from(1),
                block_hash: B256::ZERO,
                gas_used: 21000,
                contract_address: None,
                logs: vec![],
                status: true,
            },
        );
        assert_eq!(rpc.transactions.read().len(), 1);

        rpc.eidolon_reset(None).unwrap();

        assert!(rpc.transactions.read().is_empty());
        // Block history should be reset to genesis
        assert_eq!(rpc.blocks.read().len(), 1);
    }

    // --- New Phase 1+2 tests ---

    #[test]
    fn send_transaction_produces_unique_hashes() {
        let rpc = test_rpc();
        {
            let mut exec = rpc.executor.write();
            exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));
            exec.db.insert_account_info(bob(), revm::primitives::AccountInfo::default());
        }

        let req1 = CallRequest { from: Some(alice()), to: Some(bob()), value: Some(U256::from(100)), data: None };
        let req2 = CallRequest { from: Some(alice()), to: Some(bob()), value: Some(U256::from(100)), data: None };

        let hash1 = rpc.send_transaction(req1).unwrap();
        let hash2 = rpc.send_transaction(req2).unwrap();

        // Different nonces => different hashes
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn block_history_tracks_mined_blocks() {
        let rpc = test_rpc();
        // Genesis block exists
        assert_eq!(rpc.blocks.read().len(), 1);

        rpc.evm_mine().unwrap();
        assert_eq!(rpc.blocks.read().len(), 2);

        rpc.evm_mine().unwrap();
        assert_eq!(rpc.blocks.read().len(), 3);

        // Each block has a unique hash
        let blocks = rpc.blocks.read();
        assert_ne!(blocks[0].hash, blocks[1].hash);
        assert_ne!(blocks[1].hash, blocks[2].hash);

        // Parent hash chain is correct
        assert_eq!(blocks[1].parent_hash, blocks[0].hash);
        assert_eq!(blocks[2].parent_hash, blocks[1].hash);
    }

    #[test]
    fn get_block_by_number_returns_correct_block() {
        let rpc = test_rpc();
        rpc.evm_mine().unwrap(); // block 2
        rpc.evm_mine().unwrap(); // block 3

        let latest = rpc.get_block_by_number("latest".to_string(), false).unwrap().unwrap();
        assert_eq!(latest.number, U256::from(3));

        let earliest = rpc.get_block_by_number("earliest".to_string(), false).unwrap().unwrap();
        assert_eq!(earliest.number, U256::from(1)); // genesis was pinned at block 1

        // Query specific block by hex number
        let block2 = rpc.get_block_by_number("0x2".to_string(), false).unwrap();
        assert!(block2.is_some());
        assert_eq!(block2.unwrap().number, U256::from(2));
    }

    #[test]
    fn block_hashes_are_deterministic() {
        let rpc = test_rpc();
        rpc.evm_mine().unwrap();

        let blocks = rpc.blocks.read();
        let block = &blocks[1];

        // Recompute hash and verify it matches
        let expected = EidolonRpc::compute_block_hash(block.number, block.timestamp, block.parent_hash);
        assert_eq!(block.hash, expected);
    }

    #[test]
    fn send_transaction_records_block_with_tx() {
        let rpc = test_rpc();
        {
            let mut exec = rpc.executor.write();
            exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));
            exec.db.insert_account_info(bob(), revm::primitives::AccountInfo::default());
        }

        let blocks_before = rpc.blocks.read().len();
        let request = CallRequest { from: Some(alice()), to: Some(bob()), value: Some(U256::from(1000)), data: None };
        let tx_hash = rpc.send_transaction(request).unwrap();

        let blocks = rpc.blocks.read();
        assert_eq!(blocks.len(), blocks_before + 1);
        let last_block = blocks.last().unwrap();
        assert!(last_block.transactions.contains(&tx_hash));
    }

    #[test]
    fn get_transaction_by_hash_returns_input_data() {
        let rpc = test_rpc();
        {
            let mut exec = rpc.executor.write();
            exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));
            exec.db.insert_account_info(bob(), revm::primitives::AccountInfo::default());
        }

        let data = Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]);
        let request = CallRequest { from: Some(alice()), to: Some(bob()), value: Some(U256::from(0)), data: Some(data.clone()) };
        let tx_hash = rpc.send_transaction(request).unwrap();

        let tx_obj = rpc.get_transaction_by_hash(tx_hash).unwrap().unwrap();
        assert_eq!(tx_obj.input, data);
        assert_eq!(tx_obj.nonce, U64::from(0)); // first tx, nonce = 0
    }

    #[test]
    fn evm_revert_trims_block_history() {
        let rpc = test_rpc();
        let snap_id = rpc.evm_snapshot().unwrap();

        rpc.evm_mine().unwrap();
        rpc.evm_mine().unwrap();
        assert_eq!(rpc.blocks.read().len(), 3); // genesis + 2

        rpc.evm_revert(snap_id).unwrap();
        // Should trim back to genesis only
        assert_eq!(rpc.blocks.read().len(), 1);
    }
}
