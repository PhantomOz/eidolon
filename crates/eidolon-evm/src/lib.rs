pub mod tracer;

use alloy_primitives::{Address, Bytes, U256};
use anyhow::Result;
use eidolon_forkdb::{ForkDB, new_fork_db};
use revm::{
    Database, Evm,
    primitives::{AccountInfo, BlockEnv, CfgEnv, ExecutionResult, Output, TransactTo},
};
use tracer::EidolonTracer;

/// The Eidolon Executor
/// Wraps the EVM and the Database (State).
pub struct Executor {
    pub db: ForkDB,
    pub block_env: BlockEnv,
    pub cfg_env: CfgEnv,
}

impl Executor {
    pub fn new(rpc_url: String, chain_id: u64) -> Self {
        let db = new_fork_db(rpc_url);

        // Setup Block Time
        let mut block_env = BlockEnv::default();
        block_env.timestamp = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        );

        // Setup Chain Config
        let mut cfg_env = CfgEnv::default();
        cfg_env.chain_id = chain_id; // Set the Chain ID dynamically

        Self {
            db,
            block_env,
            cfg_env,
        }
    }

    pub fn increase_time(&mut self, seconds: u64) {
        self.block_env.timestamp += U256::from(seconds);
    }

    pub fn set_balance(&mut self, address: Address, amount: U256) {
        let mut account = AccountInfo::default();
        account.balance = amount;
        self.db.insert_account_info(address, account);
    }

    /// Execute a transaction
    pub fn transact(
        &mut self,
        caller: Address,
        to: Address,
        value: U256,
        data: Bytes,
    ) -> Result<ExecutionResult> {
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = TransactTo::Call(to);
                tx.value = value;
                tx.data = data;
                tx.gas_limit = 30_000_000;
                tx.gas_price = U256::from(1);
            })
            .build();

        let result = evm
            .transact_commit()
            .map_err(|e| anyhow::anyhow!("EVM Execution Error: {:?}", e))?;

        Ok(result)
    }

    pub fn call(
        &mut self,
        caller: Address,
        to: Address,
        value: U256,
        data: Bytes,
    ) -> Result<Bytes> {
        // We use transact_ref() so we don't consume the DB
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = TransactTo::Call(to);
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
        to: Address,
        value: U256,
        data: Bytes,
    ) -> Result<EidolonTracer> {
        // 1. Initialize Tracer
        let mut tracer = EidolonTracer::default();

        // 2. Build EVM with Inspector attached
        {
            let mut evm = Evm::builder()
                .with_db(&mut self.db)
                .with_external_context(&mut tracer) // Pass tracer here
                .modify_tx_env(|tx| {
                    tx.caller = caller;
                    tx.transact_to = TransactTo::Call(to);
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

    /// Helper to get balance
    pub fn get_balance(&mut self, address: Address) -> Result<U256> {
        let acc = self.db.basic(address)?.unwrap_or_default();
        Ok(acc.balance)
    }
}

use revm::inspector_handle_register;
