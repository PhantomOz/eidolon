pub mod tracer;

use alloy_primitives::{Address, Bytes, U256};
use anyhow::Result;
use eidolon_forkdb::{ForkDB, fetch_latest_block_number, new_fork_db};
use revm::primitives::AccountInfo;
use revm::{
    Database, Evm,
    db::{AccountState, DbAccount},
    primitives::{BlockEnv, CfgEnv, ExecutionResult, Output, TransactTo},
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use tracer::EidolonTracer;
use tracing::{info, warn};

// --- Simulation Data Structures ---

/// Difference in a single storage slot.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StorageDiff {
    pub slot: U256,
    pub before: U256,
    pub after: U256,
}

/// State changes for a single account.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AccountDiff {
    pub address: Address,
    pub balance_before: U256,
    pub balance_after: U256,
    pub nonce_before: u64,
    pub nonce_after: u64,
    pub storage_diffs: Vec<StorageDiff>,
}

/// Result of a transaction simulation (no state committed).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SimulationResult {
    pub success: bool,
    pub gas_used: u64,
    pub return_data: Bytes,
    pub logs: Vec<revm::primitives::Log>,
    pub state_diffs: Vec<AccountDiff>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decoded_call: Option<DecodedCall>,
}

/// Result of simulating a bundle of transactions.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BundleSimulationResult {
    pub results: Vec<SimulationResult>,
    pub total_gas_used: u64,
    pub bundle_success: bool,
}

/// A decoded function call (4-byte selector → human-readable).
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DecodedCall {
    pub selector: String,
    pub function_name: String,
}

/// Decode a 4-byte function selector into a human-readable name.
pub fn decode_selector(data: &[u8]) -> Option<DecodedCall> {
    if data.len() < 4 {
        return None;
    }
    let selector = format!("0x{:02x}{:02x}{:02x}{:02x}", data[0], data[1], data[2], data[3]);
    let name = match selector.as_str() {
        // ERC20
        "0xa9059cbb" => "transfer(address,uint256)",
        "0x095ea7b3" => "approve(address,uint256)",
        "0x23b872dd" => "transferFrom(address,address,uint256)",
        "0x70a08231" => "balanceOf(address)",
        "0xdd62ed3e" => "allowance(address,address)",
        "0x18160ddd" => "totalSupply()",
        // ERC721
        "0x42842e0e" => "safeTransferFrom(address,address,uint256)",
        "0x6352211e" => "ownerOf(uint256)",
        "0xe985e9c5" => "isApprovedForAll(address,address)",
        "0xa22cb465" => "setApprovalForAll(address,bool)",
        // Uniswap V2
        "0x38ed1739" => "swapExactTokensForTokens(uint256,uint256,address[],address,uint256)",
        "0x7ff36ab5" => "swapExactETHForTokens(uint256,address[],address,uint256)",
        "0x18cbafe5" => "swapExactTokensForETH(uint256,uint256,address[],address,uint256)",
        "0xe8e33700" => "addLiquidity(address,address,uint256,uint256,uint256,uint256,address,uint256)",
        // Uniswap V3
        "0x414bf389" => "exactInputSingle((address,address,uint24,address,uint256,uint256,uint256,uint160))",
        "0xc04b8d59" => "exactInput((bytes,address,uint256,uint256,uint256))",
        // Common
        "0x3593564c" => "execute(bytes,bytes[],uint256)",
        "0xb6f9de95" => "swapExactETHForTokensSupportingFeeOnTransferTokens(uint256,address[],address,uint256)",
        "0xd0e30db0" => "deposit()",
        "0x2e1a7d4d" => "withdraw(uint256)",
        "0x150b7a02" => "onERC721Received(address,address,uint256,bytes)",
        "0xf23a6e61" => "onERC1155Received(address,address,uint256,uint256,bytes)",
        _ => return Some(DecodedCall { selector, function_name: "unknown".to_string() }),
    };
    Some(DecodedCall {
        selector,
        function_name: name.to_string(),
    })
}

// --- Persistence Data Structures ---

/// A simple struct to save to Redis.
/// Holds everything needed to reconstruct an account.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SerializableAccount {
    pub info: AccountInfo,
    pub storage: HashMap<U256, U256>,
}

/// The full snapshot of the fork.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct StateSnapshot {
    pub accounts: HashMap<Address, SerializableAccount>,
    pub timestamp: U256,
    pub block_number: U256,
    pub chain_id: u64,
}

/// The Eidolon Executor
/// Wraps the EVM and the Database (State).
pub struct Executor {
    pub db: ForkDB,
    pub block_env: BlockEnv,
    pub cfg_env: CfgEnv,
    // Snapshots
    pub snapshots: HashMap<u64, StateSnapshot>,
    pub snapshot_id_counter: u64,
    // Cheatcode state
    pub automine: bool,
    pub impersonated_accounts: HashSet<Address>,
    pub next_block_timestamp: Option<u64>,
    pub block_gas_limit: u64,
}

