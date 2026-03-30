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
use std::collections::HashMap;
use tracer::EidolonTracer;
use tracing::{info, warn};

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

        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = transact_to;
                tx.value = value;
                tx.data = data;
                tx.gas_limit = 30_000_000;
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
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = transact_to;
                tx.value = value;
                tx.data = data;
                tx.gas_limit = 30_000_000; // Large limit for estimation
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
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = transact_to;
                tx.value = value;
                tx.data = data;
                tx.gas_limit = 30_000_000; // Infinite gas for reading
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
            let mut evm = Evm::builder()
                .with_db(&mut self.db)
                .with_external_context(&mut tracer) // Pass tracer here
                .modify_tx_env(|tx| {
                    tx.caller = caller;
                    tx.transact_to = transact_to;
                    tx.value = value;
                    tx.data = data;
                    tx.gas_limit = 30_000_000;
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
