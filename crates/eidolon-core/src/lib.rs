use anyhow::Result;
use eidolon_evm::{Executor, StateSnapshot};
use eidolon_rpc::{EidolonApiServer, EidolonRpc};
use jsonrpsee::server::ServerBuilder;
use parking_lot::RwLock;
use redis::Commands;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

pub struct NodeConfig {
    pub rpc_url: String,
    pub port: u16,
    pub chain_id: u64,
    pub block_number: Option<u64>,
    pub fork_id: String,
    pub redis_url: Option<String>,
}

pub struct EidolonNode;

impl EidolonNode {
    pub async fn run(config: NodeConfig) -> Result<()> {
        info!("👻 Eidolon v0.1.0 - SaaS Edition");
        info!("🆔 Fork ID: {}", config.fork_id);
        info!("🌍 Upstream: {}", config.rpc_url);

        let mut executor = Executor::new(config.rpc_url.clone(), config.chain_id, config.block_number);

        // Load State from Redis if available
        if let Some(ref redis_url) = config.redis_url {
            info!("💾 Connecting to Redis at {}", redis_url);
            match redis::Client::open(redis_url.as_str()) {
                Ok(client) => {
                    if let Ok(mut con) = client.get_connection() {
                        let key = format!("fork:{}:state", config.fork_id);
                        let result: redis::RedisResult<Option<String>> = con.get(&key);
                        match result {
                            Ok(Some(json)) => {
                                info!("📥 Loading existing state from Redis...");
                                match serde_json::from_str::<StateSnapshot>(&json) {
                                    Ok(snapshot) => executor.load_snapshot(snapshot),
                                    Err(e) => error!("❌ Failed to deserialize state: {:?}", e),
                                }
                            }
                            Ok(None) => info!("✨ No previous state found. Starting fresh."),
                            Err(e) => error!("❌ Redis Read Error: {:?}", e),
                        }
                    }
                }
                Err(e) => error!("❌ Redis Connection Failed: {:?}", e),
            }
        }

        let shared_executor = Arc::new(RwLock::new(executor));

        let addr = SocketAddr::from(([0, 0, 0, 0], config.port));

        let cors = CorsLayer::new()
            .allow_methods(Any)
            .allow_origin(Any)
            .allow_headers(Any);

        let service = ServiceBuilder::new().layer(cors);

        let server = ServerBuilder::default()
            .max_request_body_size(10 * 1024 * 1024)
            .max_response_body_size(10 * 1024 * 1024)
            .set_http_middleware(service)
            .build(addr)
            .await?;

        let rpc_module = EidolonRpc::new(shared_executor.clone(), config.chain_id);
        let _handle = server.start(rpc_module.into_rpc());

        info!("🚀 Server running at http://{}", addr);

        // Wait for Shutdown
        match tokio::signal::ctrl_c().await {
            Ok(()) => info!("🛑 Shutting down..."),
            Err(err) => error!("Unable to listen for shutdown signal: {}", err),
        }

        // Save State
        if let Some(ref redis_url) = config.redis_url {
            info!("💾 Saving state to Redis...");
            let mut executor = shared_executor.write();
            let snapshot = executor.get_snapshot();
            let json = serde_json::to_string(&snapshot)?;

            let client = redis::Client::open(redis_url.as_str())?;
            let mut con = client.get_connection()?;
            let key = format!("fork:{}:state", config.fork_id);
            let _: () = con.set_ex(key, json, 24 * 60 * 60)?;
            info!("✅ State saved successfully!");
        }

        Ok(())
    }
}
