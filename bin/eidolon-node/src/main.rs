use clap::Parser;
use eidolon_evm::Executor;
use eidolon_rpc::{EidolonApiServer, EidolonRpc};
use jsonrpsee::server::Server;
use parking_lot::RwLock;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

/// Eidolon: The Virtual Ethereum Node
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// The Upstream RPC URL to fork from (e.g., Alchemy/Infura)
    #[arg(long, env = "RPC_URL")]
    rpc_url: String,

    /// Port to listen on
    #[arg(short, long, default_value_t = 8545)]
    port: u16,

    /// Chain ID to mimic (1 = Mainnet, 137 = Polygon, etc.)
    #[arg(short, long, default_value_t = 1)]
    chain_id: u64,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Parse CLI Arguments
    let args = Args::parse();

    // 2. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("👻 Eidolon v0.1.0 - The Virtual Testnet");
    info!("🌍 Forking from: {}", args.rpc_url);
    info!("🔗 Chain ID: {}", args.chain_id);

    // 3. Initialize Executor with Config
    let executor = Executor::new(args.rpc_url, args.chain_id);
    let shared_executor = Arc::new(RwLock::new(executor));

    // 4. Configure Server
    let addr = SocketAddr::from(([127, 0, 0, 1], args.port)); // Listen on 0.0.0.0 for Docker
    let server = Server::builder().build(addr).await?;

    // Pass config to RPC
    let rpc_module = EidolonRpc::new(shared_executor, args.chain_id);

    let handle = server.start(rpc_module.into_rpc());

    info!("🚀 Server running at http://{}", addr);

    handle.stopped().await;

    Ok(())
}
