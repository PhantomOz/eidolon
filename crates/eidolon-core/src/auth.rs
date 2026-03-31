use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

/// An API key with metadata.
#[derive(Debug, Clone, Serialize)]
pub struct ApiKey {
    pub key: String,
    pub name: String,
    pub created_at: u64,
    pub request_count: u64,
    pub rate_limit: u64, // requests per minute, 0 = unlimited
}

/// Request to create a new API key.
#[derive(Deserialize, Debug)]
pub struct CreateKeyRequest {
    pub name: String,
    pub rate_limit: Option<u64>,
}

/// API key response (shown once on creation).
#[derive(Serialize, Debug)]
pub struct CreateKeyResponse {
    pub key: String,
    pub name: String,
    pub rate_limit: u64,
}

/// Rate limiter entry per API key.
struct RateEntry {
    count: u64,
    window_start: Instant,
}

/// Manages API keys, validation, and rate limiting.
pub struct AuthManager {
    keys: RwLock<HashMap<String, ApiKey>>,
    rate_entries: RwLock<HashMap<String, RateEntry>>,
    /// If true, require API keys for all requests. If false, open access.
    pub enabled: bool,
}

impl AuthManager {
    pub fn new(enabled: bool) -> Self {
        Self {
            keys: RwLock::new(HashMap::new()),
            rate_entries: RwLock::new(HashMap::new()),
            enabled,
        }
    }

    /// Generate a new API key.
    pub fn create_key(&self, name: String, rate_limit: Option<u64>) -> CreateKeyResponse {
        let key = format!("eid_{}", uuid::Uuid::new_v4().to_string().replace("-", ""));
        let rate = rate_limit.unwrap_or(0);

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let api_key = ApiKey {
            key: key.clone(),
            name: name.clone(),
            created_at: now,
            request_count: 0,
            rate_limit: rate,
        };

        self.keys.write().insert(key.clone(), api_key);

        CreateKeyResponse {
            key,
            name,
            rate_limit: rate,
        }
    }

    /// Validate an API key. Returns the key name if valid.
    pub fn validate_key(&self, key: &str) -> Option<String> {
        let mut keys = self.keys.write();
        if let Some(api_key) = keys.get_mut(key) {
            api_key.request_count += 1;
            Some(api_key.name.clone())
        } else {
            None
        }
    }

    /// Check rate limit. Returns true if allowed.
    pub fn check_rate_limit(&self, key: &str) -> bool {
        let keys = self.keys.read();
        let rate_limit = match keys.get(key) {
            Some(k) if k.rate_limit > 0 => k.rate_limit,
            _ => return true, // no limit
        };
        drop(keys);

        let mut entries = self.rate_entries.write();
        let entry = entries.entry(key.to_string()).or_insert(RateEntry {
            count: 0,
            window_start: Instant::now(),
        });

        // Reset window every 60 seconds
        if entry.window_start.elapsed().as_secs() >= 60 {
            entry.count = 0;
            entry.window_start = Instant::now();
        }

        if entry.count >= rate_limit {
            return false;
        }

        entry.count += 1;
        true
    }

    /// Delete an API key.
    pub fn delete_key(&self, key: &str) -> bool {
        self.keys.write().remove(key).is_some()
    }

    /// List all API keys (redacted).
    pub fn list_keys(&self) -> Vec<ApiKey> {
        self.keys.read().values().cloned().collect()
    }
}

/// Axum middleware that validates API keys from X-API-Key header.
pub async fn auth_middleware(
    headers: HeaderMap,
    state: Arc<crate::api::AppState>,
    request: Request,
    next: Next,
) -> Response {
    // Skip auth if disabled
    if !state.auth.enabled {
        return next.run(request).await;
    }

    // Skip auth for health endpoint
    let path = request.uri().path().to_string();
    if path == "/health" {
        return next.run(request).await;
    }

    // Skip auth for key management (bootstrap problem)
    if path == "/api/keys" && request.method() == axum::http::Method::POST {
        return next.run(request).await;
    }

    let api_key = match headers.get("x-api-key").and_then(|v| v.to_str().ok()) {
        Some(key) => key.to_string(),
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "Missing X-API-Key header" })),
            )
                .into_response();
        }
    };

    // Validate key
    if state.auth.validate_key(&api_key).is_none() {
        return (
            StatusCode::UNAUTHORIZED,
            Json(json!({ "error": "Invalid API key" })),
        )
            .into_response();
    }

    // Check rate limit
    if !state.auth.check_rate_limit(&api_key) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({ "error": "Rate limit exceeded" })),
        )
            .into_response();
    }

    next.run(request).await
}
