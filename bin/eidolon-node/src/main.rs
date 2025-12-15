use clap::Parser;
use eidolon_evm::{Executor, StateSnapshot};
use eidolon_rpc::{EidolonApiServer, EidolonRpc};
use jsonrpsee::server::Server;
use parking_lot::RwLock;
use redis::Commands;
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

    // Unique ID for this fork session
    #[arg(long, env = "FORK_ID", default_value = "default")]
    fork_id: String,

    // Redis URL (e.g., redis://127.0.0.1/)
    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,

    /// Block number to start from
    #[arg(long, env = "BLOCK_NUMBER", default_value = None)]
    block_number: Option<u64>,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("👻 Eidolon v0.1.0 - SaaS Edition");
    info!("🆔 Fork ID: {}", args.fork_id);

    let mut executor = Executor::new(args.rpc_url.clone(), args.chain_id, args.block_number);

    if let Some(ref redis_url) = args.redis_url {
        info!("💾 Connecting to Redis at {}", redis_url);
        match redis::Client::open(redis_url.as_str()) {
            Ok(client) => {
                if let Ok(mut con) = client.get_connection() {
                    let key = format!("fork:{}:state", args.fork_id);

                    let state_json: Option<String> = con.get(&key).unwrap_or(None);

                    if let Some(json) = state_json {
                        info!("📥 Loading existing state from Redis...");
                        match serde_json::from_str::<StateSnapshot>(&json) {
                            Ok(snapshot) => executor.load_snapshot(snapshot),
                            Err(e) => error!("❌ Failed to deserialize state: {:?}", e),
                        }
                    } else {
                        info!("✨ No previous state found. Starting fresh.");
                    }
                }
            }
            Err(e) => error!("❌ Redis Connection Failed: {:?}", e),
        }
    }

    let shared_executor = Arc::new(RwLock::new(executor));

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));
    let server = Server::builder().build(addr).await?;
    let rpc_module = EidolonRpc::new(shared_executor.clone(), args.chain_id);
    server.start(rpc_module.into_rpc());

    info!("🚀 Server running at http://{}", addr);

    match tokio::signal::ctrl_c().await {
        Ok(()) => {
            info!("🛑 Shutting down...");
        }
        Err(err) => {
            error!("Unable to listen for shutdown signal: {}", err);
        }
    }

    if let Some(ref redis_url) = args.redis_url {
        info!("💾 Saving state to Redis...");

        let mut executor = shared_executor.write();
        let snapshot = executor.get_snapshot();
        let json = serde_json::to_string(&snapshot)?;

        let client = redis::Client::open(redis_url.as_str())?;
        let mut con = client.get_connection()?;

        let key = format!("fork:{}:state", args.fork_id);
        // Save with 24h expiry
        let _: () = con.set_ex(key, json, 24 * 60 * 60)?;

        info!("✅ State saved successfully!");
    }

    Ok(())
}
