use eidolon_evm::Executor;
use eidolon_rpc::{EidolonApiServer, EidolonRpc};
use jsonrpsee::RpcModule;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

/// Request to create a new fork.
#[derive(Deserialize, Debug, Clone)]
pub struct ForkCreateRequest {
    pub rpc_url: String,
    pub chain_id: Option<u64>,
    pub block_number: Option<u64>,
    pub fork_id: Option<String>,
}

/// Fork info returned by the API.
#[derive(Serialize, Debug, Clone)]
pub struct ForkInfo {
    pub id: String,
    pub rpc_url: String,
    pub chain_id: u64,
    pub block_number: String,
    pub timestamp: String,
    pub rpc_endpoint: String,
}

/// A managed fork instance.
pub struct Fork {
    pub id: String,
    pub chain_id: u64,
    pub rpc_url: String,
    pub executor: Arc<RwLock<Executor>>,
    pub rpc_module: RpcModule<EidolonRpc>,
}

impl Fork {
    pub fn info(&self, base_url: &str) -> ForkInfo {
        let executor = self.executor.read();
        ForkInfo {
            id: self.id.clone(),
            rpc_url: self.rpc_url.clone(),
            chain_id: self.chain_id,
            block_number: format!("{}", executor.block_env.number),
            timestamp: format!("{}", executor.block_env.timestamp),
            rpc_endpoint: format!("{}/rpc/{}", base_url, self.id),
        }
    }
}

/// Manages multiple fork instances.
pub struct ForkManager {
    forks: RwLock<HashMap<String, Arc<Fork>>>,
    pub redis_url: Option<String>,
}

impl ForkManager {
    pub fn new(redis_url: Option<String>) -> Self {
        Self {
            forks: RwLock::new(HashMap::new()),
            redis_url,
        }
    }

    /// Create a new fork from config.
    pub fn create_fork(&self, req: ForkCreateRequest) -> Arc<Fork> {
        let id = req
            .fork_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let chain_id = req.chain_id.unwrap_or(1);
        let rpc_url = req.rpc_url.clone();

        info!("🔱 Creating fork: id={}, chain={}, rpc={}", id, chain_id, rpc_url);

        let executor = Executor::new(rpc_url.clone(), chain_id, req.block_number);
        let shared_executor = Arc::new(RwLock::new(executor));

        let rpc = EidolonRpc::new(shared_executor.clone(), chain_id);
        let rpc_module = rpc.into_rpc();

        let fork = Arc::new(Fork {
            id: id.clone(),
            chain_id,
            rpc_url,
            executor: shared_executor,
            rpc_module,
        });

        self.forks.write().insert(id.clone(), fork.clone());

        info!("✅ Fork created: {}", id);
        fork
    }

    /// Get a fork by ID.
    pub fn get_fork(&self, id: &str) -> Option<Arc<Fork>> {
        self.forks.read().get(id).cloned()
    }

    /// Delete a fork by ID.
    pub fn delete_fork(&self, id: &str) -> bool {
        let removed = self.forks.write().remove(id).is_some();
        if removed {
            info!("🗑️ Fork deleted: {}", id);
        }
        removed
    }

    /// List all forks.
    pub fn list_forks(&self, base_url: &str) -> Vec<ForkInfo> {
        self.forks
            .read()
            .values()
            .map(|f| f.info(base_url))
            .collect()
    }

    /// Get fork count.
    pub fn fork_count(&self) -> usize {
        self.forks.read().len()
    }
}
