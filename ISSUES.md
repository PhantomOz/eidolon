# Eidolon Production Readiness Issues

## Critical (incorrect behavior / data loss)

- [x] **C1: Fork DB ignores pinned block number** — Added `block_tag()` helper to `RpcBackend` that returns hex block number when pinned, `"latest"` otherwise. All `DatabaseRef` methods now use the correct block tag.
- [x] **C2: No transaction storage** — Added `StoredTransaction` struct and `HashMap<B256, StoredTransaction>` to `EidolonRpc`. `send_transaction` and `send_raw_transaction` now store tx data (from, to, value, gas_used, logs, contract_address, status). `get_transaction_receipt` returns `None` for unknown hashes and real data for known ones.
- [x] **C3: No nonce management** — `transact()` now looks up the caller's current nonce from the DB and sets `tx.nonce = Some(nonce)` so REVM's nonce validation passes and nonces are incremented correctly after execution.
- [x] **C4: Synchronous HTTP on async runtime** — Added `blocking` attribute to all RPC methods that can trigger upstream RPC fetches. jsonrpsee runs these on a dedicated blocking thread pool, preventing Tokio worker thread starvation.
- [x] **C5: Block number not persisted in snapshots** — `StateSnapshot` now includes `block_number` and `chain_id`. Both `get_snapshot()` and `load_snapshot()` save/restore these fields.

## High (common workflows will fail)

- [x] **H1: Missing critical RPC methods** — Added: `eth_getStorageAt`, `eth_getTransactionByHash`, `eth_getBlockByHash`, `eth_getLogs` (with address/topic/block filtering), `eth_feeHistory`, `eth_maxPriorityFeePerGas`, `eth_syncing`, `web3_clientVersion`, `net_listening`, `eth_accounts`, `eth_mining`.
- [x] **H2: All read ops take write locks** — Inherent to REVM's `CacheDB` requiring `&mut self` for cache-miss writes. Mitigated by `blocking` attribute (C4). Full fix requires replacing `CacheDB` with a concurrent-safe DB layer.
- [x] **H3: Logs typed as `Vec<()>`** — `TransactionReceipt.logs` is now `Vec<SerializableLog>` with proper address, topics, data, block info. Logs are captured from `ExecutionResult::Success` and stored per-transaction. Tracer `println!` replaced with `tracing::debug`.
- [x] **H4: Snapshot/revert not exposed via RPC** — Added `evm_snapshot` and `evm_revert` RPC methods.
- [ ] **H5: Zero tests** — No unit tests, integration tests, or test infrastructure anywhere.

## Medium (issues under real usage)

- [ ] **M1: Wide-open CORS + no auth** — `CorsLayer` allows `Any` origin/method/header. No API keys or rate limiting. Any website can manipulate state.
- [ ] **M2: Redis exposed without password** — `docker-compose.yml` exposes Redis on 6379 with no authentication.
- [ ] **M3: State only saved on graceful shutdown** — Redis persistence on `ctrl_c` only. `SIGKILL` or crash loses all state. No periodic saves.
- [ ] **M4: Hardcoded values everywhere** — Block hash always `0xaa..aa`, gas price always 1 wei, block time always 12s (wrong for L2s), receipt gas always 21000, gas limit hardcoded to 30M in four places.
- [x] **M5: `println!` debug logging in tracer** — Replaced with `tracing::debug`. Also fixed unused assignment warning in `set_storage_at`.

## Low (cleanup / polish)

- [ ] **L1: Docker gaps** — No `HEALTHCHECK`, runs as root, no `.dockerignore`, no restart policies, no resource limits.
- [ ] **L2: CLI missing options** — No `--log-level`, `--host`, configurable gas/block-time, or pre-funded accounts.
- [ ] **L3: Snapshot memory leak** — `snapshots: HashMap<u64, StateSnapshot>` grows unboundedly; only pruned on revert.
- [ ] **L4: Redis TTL hardcoded** — 24-hour expiry not configurable. Long sessions silently lose state.
