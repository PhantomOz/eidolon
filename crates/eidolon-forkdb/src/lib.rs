use alloy_primitives::{Address, B256, U64, U256};
use anyhow::Result;
use revm::{
    DatabaseRef,
    db::CacheDB,
    primitives::{AccountInfo, Bytecode},
};

use tracing::info;

/// The configuration for the fork
#[derive(Clone)]
pub struct ForkConfig {
    pub rpc_url: String,
    pub block_number: Option<u64>,
}

/// The Backend that fetches data from RPC
pub struct RpcBackend {
    config: ForkConfig,
    agent: ureq::Agent,
}

impl RpcBackend {
    pub fn new(config: ForkConfig) -> Self {
        Self {
            config,
            agent: ureq::Agent::new(),
        }
    }

    /// Helper to make JSON-RPC calls
    fn call_rpc(&self, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let res: serde_json::Value = self
            .agent
            .post(&self.config.rpc_url)
            .send_json(body)?
            .into_json()?;

        // Basic error handling
        if let Some(err) = res.get("error") {
            anyhow::bail!("RPC Error: {:?}", err);
        }

        Ok(res["result"].clone())
    }
}

/// The Database Implementation
/// When REVM asks for data, we fetch it here.
impl DatabaseRef for RpcBackend {
    type Error = anyhow::Error;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        info!("🌍 Fetching Account: {:?}", address);

        let bal_hex = self.call_rpc("eth_getBalance", serde_json::json!([address, "latest"]))?;
        let balance: U256 = serde_json::from_value(bal_hex)?;

        let nonce_hex = self.call_rpc(
            "eth_getTransactionCount",
            serde_json::json!([address, "latest"]),
        )?;
        let nonce_alloy: U64 = serde_json::from_value(nonce_hex)?;
        let nonce = nonce_alloy.to::<u64>(); // Safe cast

        let code_hex = self.call_rpc("eth_getCode", serde_json::json!([address, "latest"]))?;
        let code_bytes: alloy_primitives::Bytes = serde_json::from_value(code_hex)?;

        let bytecode = if code_bytes.is_empty() {
            Bytecode::new()
        } else {
            Bytecode::new_raw(code_bytes)
        };

        let info = AccountInfo {
            balance,
            nonce,
            code_hash: bytecode.hash_slow(),
            code: Some(bytecode),
        };

        Ok(Some(info))
    }

    fn code_by_hash_ref(&self, _code_hash: B256) -> Result<Bytecode, Self::Error> {
        // TODO: implementation, you'd need to handle this.
        // For basic_ref, we already fetched the code, so REVM usually handles this.
        Ok(Bytecode::new())
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        info!("🌍 Fetching Storage: {:?} at {:?}", address, index);

        let val_hex = self.call_rpc(
            "eth_getStorageAt",
            serde_json::json!([address, index, "latest"]),
        )?;
        let val: U256 = serde_json::from_value(val_hex)?;

        Ok(val)
    }

    fn block_hash_ref(&self, _: u64) -> Result<B256, Self::Error> {
        Ok(B256::ZERO) // TODO: Simplified for now
    }
}

/// We wrap our RpcBackend in a CacheDB so we write to memory but read from RPC.
pub type ForkDB = CacheDB<RpcBackend>;

pub fn new_fork_db(rpc_url: String, block_number: Option<u64>) -> ForkDB {
    let backend = RpcBackend::new(ForkConfig {
        rpc_url,
        block_number,
    });
    CacheDB::new(backend)
}
