use alloy_primitives::{Address, Bytes, U256};
use anyhow::Result;
use revm::{
    Database, Evm,
    db::{CacheDB, EmptyDB},
    primitives::{AccountInfo, ExecutionResult, Output, TransactTo, TxEnv},
};

/// The Eidolon Executor
/// Wraps the EVM and the Database (State).
pub struct Executor {
    // We use CacheDB<EmptyDB> for Phase 1 (In-Memory only)
    pub db: CacheDB<EmptyDB>,
}

impl Executor {
    /// Create a new, empty execution environment
    pub fn new() -> Self {
        Self {
            db: CacheDB::new(EmptyDB::default()),
        }
    }

    /// "God Mode": Manually set the balance of an account
    /// This mimics `tenderly_setBalance`
    pub fn set_balance(&mut self, address: Address, amount: U256) {
        let mut account = AccountInfo::default();
        account.balance = amount;

        // Insert directly into the DB, bypassing execution
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
        // 1. Configure the Transaction Environment
        let mut evm = Evm::builder()
            .with_db(&mut self.db)
            .modify_tx_env(|tx| {
                tx.caller = caller;
                tx.transact_to = TransactTo::Call(to);
                tx.value = value;
                tx.data = data;
                tx.gas_limit = 30_000_000; // High gas limit for testing
                tx.gas_price = U256::from(1);
            })
            .build();

        // 2. Execute
        // transact_commit() executes AND writes changes to the DB
        let result = evm.transact_commit()?;

        Ok(result)
    }

    /// Helper to get balance
    pub fn get_balance(&mut self, address: Address) -> Result<U256> {
        let acc = self.db.basic(address)?.unwrap_or_default();
        Ok(acc.balance)
    }
}
