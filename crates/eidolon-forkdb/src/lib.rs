use alloy_primitives::{Address, B256, U64, U256};
use anyhow::Result;
use eidolon_types::ForkConfig;
use revm::{
    DatabaseRef,
    db::CacheDB,
    primitives::{AccountInfo, Bytecode},
};

use tracing::{info, warn};

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

    /// Returns the block tag to use in RPC calls.
    /// If a block number is pinned, returns its hex representation; otherwise returns "latest".
    fn block_tag(&self) -> String {
        match self.config.block_number {
            Some(n) => format!("0x{:x}", n),
            None => "latest".to_string(),
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

        let tag = self.block_tag();

        let bal_hex = self.call_rpc("eth_getBalance", serde_json::json!([address, &tag]))?;
        let balance: U256 = serde_json::from_value(bal_hex)?;

        let nonce_hex = self.call_rpc(
            "eth_getTransactionCount",
            serde_json::json!([address, &tag]),
        )?;
        let nonce_alloy: U64 = serde_json::from_value(nonce_hex)?;
        let nonce = nonce_alloy.to::<u64>(); // Safe cast

        let code_hex = self.call_rpc("eth_getCode", serde_json::json!([address, &tag]))?;
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

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        // REVM typically calls basic_ref first, which populates the cache.
        // If we reach here, it means the code is missing from cache or we are in a weird state.
        // Since we can't fetch code by hash from standard RPC, we return empty.
        warn!("⚠️ code_by_hash_ref called for {:?}. Returning empty bytecode.", code_hash);
        Ok(Bytecode::new())
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        info!("🌍 Fetching Storage: {:?} at {:?}", address, index);

        let val_hex = self.call_rpc(
            "eth_getStorageAt",
            serde_json::json!([address, index, self.block_tag()]),
        )?;
        let val: U256 = serde_json::from_value(val_hex)?;

        Ok(val)
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        info!("🌍 Fetching Block Hash: {}", number);
        let block_hex = self.call_rpc(
            "eth_getBlockByNumber",
            serde_json::json!([format!("0x{:x}", number), false]),
        )?;

        if block_hex.is_null() {
             return Ok(B256::ZERO);
        }

        let hash_val = block_hex.get("hash").and_then(|v| v.as_str()).unwrap_or_default();
        let hash: B256 = hash_val.parse().unwrap_or(B256::ZERO);
        Ok(hash)
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

pub fn fetch_latest_block_number(rpc_url: &str) -> Result<u64, anyhow::Error> {
    let agent = ureq::Agent::new();
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    });

    let response: serde_json::Value = agent.post(rpc_url).send_json(body)?.into_json()?;

    if let Some(err) = response.get("error") {
        anyhow::bail!("RPC Error: {:?}", err);
    }

    let hex_val = response["result"]
        .as_str()
        .ok_or(anyhow::anyhow!("Invalid response"))?;
    // Parse Hex to u64
    let num = u64::from_str_radix(hex_val.trim_start_matches("0x"), 16)?;

    Ok(num)
}
