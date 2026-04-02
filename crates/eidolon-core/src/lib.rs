pub mod api;
pub mod auth;
pub mod fork_manager;

use anyhow::Result;
use api::AppState;
use auth::AuthManager;
use axum::{
    Router,
    middleware,
    routing::{delete, get, post},
};
use fork_manager::{ForkCreateRequest, ForkManager};
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

pub struct NodeConfig {
    pub rpc_url: Option<String>,
    pub port: u16,
    pub chain_id: u64,
    pub block_number: Option<u64>,
    pub fork_id: String,
    pub redis_url: Option<String>,
    pub auth_enabled: bool,
}

pub struct EidolonNode;

impl EidolonNode {
    pub async fn run(config: NodeConfig) -> Result<()> {
        info!("👻 Eidolon v0.2.0 — SaaS Edition");
        info!("🚀 Mode: {}", if config.rpc_url.is_some() { "Single Fork" } else { "SaaS (API)" });
        info!("🔐 Auth: {}", if config.auth_enabled { "Enabled" } else { "Disabled" });

        let fork_manager = ForkManager::new(config.redis_url.clone());
        let auth = AuthManager::new(config.auth_enabled);

        // If rpc_url is provided, auto-create a default fork (backward compat)
        if let Some(ref rpc_url) = config.rpc_url {
            info!("🔱 Auto-creating default fork: id={}", config.fork_id);
            fork_manager.create_fork(ForkCreateRequest {
                rpc_url: rpc_url.clone(),
                chain_id: Some(config.chain_id),
                block_number: config.block_number,
                fork_id: Some(config.fork_id.clone()),
            });
        }

        let base_url = format!("http://0.0.0.0:{}", config.port);

        let state = Arc::new(AppState {
            fork_manager,
            auth,
            base_url: base_url.clone(),
        });

        let cors = CorsLayer::new()
            .allow_methods(Any)
            .allow_origin(Any)
            .allow_headers(Any);

        let shared_state = state.clone();

        let app = Router::new()
            // Health
            .route("/health", get(api::health))
            // API Key Management
            .route("/api/keys", post(api::create_key))
            .route("/api/keys", get(api::list_keys))
            .route("/api/keys/:key", delete(api::delete_key_handler))
            // Usage Metering
            .route("/api/usage", get(api::usage_stats))
            // Fork Management REST API
            .route("/api/forks", post(api::create_fork))
            .route("/api/forks", get(api::list_forks))
            .route("/api/forks/:id", get(api::get_fork))
            .route("/api/forks/:id", delete(api::delete_fork))
            .route("/api/forks/:id/transactions", get(api::get_fork_transactions))
            // Fork Snapshots
            .route("/api/forks/:id/snapshot", post(api::snapshot_fork))
            .route("/api/forks/:id/restore/:snap_id", post(api::restore_fork))
            // JSON-RPC Router
            .route("/rpc/:fork_id", post(api::handle_rpc))
            // Auth middleware
            .layer(middleware::from_fn(move |headers, request, next| {
                let state = shared_state.clone();
                auth::auth_middleware(headers, state, request, next)
            }))
            .layer(cors)
            .with_state(state.clone());

        let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
        info!("🚀 Server running at http://{}", addr);
        info!("📡 REST API: http://{}/api/forks", addr);
        info!("📊 Usage: http://{}/api/usage", addr);

        if let Some(ref _rpc_url) = config.rpc_url {
            info!(
                "🔗 Default fork RPC: http://{}/rpc/{}",
                addr, config.fork_id
            );
        }

        // Start periodic Redis save background task
        if config.redis_url.is_some() {
            let save_state = state.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
                loop {
                    interval.tick().await;
                    let forks = save_state.fork_manager.list_forks("");
                    if !forks.is_empty() {
                        info!("💾 Periodic save: {} forks", forks.len());
                        // Save each fork's state to Redis
                        if let Some(ref redis_url) = save_state.fork_manager.redis_url {
                            match redis::Client::open(redis_url.as_str()) {
                                Ok(client) => {
                                    if let Ok(mut conn) = client.get_connection() {
                                        save_state.fork_manager.save_all_forks(&mut conn);
                                    }
                                }
                                Err(e) => {
                                    tracing::warn!("⚠️ Redis connection failed: {}", e);
                                }
                            }
                        }
                    }
                }
            });
            info!("💾 Periodic Redis save: every 60s");
        }

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                match tokio::signal::ctrl_c().await {
                    Ok(()) => info!("🛑 Shutting down..."),
                    Err(err) => error!("Unable to listen for shutdown signal: {}", err),
                }
            })
            .await?;

        Ok(())
    }
}

