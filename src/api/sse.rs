use std::convert::Infallible;
use std::time::Duration;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::error::AppError;
use crate::state::SharedState;
use crate::types::CallEvent;

const WEBHOOK_TIMEOUT: Duration = Duration::from_secs(5);
const KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(30);

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/events", get(sse_handler))
        .route("/api/call", post(post_call))
}

async fn sse_handler(State(state): State<SharedState>) -> impl IntoResponse {
    let rx = state.call_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(event) => {
            let data = serde_json::to_string(&event).unwrap_or_default();
            let sse_event = Event::default().event("call").data(data);
            Some(Ok::<_, Infallible>(sse_event))
        }
        Err(err) => {
            tracing::warn!("[SSE] broadcast receive error: {}", err);
            None
        }
    });

    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(KEEP_ALIVE_INTERVAL)
            .text("keep-alive"),
    )
}

async fn post_call(
    State(state): State<SharedState>,
    Json(event): Json<CallEvent>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("[CALL] incoming event: {:?}", event);

    // Broadcast to SSE subscribers. Ignore error when there are no receivers.
    let _ = state.call_tx.send(event.clone());

    // Fire webhook asynchronously -- read webhook URL under lock, then release before spawning.
    let webhook_url = {
        let config = state.config.read().await;
        config.call_webhook_url.clone()
    };
    if !webhook_url.is_empty() {
        tokio::spawn(async move {
            fire_webhook(&webhook_url, &event).await;
        });
    }

    Ok(Json(json!({ "ok": true })))
}

async fn fire_webhook(url: &str, event: &CallEvent) {
    let client = reqwest::Client::new();
    let result = tokio::time::timeout(WEBHOOK_TIMEOUT, client.post(url).json(event).send()).await;
    match result {
        Ok(Ok(resp)) => tracing::info!("[WEBHOOK] sent -> {}", resp.status()),
        Ok(Err(e)) => tracing::error!("[WEBHOOK] failed: {}", e),
        Err(_) => tracing::error!("[WEBHOOK] timeout"),
    }
}

/// Background task that listens for ALL call events (including SIP) and fires webhook.
pub fn spawn_webhook_listener(state: SharedState) {
    let mut rx = state.call_tx.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let webhook_url = {
                        let config = state.config.read().await;
                        config.call_webhook_url.clone()
                    };
                    if !webhook_url.is_empty() {
                        fire_webhook(&webhook_url, &event).await;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!("[WEBHOOK] missed {} events", n);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    tracing::info!("[WEBHOOK] channel closed, stopping listener");
                    break;
                }
            }
        }
    });
}
