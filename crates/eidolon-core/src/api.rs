use crate::auth::{AuthManager, CreateKeyRequest};
use crate::fork_manager::{ForkCreateRequest, ForkManager};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;

/// Shared application state.
pub struct AppState {
    pub fork_manager: ForkManager,
    pub auth: AuthManager,
    pub base_url: String,
}

/// Custom RPC params wrapper for jsonrpsee's RpcModule::call.
pub struct RawParams(pub Option<Box<serde_json::value::RawValue>>);

impl jsonrpsee::core::traits::ToRpcParams for RawParams {
    fn to_rpc_params(self) -> Result<Option<Box<serde_json::value::RawValue>>, serde_json::Error> {
        Ok(self.0)
    }
}

// --- Health ---

pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "version": "0.2.0" }))
}

// --- API Key Management ---

/// POST /api/keys — Create a new API key.
pub async fn create_key(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateKeyRequest>,
) -> impl IntoResponse {
    let key = state.auth.create_key(req.name, req.rate_limit);
    (StatusCode::CREATED, Json(json!(key)))
}

/// GET /api/keys — List all API keys.
pub async fn list_keys(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let keys = state.auth.list_keys();
    Json(json!({ "keys": keys, "count": keys.len() }))
}

/// DELETE /api/keys/:key — Delete an API key.
pub async fn delete_key_handler(
    State(state): State<Arc<AppState>>,
    Path(key): Path<String>,
) -> impl IntoResponse {
    if state.auth.delete_key(&key) {
        (StatusCode::OK, Json(json!({ "deleted": true }))).into_response()
    } else {
        (StatusCode::NOT_FOUND, Json(json!({ "error": "Key not found" }))).into_response()
    }
}

// --- Usage Metering ---

/// GET /api/usage — Usage stats per API key.
pub async fn usage_stats(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let keys = state.auth.list_keys();
    let usage: Vec<serde_json::Value> = keys
        .iter()
        .map(|k| {
            json!({
                "key": format!("{}...{}", &k.key[..8], &k.key[k.key.len()-4..]),
                "name": k.name,
                "request_count": k.request_count,
                "rate_limit": k.rate_limit,
            })
        })
        .collect();
    let total: u64 = keys.iter().map(|k| k.request_count).sum();
    Json(json!({
        "total_requests": total,
        "active_forks": state.fork_manager.fork_count(),
        "keys": usage,
    }))
}

// --- Fork Management REST API ---

/// POST /api/forks — Create a new fork.
pub async fn create_fork(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ForkCreateRequest>,
) -> impl IntoResponse {
    let fork = state.fork_manager.create_fork(req);
    let info = fork.info(&state.base_url);
    (StatusCode::CREATED, Json(json!(info)))
}

/// GET /api/forks — List all forks.
pub async fn list_forks(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let forks = state.fork_manager.list_forks(&state.base_url);
    Json(json!({ "forks": forks, "count": forks.len() }))
}

/// GET /api/forks/:id — Get fork details.
pub async fn get_fork(
    State(state): State<Arc<AppState>>,
    Path(fork_id): Path<String>,
) -> impl IntoResponse {
    match state.fork_manager.get_fork(&fork_id) {
        Some(fork) => {
            let info = fork.info(&state.base_url);
            (StatusCode::OK, Json(json!(info))).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Fork not found" })),
        )
            .into_response(),
    }
}

/// DELETE /api/forks/:id — Delete a fork.
pub async fn delete_fork(
    State(state): State<Arc<AppState>>,
    Path(fork_id): Path<String>,
) -> impl IntoResponse {
    if state.fork_manager.delete_fork(&fork_id) {
        (StatusCode::OK, Json(json!({ "deleted": true }))).into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Fork not found" })),
        )
            .into_response()
    }
}

// --- Fork Snapshots ---

/// POST /api/forks/:id/snapshot — Create a snapshot.
pub async fn snapshot_fork(
    State(state): State<Arc<AppState>>,
    Path(fork_id): Path<String>,
) -> impl IntoResponse {
    match state.fork_manager.snapshot_fork(&fork_id) {
        Some(snap_id) => {
            (StatusCode::CREATED, Json(json!({ "snapshot_id": snap_id, "fork_id": fork_id }))).into_response()
        }
        None => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": "Fork not found" }))).into_response()
        }
    }
}

/// POST /api/forks/:id/restore/:snap_id — Restore to a snapshot.
pub async fn restore_fork(
    State(state): State<Arc<AppState>>,
    Path((fork_id, snap_id)): Path<(String, u64)>,
) -> impl IntoResponse {
    match state.fork_manager.restore_fork(&fork_id, snap_id) {
        Some(true) => {
            (StatusCode::OK, Json(json!({ "restored": true, "fork_id": fork_id, "snapshot_id": snap_id }))).into_response()
        }
        Some(false) => {
            (StatusCode::BAD_REQUEST, Json(json!({ "error": "Invalid or expired snapshot ID" }))).into_response()
        }
        None => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": "Fork not found" }))).into_response()
        }
    }
}

// --- JSON-RPC Router ---

/// POST /rpc/:fork_id — Route JSON-RPC requests to the correct fork.
pub async fn handle_rpc(
    State(state): State<Arc<AppState>>,
    Path(fork_id): Path<String>,
    body: String,
) -> impl IntoResponse {
    // 1. Look up fork
    let fork = match state.fork_manager.get_fork(&fork_id) {
        Some(f) => f,
        None => {
            let error_response = json!({
                "jsonrpc": "2.0",
                "error": { "code": -32001, "message": format!("Fork '{}' not found", fork_id) },
                "id": null
            });
            return (StatusCode::NOT_FOUND, Json(error_response)).into_response();
        }
    };

    // 2. Parse JSON-RPC request
    let request: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            let error_response = json!({
                "jsonrpc": "2.0",
                "error": { "code": -32700, "message": "Parse error" },
                "id": null
            });
            return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
        }
    };

    let method = match request.get("method").and_then(|m| m.as_str()) {
        Some(m) => m.to_string(),
        None => {
            let id = request.get("id").cloned().unwrap_or(serde_json::Value::Null);
            let error_response = json!({
                "jsonrpc": "2.0",
                "error": { "code": -32600, "message": "Missing method" },
                "id": id
            });
            return (StatusCode::BAD_REQUEST, Json(error_response)).into_response();
        }
    };

    let id = request
        .get("id")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    // 3. Extract params as RawValue for RpcModule::call
    let raw_params = request
        .get("params")
        .map(|p| serde_json::value::to_raw_value(p).unwrap());

    // 4. Dispatch via RpcModule::call
    let rpc_module = &fork.rpc_module;
    let params = RawParams(raw_params);

    match rpc_module.call::<RawParams, serde_json::Value>(&method, params).await {
        Ok(result) => {
            let response = json!({
                "jsonrpc": "2.0",
                "result": result,
                "id": id
            });
            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            let error_response = json!({
                "jsonrpc": "2.0",
                "error": { "code": -32000, "message": e.to_string() },
                "id": id
            });
            (StatusCode::OK, Json(error_response)).into_response()
        }
    }
}
