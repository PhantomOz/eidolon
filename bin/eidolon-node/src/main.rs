use alloy_primitives::{address, uint};
use eidolon_evm::Executor;
use eidolon_rpc::{EidolonApiServer, EidolonRpc};
use jsonrpsee::server::Server;
use parking_lot::RwLock;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("👻 Eidolon v0.1.0 - Phase 2: API Gateway");

    // 2. Initialize the Virtual Machine
    let mut executor = Executor::new();

    // 3. Define Actors
    let alice = address!("0000000000000000000000000000000000000001");

    // 4. God Mode: Give Alice 100 ETH
    // 1 ETH = 10^18 wei
    let one_eth = uint!(1_000_000_000_000_000_000_U256);
    let start_balance = one_eth * uint!(100_U256);

    executor.set_balance(alice, start_balance);
    info!("💰 Funded Alice with 100 ETH");

    let shared_executor = Arc::new(RwLock::new(executor));

    // 3. Configure the JSON-RPC Server
    let addr = SocketAddr::from(([127, 0, 0, 1], 3001));
    let server = Server::builder().build(addr).await?;

    // 4. Register Methods
    let rpc_module = EidolonRpc::new(shared_executor);
    let handle = server.start(rpc_module.into_rpc());

    info!("🚀 Server running at http://{}", addr);
    info!(
        "   Try: curl -X POST -H 'Content-Type: application/json' -d '{{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBalance\",\"params\":[\"{:?}\", \"latest\"],\"id\":1}}' http://localhost:3001",
        alice
    );

    // 5. Keep running until Ctrl+C
    handle.stopped().await;

    Ok(())
}