impl Executor {
    pub fn new(rpc_url: String, chain_id: u64, block_number: Option<u64>) -> Self {
        let db = new_fork_db(rpc_url.clone(), block_number);

        let mut block_env = BlockEnv::default();

        // 1. Set Timestamp (System Time)
        block_env.timestamp = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        // 2. Set Block Number (Dynamic!)
        let target_block = if let Some(b) = block_number {
            b // User pinned a block
        } else {
            // User wants latest, so let's fetch it
            match fetch_latest_block_number(&rpc_url) {
                Ok(n) => {
                    info!("🔗 Synced to latest block: {}", n);
                    n
                }
                Err(e) => {
                    warn!("⚠️ Failed to fetch block number: {:?}. Defaulting to 0.", e);
                    0
                }
            }
        };
        block_env.number = U256::from(target_block);

        let mut cfg_env = CfgEnv::default();
        cfg_env.chain_id = chain_id;

        Self {
            db,
            block_env,
            cfg_env,
            snapshots: HashMap::new(),
            snapshot_id_counter: 0,
            automine: true,
            impersonated_accounts: HashSet::new(),
            next_block_timestamp: None,
            block_gas_limit: 30_000_000,
        }
    }

    pub fn increase_time(&mut self, seconds: u64) {
        self.block_env.timestamp += U256::from(seconds);
    }

    pub fn set_block_timestamp(&mut self, timestamp: u64) {
        self.block_env.timestamp = U256::from(timestamp);
    }

    pub fn set_block_number(&mut self, number: u64) {
        self.block_env.number = U256::from(number);
    }

    /// Mine a number of blocks, advancing block number and timestamp.
    pub fn mine_blocks(&mut self, count: u64, interval: Option<u64>) {
        let time_step = interval.unwrap_or(12); // default 12s per block
        for _ in 0..count {
            self.block_env.number += U256::from(1);
            self.block_env.timestamp += U256::from(time_step);
        }
    }

    /// Set the timestamp for the next block only. Consumed after the next mine.
    pub fn set_next_block_timestamp(&mut self, timestamp: u64) {
        self.next_block_timestamp = Some(timestamp);
    }

    /// Toggle automine mode. When true, each transaction auto-mines a block.
    pub fn set_automine(&mut self, enabled: bool) {
        self.automine = enabled;
    }

    /// Set the block gas limit.
    pub fn set_block_gas_limit(&mut self, limit: u64) {
        self.block_gas_limit = limit;
    }

    /// Set the nonce of an account.
    pub fn set_nonce(&mut self, address: Address, nonce: u64) {
        let mut account = AccountInfo::default();
        if let Ok(Some(existing)) = self.db.basic(address) {
            account = existing;
        }
        account.nonce = nonce;
        self.db.insert_account_info(address, account);
    }

    /// Impersonate an account (allow sending transactions from it without signature).
    pub fn impersonate_account(&mut self, address: Address) {
        self.impersonated_accounts.insert(address);
    }

    /// Stop impersonating an account.
    pub fn stop_impersonating_account(&mut self, address: Address) {
        self.impersonated_accounts.remove(&address);
    }

    /// Check if an account is being impersonated.
    pub fn is_impersonated(&self, address: &Address) -> bool {
        self.impersonated_accounts.contains(address)
    }

    /// Reset the fork to a new block number (or latest).
    pub fn reset_fork(&mut self, rpc_url: Option<String>, block_number: Option<u64>) {
        let url = rpc_url.unwrap_or_else(|| {
            // Use the existing DB's backend URL
            self.db.db.config.rpc_url.clone()
        });
        self.db = new_fork_db(url, block_number);
        if let Some(bn) = block_number {
            self.block_env.number = U256::from(bn);
        }
    }

    /// Advance the block by one, applying next_block_timestamp if set.
    pub fn mine_one_block(&mut self) {
        self.block_env.number += U256::from(1);
        if let Some(ts) = self.next_block_timestamp.take() {
            self.block_env.timestamp = U256::from(ts);
        } else {
            self.block_env.timestamp += U256::from(12);
        }
    }

    pub fn set_balance(&mut self, address: Address, amount: U256) {
        let mut account = AccountInfo::default();
        // Preserve existing code/nonce if possible, but for setBalance often we just want to set balance.
        // If we want to be safe, we should check if account exists first.
        if let Ok(Some(existing)) = self.db.basic(address) {
             account = existing;
        }
        account.balance = amount;
        self.db.insert_account_info(address, account);
    }

