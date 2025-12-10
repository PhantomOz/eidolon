use alloy_primitives::{Address, B256, U256};
use anyhow::Result;
use revm::{
    Database, DatabaseRef,
    db::{CacheDB, EmptyDB},
    primitives::{AccountInfo, Bytecode},
};
use serde::Deserialize;
use std::collections::HashMap;
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
    client: reqwest::blocking::Client, // Using blocking for simplicity in Phase 3
}

impl RpcBackend {
    pub fn new(config: ForkConfig) -> Self {
        Self {
            config,
            client: reqwest::blocking::Client::new(),
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
            .client
            .post(&self.config.rpc_url)
            .json(&body)
            .send()?
            .json()?;

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

        // 1. Fetch Balance
        let bal_hex = self.call_rpc("eth_getBalance", serde_json::json!([address, "latest"]))?;
        let balance: U256 = serde_json::from_value(bal_hex)?;

        // 2. Fetch Nonce
        let nonce_hex = self.call_rpc(
            "eth_getTransactionCount",
            serde_json::json!([address, "latest"]),
        )?;
        let nonce: u64 = serde_json::from_value(nonce_hex)?;

        // 3. Fetch Code (Smart Contract Bytecode)
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
        // In a real implementation, you'd need to handle this.
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

    fn block_hash_ref(&self, number: U256) -> Result<B256, Self::Error> {
        Ok(B256::ZERO) // Simplified for now
    }
}
