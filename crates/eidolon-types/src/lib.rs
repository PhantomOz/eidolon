use alloy_primitives::{Address, B256, U256, U64, Bytes};
use serde::{Deserialize, Serialize};

/// The configuration for the fork
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ForkConfig {
    pub rpc_url: String,
    pub block_number: Option<u64>,
}

/// A Fake Block to satisfy MetaMask
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MockBlock {
    pub number: U256,
    pub hash: B256,
    pub parent_hash: B256,
    pub nonce: U64,
    pub sha3_uncles: B256,
    pub logs_bloom: Bytes,
    pub transactions_root: B256,
    pub state_root: B256,
    pub receipts_root: B256,
    pub miner: Address,
    pub difficulty: U256,
    pub total_difficulty: U256,
    pub extra_data: Bytes,
    pub size: U256,
    pub gas_limit: U256,
    pub gas_used: U256,
    pub timestamp: U256,
    pub transactions: Vec<B256>,
    pub uncles: Vec<B256>,
}
