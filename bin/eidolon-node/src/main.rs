use clap::Parser;
use eidolon_core::{EidolonNode, NodeConfig};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(author, version, about = "Eidolon — Virtual Testnet Engine")]
struct Args {
    /// Upstream RPC URL to fork from. If omitted, starts in SaaS mode (create forks via API).
    #[arg(long, env = "RPC_URL")]
    rpc_url: Option<String>,

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

    /// Enable API key authentication. When enabled, all requests require X-API-Key header.
    #[arg(long, env = "AUTH_ENABLED", default_value_t = false)]
    auth: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    let config = NodeConfig {
        rpc_url: args.rpc_url,
        port: args.port,
        chain_id: args.chain_id,
        block_number: args.block_number,
        fork_id: args.fork_id,
        redis_url: args.redis_url,
        auth_enabled: args.auth,
    };

    EidolonNode::run(config).await?;

    Ok(())
}