    pub fn set_code(&mut self, address: Address, code: Bytes) {
        let mut account = AccountInfo::default();
        if let Ok(Some(existing)) = self.db.basic(address) {
             account = existing;
        }
        let bytecode = revm::primitives::Bytecode::new_raw(code);
        account.code_hash = bytecode.hash_slow();
        account.code = Some(bytecode);
        self.db.insert_account_info(address, account);
    }

    pub fn set_storage_at(&mut self, address: Address, slot: U256, value: U256) -> Result<()> {
        // Ensure account exists in the cache, or create a default one
        if self.db.basic(address)?.is_none() {
            self.db.insert_account_info(address, AccountInfo::default());
        }

        // We need to insert storage. ForkDB (CacheDB) handles this.
        self.db.insert_account_storage(address, slot, value)?;
        Ok(())
    }

    pub fn take_snapshot(&mut self) -> u64 {
        let id = self.snapshot_id_counter;
        self.snapshot_id_counter += 1;
        let snapshot = self.get_snapshot();
        self.snapshots.insert(id, snapshot);
        id
    }

    pub fn revert_snapshot(&mut self, id: u64) -> bool {
        if let Some(snapshot) = self.snapshots.get(&id) {
            // Deep clone needed because we are restoring it but want to keep the snapshot valid?
            // Usually revert keeps the snapshot or deletes it?
            // Anvil keeps it. Hardhat keeps it.
            // Let's clone it.
            self.load_snapshot(snapshot.clone());
            // We usually invalidate all snapshots taken AFTER this one.
            self.snapshots.retain(|k, _| *k <= id);
            true
        } else {
            false
        }
    }

    pub fn get_nonce(&mut self, address: Address) -> Result<u64> {
        // This triggers the ForkDB fetch if not in memory
        let acc = self.db.basic(address)?.unwrap_or_default();
        Ok(acc.nonce)
    }

    // NEW: Helper to get Code
    pub fn get_code(&mut self, address: Address) -> Result<Bytes> {
        let acc = self.db.basic(address)?.unwrap_or_default();
        // Extract bytecode if it exists, otherwise empty
        match acc.code {
            Some(code) => Ok(code.original_bytes()),
            None => Ok(Bytes::default()),
        }
    }

