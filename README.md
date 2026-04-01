<div align="center">

# 👻 Eidolon

**Virtual Testnet Engine — Fork any EVM chain, simulate transactions, get state diffs.**

[![CI](https://github.com/PhantomOz/eidolon/actions/workflows/ci.yml/badge.svg)](https://github.com/PhantomOz/eidolon/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

[Quickstart](#-quickstart) · [API Reference](#-api-reference) · [Dashboard](#-dashboard) · [Deploy](#-deploy)

</div>

---

## What is Eidolon?

Eidolon is a **multi-tenant virtual testnet engine** for EVM chains. Fork Ethereum, Polygon, Arbitrum, or any EVM chain — then simulate transactions, inspect state diffs, and manage isolated environments through a REST API or web dashboard.

**Think Tenderly/Anvil, but designed as a SaaS from day one.**

### Key Features

| Feature | Description |
|---------|-------------|
| 🔱 **Multi-Fork Management** | Create isolated EVM forks via REST API |
| 🔬 **Transaction Simulation** | Simulate txs without committing state — returns gas, logs, state diffs |
| 📦 **Bundle Simulation** | Simulate multiple txs in sequence (MEV, DeFi workflows) |
| 🧬 **ABI-Decoded Traces** | Auto-decode function calls (ERC20, Uniswap, etc.) |
| 📸 **Fork Snapshots** | Save and restore fork state at any point |
| 🔐 **API Key Auth** | Generate keys with per-key rate limiting |
| 📊 **Usage Metering** | Track requests per key for billing |
| 🖥️ **Web Dashboard** | Manage forks, simulate txs, view traces in the browser |
| 💾 **State Persistence** | Periodic Redis snapshots for crash recovery |

---

## 🚀 Quickstart

### From Source

```bash
git clone https://github.com/PhantomOz/eidolon.git
cd eidolon

# Run in single-fork mode (backward compatible with Anvil)
cargo run -- --rpc-url https://eth.llamarpc.com

# Run in SaaS mode (create forks via API)
cargo run

# Run with auth enabled
cargo run -- --auth
```

### With Docker

```bash
docker build -t eidolon .
docker run -p 8545:8545 eidolon eidolon-node --rpc-url https://eth.llamarpc.com
```

### With Docker Compose (multi-chain)

```bash
docker compose up -d
# Mainnet: localhost:3001
# Polygon: localhost:3002
# Optimism: localhost:3003
# Arbitrum: localhost:3004
# Base:     localhost:3005
```

---

## 📡 API Reference

### Fork Management

```bash
# Create a fork
curl -X POST http://localhost:8545/api/forks \
  -H 'Content-Type: application/json' \
  -d '{"rpc_url":"https://eth.llamarpc.com","chain_id":1}'

# List forks
curl http://localhost:8545/api/forks

# Delete a fork
curl -X DELETE http://localhost:8545/api/forks/{fork_id}

# Snapshot a fork
curl -X POST http://localhost:8545/api/forks/{fork_id}/snapshot

# Restore a snapshot
curl -X POST http://localhost:8545/api/forks/{fork_id}/restore/{snap_id}
```

### JSON-RPC (per fork)

```bash
# Standard eth_* methods
curl -X POST http://localhost:8545/rpc/{fork_id} \
  -H 'Content-Type: application/json' \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Simulate a transaction (returns state diffs)
curl -X POST http://localhost:8545/rpc/{fork_id} \
  -d '{"jsonrpc":"2.0","method":"eidolon_simulateTransaction","params":[{
    "from":"0xSender","to":"0xContract","data":"0xa9059cbb..."
  }],"id":1}'

# Simulate a bundle (sequential txs)
curl -X POST http://localhost:8545/rpc/{fork_id} \
  -d '{"jsonrpc":"2.0","method":"eidolon_simulateBundle","params":[[
    {"from":"0x...","to":"0xToken","data":"0x095ea7b3..."},
    {"from":"0x...","to":"0xRouter","data":"0x38ed1739..."}
  ]],"id":1}'
```

### API Keys

```bash
# Create a key
curl -X POST http://localhost:8545/api/keys \
  -H 'Content-Type: application/json' \
  -d '{"name":"my-app","rate_limit":100}'

# List keys
curl http://localhost:8545/api/keys

# Usage stats
curl http://localhost:8545/api/usage
```

### Supported RPC Methods

<details>
<summary>40+ methods</summary>

**Standard:** `eth_chainId`, `eth_blockNumber`, `eth_getBalance`, `eth_getCode`, `eth_getStorageAt`, `eth_getTransactionCount`, `eth_call`, `eth_estimateGas`, `eth_sendTransaction`, `eth_sendRawTransaction`, `eth_getTransactionByHash`, `eth_getTransactionReceipt`, `eth_getBlockByNumber`, `eth_getBlockByHash`, `eth_getLogs`, `eth_gasPrice`, `eth_maxPriorityFeePerGas`, `eth_feeHistory`, `eth_accounts`, `eth_mining`, `eth_syncing`, `net_version`, `net_listening`, `web3_clientVersion`

**Debug:** `debug_traceTransaction`, `debug_traceCall`

**Cheatcodes:** `evm_mine`, `evm_snapshot`, `evm_revert`, `evm_setAutomine`, `evm_setBlockGasLimit`, `evm_setNextBlockTimestamp`, `evm_increaseTime`

**Eidolon:** `eidolon_setBalance`, `eidolon_setCode`, `eidolon_setNonce`, `eidolon_setStorageAt`, `eidolon_impersonateAccount`, `eidolon_stopImpersonatingAccount`, `eidolon_reset`, `eidolon_simulateTransaction`, `eidolon_simulateBundle`

**Anvil-compatible:** `anvil_setBalance`, `anvil_setCode`, `anvil_setNonce`, `anvil_setStorageAt`, `anvil_impersonateAccount`, `anvil_stopImpersonatingAccount`, `anvil_mine`

</details>

---

## 🖥️ Dashboard

```bash
cd dashboard
npm install
npm run dev
# Open http://localhost:5173
```

The dashboard connects to `http://localhost:8545` and provides:
- **Overview** — active forks, API keys, request counts
- **Forks** — create, delete, snapshot, restore
- **Simulate** — run transactions with decoded results and state diffs
- **API Keys** — manage keys and rate limits

---

## 🚢 Deploy

### Fly.io

```bash
fly launch --copy-config
fly deploy
```

### Railway

[![Deploy on Railway](https://railway.app/button.svg)](https://railway.app/template)

Set environment variables: `PORT=8545`, `AUTH_ENABLED=true`

---

## 🏗️ Architecture

```
eidolon/
├── bin/eidolon-node/    # CLI binary
├── crates/
│   ├── eidolon-evm/     # EVM executor (wraps revm)
│   ├── eidolon-forkdb/  # Lazy-loading fork database
│   ├── eidolon-rpc/     # JSON-RPC server (40+ methods)
│   ├── eidolon-core/    # Axum server, fork manager, auth
│   └── eidolon-types/   # Shared types
└── dashboard/           # Web UI (Vite SPA)
```

#### Core Design Principles

- **Lazy Forking** — state loaded on-demand from upstream RPCs
- **Ephemeral State** — each fork is isolated, snapshottable, and disposable
- **Multi-Tenant** — API keys, rate limiting, usage metering built in
- **Spec Compliant** — deterministic tx hashing, block history tracking

---

## License

MIT
