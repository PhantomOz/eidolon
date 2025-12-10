use alloy_primitives::{Address, Bytes, U256};
use anyhow::Result;
use eidolon_forkdb::{ForkDB, new_fork_db};
use revm::{
    Database, Evm,
    db::{CacheDB, EmptyDB},
    primitives::{AccountInfo, ExecutionResult, Output, TransactTo, TxEnv},
};

/// The Eidolon Executor
/// Wraps the EVM and the Database (State).
pub struct Executor {
    pub db: ForkDB,
}

impl Executor {
    pub fn new(rpc_url: String) -> Self {
        Self {
            db: new_fork_db(rpc_url),
        }
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

    /// Helper to get balance
    pub fn get_balance(&mut self, address: Address) -> Result<U256> {
        let acc = self.db.basic(address)?.unwrap_or_default();
        Ok(acc.balance)
    }
}
