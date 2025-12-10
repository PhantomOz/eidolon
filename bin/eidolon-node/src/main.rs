use alloy_primitives::{Bytes, U256, address, uint};
use eidolon_evm::Executor;
use tracing::{Level, info};
use tracing_subscriber::FmtSubscriber;

fn main() -> anyhow::Result<()> {
    // 1. Setup Logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    info!("👻 Eidolon v0.1.0 - Phase 1: Execution Core");

    // 2. Initialize the Virtual Machine
    let mut executor = Executor::new();

    // 3. Define Actors
    let alice = address!("0000000000000000000000000000000000000001");
    let bob = address!("0000000000000000000000000000000000000002");

    // 4. God Mode: Give Alice 100 ETH
    // 1 ETH = 10^18 wei
    let one_eth = uint!(1_000_000_000_000_000_000_U256);
    let start_balance = one_eth * uint!(100_U256);

    executor.set_balance(alice, start_balance);
    info!("💰 Funded Alice with 100 ETH");

    // 5. Execute: Alice sends 10 ETH to Bob
    let value_to_send = one_eth * uint!(10_U256);

    info!("🚀 Executing Transaction: Alice -> Bob (10 ETH)...");

    let result = executor.transact(
        alice,
        bob,
        value_to_send,
        Bytes::default(), // Empty data (simple transfer)
    )?;

    // 6. Inspect Results
    info!("✅ Execution Status: {:?}", result.is_success());

    let alice_new_bal = executor.get_balance(alice)?;
    let bob_new_bal = executor.get_balance(bob)?;

    info!("📊 Final State:");
    info!("   Alice Balance: {} wei", alice_new_bal);
    info!("   Bob Balance:   {} wei", bob_new_bal);

    assert_eq!(bob_new_bal, value_to_send);
    info!("👻 Simulation complete. The ghost is alive.");

    Ok(())
}
