use axum::extract::State;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::AppError;
use crate::state::{self, SharedState};

pub fn router() -> Router<SharedState> {
    Router::new().route("/api/config", get(get_config).post(update_config))
}

async fn get_config(State(state): State<SharedState>) -> Json<Value> {
    // Re-read from disk to pick up any external changes, matching TS behaviour.
    let fresh = state::load_config(&state::config_path());
    let mut config = state.config.write().await;
    *config = fresh;
    Json(json!({
        "callPollingIntervalMs": config.call_polling_interval_ms,
        "callWebhookUrl": config.call_webhook_url,
    }))
}

async fn update_config(
    State(state): State<SharedState>,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let mut config = state.config.write().await;

    // Merge partial fields from the request body.
    if let Some(interval) = body.get("callPollingIntervalMs").and_then(|v| v.as_u64()) {
        config.call_polling_interval_ms = interval;
    }
    if let Some(url) = body.get("callWebhookUrl").and_then(|v| v.as_str()) {
        config.call_webhook_url = url.to_string();
    }

    state::save_config(&state::config_path(), &config);

    Ok(Json(json!({
        "ok": true,
        "data": {
            "callPollingIntervalMs": config.call_polling_interval_ms,
            "callWebhookUrl": config.call_webhook_url,
        }
    })))
}