    /// Execute a transaction
    pub fn transact(
        &mut self,
        caller: Address,
        to: Option<Address>,
        value: U256,
        data: Bytes,
    ) -> Result<ExecutionResult> {
        let transact_to = match to {
            Some(addr) => TransactTo::Call(addr),
            None => TransactTo::Create,
        };

        // Look up the caller's current nonce so REVM's nonce check passes
        let nonce = self.get_nonce(caller)?;

        let gas_limit = self.block_gas_limit;
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = transact_to;
                tx.value = value;
                tx.data = data;
                tx.gas_limit = gas_limit;
                tx.gas_price = U256::from(1);
                tx.nonce = Some(nonce);
            })
            .build();

        let result = evm
            .transact_commit()
            .map_err(|e| anyhow::anyhow!("EVM Execution Error: {:?}", e))?;

        Ok(result)
    }

    /// Estimate the gas used by a transaction
    pub fn estimate_gas(
        &mut self,
        caller: Address,
        to: Option<Address>,
        value: U256,
        data: Bytes,
    ) -> Result<u64> {
        let transact_to = match to {
            Some(addr) => TransactTo::Call(addr),
            None => TransactTo::Create,
        };

        // We use transact_ref() or similar to not persist changes
        let gas_limit = self.block_gas_limit;
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = transact_to;
                tx.value = value;
                tx.data = data;
                tx.gas_limit = gas_limit; // Large limit for estimation
            })
            .build();

        let result_and_state = evm
            .transact()
            .map_err(|e| anyhow::anyhow!("EVM Execution Error: {:?}", e))?;

        match result_and_state.result {
            ExecutionResult::Success { gas_used, .. } => Ok(gas_used),
            ExecutionResult::Revert { output, .. } => {
                anyhow::bail!("Reverted during estimation: {:?}", output);
            }
            ExecutionResult::Halt { reason, .. } => {
                anyhow::bail!("Halted during estimation: {:?}", reason);
            }
        }
    }

    pub fn call(
        &mut self,
        caller: Address,
        to: Option<Address>,
        value: U256,
        data: Bytes,
    ) -> Result<Bytes> {
        let transact_to = match to {
            Some(addr) => TransactTo::Call(addr),
            None => TransactTo::Create,
        };

        // We use transact_ref() so we don't consume the DB
        let gas_limit = self.block_gas_limit;
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = transact_to;
                tx.value = value;
                tx.data = data;
                tx.gas_limit = gas_limit; // Infinite gas for reading
            })
            .build();
        let result_and_state = evm
            .transact()
            .map_err(|e| anyhow::anyhow!("EVM Execution Error: {:?}", e))?;

        // Extract the raw bytes (e.g., the return value of a function)
        match result_and_state.result {
            ExecutionResult::Success { output, .. } => match output {
                Output::Call(bytes) => Ok(bytes),
                Output::Create(bytes, _) => Ok(bytes),
            },
            ExecutionResult::Revert { output, .. } => {
                anyhow::bail!("Reverted: {:?}", output);
            }
            ExecutionResult::Halt { reason, .. } => {
                anyhow::bail!("Halted: {:?}", reason);
            }
        }
    }

    pub fn trace_transaction(
        &mut self,
        caller: Address,
        to: Option<Address>,
        value: U256,
        data: Bytes,
    ) -> Result<EidolonTracer> {
        let transact_to = match to {
            Some(addr) => TransactTo::Call(addr),
            None => TransactTo::Create,
        };

        // 1. Initialize Tracer
        let mut tracer = EidolonTracer::default();

        // 2. Build EVM with Inspector attached
        {
            let gas_limit = self.block_gas_limit;
            let mut evm = Evm::builder()
                .with_db(&mut self.db)
                .with_external_context(&mut tracer) // Pass tracer here
                .modify_tx_env(|tx| {
                    tx.caller = caller;
                    tx.transact_to = transact_to;
                    tx.value = value;
                    tx.data = data;
                    tx.gas_limit = gas_limit;
                })
                // This tells REVM to use the external context as an Inspector
                .append_handler_register(inspector_handle_register)
                .build();

            // 3. Execute (read-only)
            let _ = evm
                .transact()
                .map_err(|e| anyhow::anyhow!("Trace Error: {:?}", e))?;
        }

        // 4. Return the populated tracer
        Ok(tracer)
    }

    /// Simulate a transaction without committing state.
    /// Returns execution result + state diffs showing what would change.
    pub fn simulate_transaction(
        &mut self,
        caller: Address,
        to: Option<Address>,
        value: U256,
        data: Bytes,
    ) -> Result<SimulationResult> {
        let data_for_decode = data.clone();
        let transact_to = match to {
            Some(addr) => TransactTo::Call(addr),
            None => TransactTo::Create,
        };

        let gas_limit = self.block_gas_limit;

        // Execute via transact() (non-committing) inside a block so evm is dropped
        let result_and_state = {
            let mut evm = Evm::builder()
                .with_db(&mut self.db)
                .modify_tx_env(|tx| {
                    tx.caller = caller;
                    tx.transact_to = transact_to;
                    tx.value = value;
                    tx.data = data;
                    tx.gas_limit = gas_limit;
                    tx.gas_price = U256::from(1);
                })
                .build();

            evm.transact()
                .map_err(|e| anyhow::anyhow!("Simulation Error: {:?}", e))?
        };
        // evm is dropped here, self.db is free to use again

        // Extract state diffs — self.db still has old values since transact() doesn't commit
        let mut state_diffs = Vec::new();
        for (address, account) in &result_and_state.state {
            let mut storage_diffs = Vec::new();

            for (slot, storage_slot) in &account.storage {
                let before = storage_slot.original_value();
                let after = storage_slot.present_value();
                if before != after {
                    storage_diffs.push(StorageDiff {
                        slot: *slot,
                        before,
                        after,
                    });
                }
            }

            // Read before-values from our unchanged DB
            let (balance_before, nonce_before) = match self.db.basic(*address) {
                Ok(Some(info)) => (info.balance, info.nonce),
                _ => (U256::ZERO, 0),
            };

            let balance_after = account.info.balance;
            let nonce_after = account.info.nonce;

            // Only include accounts that actually changed
            if balance_before != balance_after || nonce_before != nonce_after || !storage_diffs.is_empty() {
                state_diffs.push(AccountDiff {
                    address: *address,
                    balance_before,
                    balance_after,
                    nonce_before,
                    nonce_after,
                    storage_diffs,
                });
            }
        }

        // Extract execution result
        let (success, gas_used, return_data, logs) = match &result_and_state.result {
            ExecutionResult::Success { gas_used, output, logs, .. } => {
                let data = match output {
                    Output::Call(bytes) => bytes.clone(),
                    Output::Create(bytes, _) => bytes.clone(),
                };
                (true, *gas_used, data, logs.clone())
            }
            ExecutionResult::Revert { gas_used, output, .. } => {
                (false, *gas_used, output.clone(), vec![])
            }
            ExecutionResult::Halt { gas_used, .. } => {
                (false, *gas_used, Bytes::default(), vec![])
            }
        };

        Ok(SimulationResult {
            success,
            gas_used,
            return_data: return_data.clone(),
            logs,
            state_diffs,
            decoded_call: decode_selector(&data_for_decode),
        })
    }

    /// Simulate a bundle of transactions sequentially.
    /// Uses snapshot/revert so no state is permanently changed.
    pub fn simulate_bundle(
        &mut self,
        transactions: Vec<(Address, Option<Address>, U256, Bytes)>,
    ) -> Result<BundleSimulationResult> {
        // Take a snapshot before the bundle
        let snap_id = self.take_snapshot();

        let mut results = Vec::with_capacity(transactions.len());
        let mut total_gas = 0u64;
        let mut all_success = true;

        for (caller, to, value, data) in transactions {
            // Execute each tx (commits state so next tx sees changes)
            let result = self.transact(caller, to, value, data.clone());

            match result {
                Ok(exec_result) => {
                    let (success, gas_used, return_data, logs) = match &exec_result {
                        ExecutionResult::Success { gas_used, output, logs, .. } => {
                            let d = match output {
                                Output::Call(bytes) => bytes.clone(),
                                Output::Create(bytes, _) => bytes.clone(),
                            };
                            (true, *gas_used, d, logs.clone())
                        }
                        ExecutionResult::Revert { gas_used, output, .. } => {
                            (false, *gas_used, output.clone(), vec![])
                        }
                        ExecutionResult::Halt { gas_used, .. } => {
                            (false, *gas_used, Bytes::default(), vec![])
                        }
                    };

                    total_gas += gas_used;
                    if !success {
                        all_success = false;
                    }

                    results.push(SimulationResult {
                        success,
                        gas_used,
                        return_data,
                        logs,
                        state_diffs: vec![], // Individual diffs not tracked in bundle mode
                        decoded_call: decode_selector(&data),
                    });
                }
                Err(_e) => {
                    all_success = false;
                    results.push(SimulationResult {
                        success: false,
                        gas_used: 0,
                        return_data: Bytes::default(),
                        logs: vec![],
                        state_diffs: vec![],
                        decoded_call: decode_selector(&data),
                    });
                }
            }
        }

        // Revert to snapshot — bundle didn't permanently change state
        self.revert_snapshot(snap_id);

        Ok(BundleSimulationResult {
            results,
            total_gas_used: total_gas,
            bundle_success: all_success,
        })
    }

    /// SAVE: Extract internal REVM state into our clean struct
    pub fn get_snapshot(&mut self) -> StateSnapshot {
        let mut serializable_accounts = HashMap::new();

        // Iterate over the CacheDB's internal accounts
        // 'db.accounts' maps Address -> DbAccount
        for (addr, db_acc) in &self.db.accounts {
            serializable_accounts.insert(
                *addr,
                SerializableAccount {
                    info: db_acc.info.clone(),
                    storage: db_acc.storage.clone().into_iter().collect(),
                },
            );
        }

        StateSnapshot {
            accounts: serializable_accounts,
            timestamp: self.block_env.timestamp,
            block_number: self.block_env.number,
            chain_id: self.cfg_env.chain_id,
        }
    }

    /// LOAD: Reconstruct REVM state from our struct
    pub fn load_snapshot(&mut self, snapshot: StateSnapshot) {
        // 1. Restore Block Environment
        self.block_env.timestamp = snapshot.timestamp;
        self.block_env.number = snapshot.block_number;
        self.cfg_env.chain_id = snapshot.chain_id;

        // 2. Restore Accounts
        self.db.accounts.clear(); // Wipe existing memory

        for (addr, saved_acc) in snapshot.accounts {
            // Reconstruct the complicated DbAccount struct
            let db_acc = DbAccount {
                info: saved_acc.info,
                storage: saved_acc.storage.clone().into_iter().collect(),
                account_state: AccountState::Touched, // Mark as 'active' so it doesn't get deleted
            };

            self.db.accounts.insert(addr, db_acc);
        }
    }

    /// Helper to get balance
    pub fn get_balance(&mut self, address: Address) -> Result<U256> {
        let acc = self.db.basic(address)?.unwrap_or_default();
        Ok(acc.balance)
    }

    /// Helper to get storage at a specific slot
    pub fn get_storage_at(&mut self, address: Address, slot: U256) -> Result<U256> {
        let val = self.db.storage(address, slot)?;
        Ok(val)
    }
}

