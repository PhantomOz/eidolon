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
