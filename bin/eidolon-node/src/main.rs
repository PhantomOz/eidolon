use alloy_primitives::{address, uint};
use eidolon_evm::Executor;
use eidolon_rpc::{EidolonApiServer, EidolonRpc};
use jsonrpsee::server::Server;
use parking_lot::RwLock;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("👻 Eidolon v0.1.0 - Phase 3: Lazy Forking");

    // 2. Get RPC URL from Environment
    // You MUST set this, or the node won't know where to fork from.
    let rpc_url = env::var("RPC_URL").map_err(|_| {
        error!("CRITICAL: RPC_URL environment variable is not set.");
        anyhow::anyhow!("Missing RPC_URL")
    })?;

    info!("🌍 Forking from upstream: {}", rpc_url);

    // 3. Initialize the Executor with the URL
    let executor = Executor::new(rpc_url);
    let shared_executor = Arc::new(RwLock::new(executor));

    // 4. Configure Server
    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    let server = Server::builder().build(addr).await?;
    let rpc_module = EidolonRpc::new(shared_executor);
    let handle = server.start(rpc_module.into_rpc());

    info!("🚀 Server running at http://{}", addr);

    handle.stopped().await;

    Ok(())
}
