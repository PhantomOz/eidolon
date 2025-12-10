use alloy_primitives::{Address, B256, U256};
use anyhow::Result;
use revm::{
    Database, DatabaseRef,
    db::{CacheDB, EmptyDB},
    primitives::{AccountInfo, Bytecode},
};
use serde::Deserialize;
use std::collections::HashMap;
use tracing::info;

/// The configuration for the fork
#[derive(Clone)]
pub struct ForkConfig {
    pub rpc_url: String,
    pub block_number: Option<u64>,
}