use revm::inspector_handle_register;

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an Executor that never touches the network.
    /// Uses a pinned block number so `new()` skips `fetch_latest_block_number`.
    /// Pre-populates the coinbase account so EVM execution doesn't hit the backend.
    fn test_executor() -> Executor {
        let mut exec = Executor::new(
            "http://localhost:8545".to_string(), // dummy URL, never used
            1,     // chain_id
            Some(1), // pinned block — avoids network call
        );
        // Pre-populate the coinbase/beneficiary (default Address::ZERO) so the EVM
        // doesn't try to fetch it from the (unreachable) RPC backend.
        exec.db.insert_account_info(Address::ZERO, AccountInfo::default());
        exec
    }

    fn alice() -> Address {
        Address::repeat_byte(0xAA)
    }
    fn bob() -> Address {
        Address::repeat_byte(0xBB)
    }

    // --- Balance ---

    #[test]
    fn set_and_get_balance() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1_000_000));

        let bal = exec.get_balance(alice()).unwrap();
        assert_eq!(bal, U256::from(1_000_000));
    }

    #[test]
    fn get_balance_unknown_account_returns_zero() {
        let mut exec = test_executor();
        // Bob was never funded — but since the RPC backend would fail, we
        // insert a default account manually so the test stays offline.
        exec.db.insert_account_info(bob(), AccountInfo::default());

        let bal = exec.get_balance(bob()).unwrap();
        assert_eq!(bal, U256::ZERO);
    }

    #[test]
    fn set_balance_preserves_nonce() {
        let mut exec = test_executor();
        // Give alice a nonce of 10
        let info = AccountInfo {
            balance: U256::from(500),
            nonce: 10,
            ..Default::default()
        };
        exec.db.insert_account_info(alice(), info);

        // Now set balance — nonce should be preserved
        exec.set_balance(alice(), U256::from(9999));
        let nonce = exec.get_nonce(alice()).unwrap();
        assert_eq!(nonce, 10);
        let bal = exec.get_balance(alice()).unwrap();
        assert_eq!(bal, U256::from(9999));
    }

    // --- Code ---

    #[test]
    fn set_and_get_code() {
        let mut exec = test_executor();
        let bytecode = Bytes::from_static(&[0x60, 0x00, 0x60, 0x00, 0xFD]); // PUSH0 PUSH0 REVERT
        exec.set_code(alice(), bytecode.clone());

        let code = exec.get_code(alice()).unwrap();
        assert_eq!(code, bytecode);
    }

    #[test]
    fn get_code_empty_for_eoa() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1));
        let code = exec.get_code(alice()).unwrap();
        assert!(code.is_empty());
    }

    // --- Storage ---

    #[test]
    fn set_and_get_storage() {
        let mut exec = test_executor();
        // Insert account first so storage has a valid account_state
        exec.db.insert_account_info(alice(), AccountInfo::default());
        exec.set_storage_at(alice(), U256::from(0), U256::from(42)).unwrap();

        let val = exec.get_storage_at(alice(), U256::from(0)).unwrap();
        assert_eq!(val, U256::from(42));
    }

    #[test]
    fn storage_default_is_zero_for_new_account() {
        let mut exec = test_executor();
        // Insert account directly into the DB with StorageCleared state
        // so CacheDB knows all storage is zero without hitting the backend
        let db_acc = revm::db::DbAccount {
            info: AccountInfo::default(),
            storage: Default::default(),
            account_state: revm::db::AccountState::StorageCleared,
        };
        exec.db.accounts.insert(alice(), db_acc);

        let val = exec.get_storage_at(alice(), U256::from(99)).unwrap();
        assert_eq!(val, U256::ZERO);
    }

    // --- Nonce ---

    #[test]
    fn get_nonce_for_new_account() {
        let mut exec = test_executor();
        exec.db.insert_account_info(alice(), AccountInfo::default());

        let nonce = exec.get_nonce(alice()).unwrap();
        assert_eq!(nonce, 0);
    }

    // --- Block environment ---

    #[test]
    fn increase_time() {
        let mut exec = test_executor();
        let before = exec.block_env.timestamp;
        exec.increase_time(100);
        assert_eq!(exec.block_env.timestamp, before + U256::from(100));
    }

    #[test]
    fn set_block_timestamp() {
        let mut exec = test_executor();
        exec.set_block_timestamp(1700000000);
        assert_eq!(exec.block_env.timestamp, U256::from(1700000000u64));
    }

    #[test]
    fn set_block_number() {
        let mut exec = test_executor();
        exec.set_block_number(999);
        assert_eq!(exec.block_env.number, U256::from(999));
    }

    #[test]
    fn executor_uses_pinned_block_number() {
        let exec = Executor::new(
            "http://localhost:8545".to_string(),
            1,
            Some(18_000_000),
        );
        assert_eq!(exec.block_env.number, U256::from(18_000_000u64));
    }

    #[test]
    fn executor_respects_chain_id() {
        let exec = Executor::new(
            "http://localhost:8545".to_string(),
            137, // Polygon
            Some(1),
        );
        assert_eq!(exec.cfg_env.chain_id, 137);
    }

    // --- ETH Transfer ---

    #[test]
    fn simple_eth_transfer() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128)); // 1 ETH
        // Pre-populate bob so the EVM doesn't need to fetch from network
        exec.db.insert_account_info(bob(), AccountInfo::default());

        let result = exec.transact(
            alice(),
            Some(bob()),
            U256::from(500_000_000_000_000_000u128), // 0.5 ETH
            Bytes::default(),
        ).unwrap();

        assert!(result.is_success());

        let bob_bal = exec.get_balance(bob()).unwrap();
        assert_eq!(bob_bal, U256::from(500_000_000_000_000_000u128));
    }

    #[test]
    fn transfer_increments_nonce() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));
        exec.db.insert_account_info(bob(), AccountInfo::default());

        assert_eq!(exec.get_nonce(alice()).unwrap(), 0);

        let result = exec.transact(
            alice(),
            Some(bob()),
            U256::from(1000),
            Bytes::default(),
        ).unwrap();
        assert!(result.is_success());

        assert_eq!(exec.get_nonce(alice()).unwrap(), 1);

        // Second transfer
        let result2 = exec.transact(
            alice(),
            Some(bob()),
            U256::from(1000),
            Bytes::default(),
        ).unwrap();
        assert!(result2.is_success());

        assert_eq!(exec.get_nonce(alice()).unwrap(), 2);
    }

    #[test]
    fn transfer_insufficient_funds_reverts() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(100)); // only 100 wei
        exec.db.insert_account_info(bob(), AccountInfo::default());

        let result = exec.transact(
            alice(),
            Some(bob()),
            U256::from(1_000_000_000_000_000_000u128), // 1 ETH
            Bytes::default(),
        );

        // Should error because alice can't afford gas + value
        assert!(result.is_err());
    }

    // --- Gas estimation ---

    #[test]
    fn estimate_gas_simple_transfer() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));
        exec.db.insert_account_info(bob(), AccountInfo::default());

        let gas = exec.estimate_gas(
            alice(),
            Some(bob()),
            U256::from(1000),
            Bytes::default(),
        ).unwrap();

        // Simple ETH transfer should cost 21000 gas
        assert_eq!(gas, 21000);
    }

    // --- Snapshots ---

    #[test]
    fn snapshot_and_revert() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1000));

        // Take snapshot before changing state
        let snap_id = exec.take_snapshot();
        assert_eq!(snap_id, 0);

        // Modify state
        exec.set_balance(alice(), U256::from(9999));
        assert_eq!(exec.get_balance(alice()).unwrap(), U256::from(9999));

        // Revert
        let success = exec.revert_snapshot(snap_id);
        assert!(success);

        // Balance should be restored
        assert_eq!(exec.get_balance(alice()).unwrap(), U256::from(1000));
    }

    #[test]
    fn revert_invalid_snapshot_returns_false() {
        let mut exec = test_executor();
        assert!(!exec.revert_snapshot(999));
    }

    #[test]
    fn snapshot_preserves_block_env() {
        let mut exec = test_executor();
        exec.set_block_number(100);
        exec.set_block_timestamp(1700000000);

        let snap_id = exec.take_snapshot();

        exec.set_block_number(200);
        exec.set_block_timestamp(1800000000);

        exec.revert_snapshot(snap_id);
        assert_eq!(exec.block_env.number, U256::from(100));
        assert_eq!(exec.block_env.timestamp, U256::from(1700000000u64));
    }

    #[test]
    fn multiple_snapshots_invalidate_later_ones() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(100));

        let snap0 = exec.take_snapshot();
        exec.set_balance(alice(), U256::from(200));
        let _snap1 = exec.take_snapshot();
        exec.set_balance(alice(), U256::from(300));
        let _snap2 = exec.take_snapshot();

        // Revert to snap0 — snap1 and snap2 should be invalidated
        exec.revert_snapshot(snap0);
        assert_eq!(exec.get_balance(alice()).unwrap(), U256::from(100));
        assert_eq!(exec.snapshots.len(), 1); // only snap0 remains
    }

    // --- Snapshot serialization ---

    #[test]
    fn snapshot_round_trip() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(5000));
        exec.set_block_number(42);
        exec.set_block_timestamp(1700000000);

        let snapshot = exec.get_snapshot();

        // Serialize to JSON and back
        let json = serde_json::to_string(&snapshot).unwrap();
        let restored: StateSnapshot = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.block_number, U256::from(42));
        assert_eq!(restored.timestamp, U256::from(1700000000u64));
        assert_eq!(restored.chain_id, 1);
        assert!(restored.accounts.contains_key(&alice()));
        assert_eq!(
            restored.accounts[&alice()].info.balance,
            U256::from(5000)
        );
    }

    // --- Contract deployment ---

    #[test]
    fn deploy_simple_contract() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1_000_000_000_000_000_000u128));

        // Pre-populate the expected contract address.
        // CREATE address = keccak256(rlp([sender, nonce]))[12..]
        // For nonce 0, RLP([addr, 0]) = 0xd6 0x94 <20 byte addr> 0x80
        let mut rlp_input = vec![0xd6, 0x94];
        rlp_input.extend_from_slice(alice().as_slice());
        rlp_input.push(0x80); // RLP encoding of 0
        let hash = alloy_primitives::keccak256(&rlp_input);
        let expected_addr = Address::from_slice(&hash[12..]);
        exec.db.insert_account_info(expected_addr, AccountInfo::default());

        // Minimal init code: PUSH1 0x00 PUSH1 0x00 RETURN (returns empty runtime)
        let init_code = Bytes::from_static(&[0x60, 0x00, 0x60, 0x00, 0xF3]);

        let result = exec.transact(
            alice(),
            None, // Create
            U256::ZERO,
            init_code,
        ).unwrap();

        assert!(result.is_success());
        match result {
            ExecutionResult::Success { output, .. } => {
                match output {
                    Output::Create(_, addr) => {
                        assert!(addr.is_some());
                        assert_eq!(addr.unwrap(), expected_addr);
                    }
                    _ => panic!("Expected Create output"),
                }
            }
            _ => panic!("Expected success"),
        }
    }

    // --- Cheatcode: mine_blocks ---

    #[test]
    fn mine_blocks_advances_number_and_timestamp() {
        let mut exec = test_executor();
        let block_before = exec.block_env.number;
        let ts_before = exec.block_env.timestamp;

        exec.mine_blocks(5, None);

        assert_eq!(exec.block_env.number, block_before + U256::from(5));
        assert_eq!(exec.block_env.timestamp, ts_before + U256::from(60)); // 5 * 12s
    }

    #[test]
    fn mine_blocks_with_custom_interval() {
        let mut exec = test_executor();
        let ts_before = exec.block_env.timestamp;

        exec.mine_blocks(3, Some(5));

        assert_eq!(exec.block_env.timestamp, ts_before + U256::from(15)); // 3 * 5s
    }

    // --- Cheatcode: mine_one_block with next_block_timestamp ---

    #[test]
    fn mine_one_block_uses_next_timestamp() {
        let mut exec = test_executor();
        exec.set_next_block_timestamp(9999);

        exec.mine_one_block();

        assert_eq!(exec.block_env.timestamp, U256::from(9999));
        // next_block_timestamp should be consumed
        assert!(exec.next_block_timestamp.is_none());
    }

    #[test]
    fn mine_one_block_without_next_timestamp_adds_12s() {
        let mut exec = test_executor();
        let ts_before = exec.block_env.timestamp;
        let block_before = exec.block_env.number;

        exec.mine_one_block();

        assert_eq!(exec.block_env.number, block_before + U256::from(1));
        assert_eq!(exec.block_env.timestamp, ts_before + U256::from(12));
    }

    // --- Cheatcode: set_nonce ---

    #[test]
    fn set_nonce_updates_account() {
        let mut exec = test_executor();
        exec.set_balance(alice(), U256::from(1000));
        exec.set_nonce(alice(), 42);

        assert_eq!(exec.get_nonce(alice()).unwrap(), 42);
        // Balance should be preserved
        assert_eq!(exec.get_balance(alice()).unwrap(), U256::from(1000));
    }

    // --- Cheatcode: automine ---

    #[test]
    fn automine_defaults_to_true() {
        let exec = test_executor();
        assert!(exec.automine);
    }

    #[test]
    fn set_automine_toggles() {
        let mut exec = test_executor();
        exec.set_automine(false);
        assert!(!exec.automine);
        exec.set_automine(true);
        assert!(exec.automine);
    }

    // --- Cheatcode: impersonation ---

    #[test]
    fn impersonate_and_stop() {
        let mut exec = test_executor();
        assert!(!exec.is_impersonated(&alice()));

        exec.impersonate_account(alice());
        assert!(exec.is_impersonated(&alice()));

        exec.stop_impersonating_account(alice());
        assert!(!exec.is_impersonated(&alice()));
    }

    // --- Cheatcode: block_gas_limit ---

    #[test]
    fn set_block_gas_limit() {
        let mut exec = test_executor();
        assert_eq!(exec.block_gas_limit, 30_000_000);

        exec.set_block_gas_limit(50_000_000);
        assert_eq!(exec.block_gas_limit, 50_000_000);
    }
}
