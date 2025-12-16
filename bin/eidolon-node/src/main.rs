use clap::Parser;
use eidolon_evm::{Executor, StateSnapshot};
use eidolon_rpc::{EidolonApiServer, EidolonRpc};
use redis::Commands;
// FIX: Import ServerBuilder explicitly
use jsonrpsee::server::ServerBuilder;
use parking_lot::RwLock;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tracing::{Level, error, info};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(author, version, about)]
struct Args {
    #[arg(long, env = "RPC_URL")]
    rpc_url: String,

    #[arg(short, long, env = "PORT", default_value_t = 8545)]
    port: u16,

    #[arg(short, long, env = "CHAIN_ID", default_value_t = 1)]
    chain_id: u64,

    #[arg(short, long)]
    block_number: Option<u64>,

    #[arg(long, env = "FORK_ID", default_value = "default")]
    fork_id: String,

    #[arg(long, env = "REDIS_URL")]
    redis_url: Option<String>,
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
    info!("🌍 Upstream: {}", args.rpc_url);

    let mut executor = Executor::new(args.rpc_url.clone(), args.chain_id, args.block_number);

    if let Some(ref redis_url) = args.redis_url {
        info!("💾 Connecting to Redis at {}", redis_url);
        match redis::Client::open(redis_url.as_str()) {
            Ok(client) => {
                if let Ok(mut con) = client.get_connection() {
                    let key = format!("fork:{}:state", args.fork_id);
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

    let addr = SocketAddr::from(([0, 0, 0, 0], args.port));

    let cors = CorsLayer::new()
        .allow_methods(Any) // Allow POST, GET, OPTIONS
        .allow_origin(Any) // Allow chrome-extension://...
        .allow_headers(Any); // Allow Content-Type: application/json

    let service = ServiceBuilder::new().layer(cors);

    // FIX: Configure server to prevent connection resets on large/weird requests
    let server = ServerBuilder::default()
        .max_request_body_size(10 * 1024 * 1024) // 10MB
        .max_response_body_size(10 * 1024 * 1024) // 10MB
        .set_http_middleware(service)
        .build(addr)
        .await?;

    let rpc_module = EidolonRpc::new(shared_executor.clone(), args.chain_id);
    let handle = server.start(rpc_module.into_rpc());

    info!("🚀 Server running at http://{}", addr);

    // 4. WAIT for Shutdown
    match tokio::signal::ctrl_c().await {
        Ok(()) => info!("🛑 Shutting down..."),
        Err(err) => error!("Unable to listen for shutdown signal: {}", err),
    }

    // 5. SAVE STATE
    if let Some(ref redis_url) = args.redis_url {
        info!("💾 Saving state to Redis...");
        let mut executor = shared_executor.write();
        let snapshot = executor.get_snapshot();
        let json = serde_json::to_string(&snapshot)?;

        let client = redis::Client::open(redis_url.as_str())?;
        let mut con = client.get_connection()?;
        let key = format!("fork:{}:state", args.fork_id);
        let _: () = con.set_ex(key, json, 24 * 60 * 60)?;
        info!("✅ State saved successfully!");
    }

    Ok(())
}
