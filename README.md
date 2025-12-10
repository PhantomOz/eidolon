# Eidolon 👻

"Ephemeral State, Infinite Possibility."

Eidolon is a high-performance Virtual Testnet Engine written in Rust. It allows developers to fork the Ethereum state instantly, simulate complex transactions, and debug smart contracts with instruction-level precision—without the overhead of running a full node.

## 🏗 Architecture

Eidolon rejects the traditional blockchain node architecture (Geth/Reth) in favor of a State-Transition-as-a-Service model.

### Core Principles

1. Lazy Forking: We never sync the chain. We fetch state on-demand from upstream Archive Nodes (Alchemy/Infura) only when the EVM attempts to read a slot that isn't in our local cache.
2. Ephemeral by Design: Forks are temporary. They exist only as long as the session is active (or persisted via Redis for "Time Travel").
3. Deterministic Time: Time is a variable, not a constant. Users can warp block.timestamp forward to test expirations and timelocks instantly.

### The Stack

- Language: Rust 🦀
- EVM: revm (The fastest EVM implementation)
- RPC: jsonrpsee
- Data Types: alloy-rs
- Async Runtime: Tokio
- Storage: Redis (Hot State) + Postgres (Metadata)

## 📂 Workspace Structure

Eidolon is a Rust Monorepo organized into specialized crates:

```
eidolon/
├── Cargo.toml              # Workspace definitions
├── crates/
│   ├── eidolon-core/       # The heartbeat. Contains the Event Loop and Actor logic.
│   ├── eidolon-evm/        # Wrapper around REVM. Handles custom Inspectors/Tracers.
│   ├── eidolon-forkdb/     # The "Magic". Implements revm::DatabaseRef.
│   │                       # Handles caching, upstream fetching, and Redis commits.
│   ├── eidolon-rpc/        # The Gateway. JSON-RPC server (eth_*, tenderly_*).
│   └── eidolon-types/      # Shared structs (AccountInfo, ForkConfig, Trace).
└── bin/
    └── eidolon-node/       # The executable binary.
```

## 🚀 Getting Started

### Prerequisites
- Rust (Stable)
- Redis (running locally on port 6379)
- An Ethereum Archive Node URL (e.g., Alchemy)

### Installation

```md
# Clone the repository
git clone [https://github.com/PhantomOz/eidolon.git](https://github.com/PhantomOz/eidolon.git)
cd eidolon

# Build the release binary
cargo build --release

```

### Running a Virtual Fork

```md
# Start the server
export UPSTREAM_RPC="[https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY](https://eth-mainnet.g.alchemy.com/v2/YOUR_KEY)"
./target/release/eidolon-node
```

Then, connect your Metamask or Hardhat config to: `http://localhost:3000/fork/{fork_id}`

## 🔮 Roadmap
- [ ] Phase 1: The Skeleton: Basic revm loop with in-memory storage.
- [ ] Phase 2: The Gateway: jsonrpsee server handling eth_call and eth_sendRawTransaction.
- [ ] Phase 3: The ForkDB: Implementation of the "Lazy Loading" database trait.
- [ ] Phase 4: Persistence: Redis integration for saving session state.
- [ ] Phase 5: Time Travel: Implementing eidolon_increaseTime.

## 🤝 Contributing
Eidolon is open-source software. We welcome PRs that make the phantom more tangible.
