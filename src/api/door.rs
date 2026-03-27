use axum::extract::State;
use axum::routing::post;
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::state::SharedState;
use crate::types::AccessDevice;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenDoorBody {
    place_id: i64,
    device: AccessDevice,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OpenEntranceBody {
    place_id: i64,
    access_control_id: i64,
    entrance_id: i64,
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/open-door", post(open_door))
        .route("/api/open-entrance", post(open_entrance))
        .route("/api/answer-and-open", post(answer_and_open))
}

async fn open_door(
    State(state): State<SharedState>,
    Json(body): Json<OpenDoorBody>,
) -> Result<Json<Value>, AppError> {
    tracing::info!(
        "[DOOR] open door place_id={} device_id={}",
        body.place_id,
        body.device.id,
    );
    let mut client = state.client.write().await;
    client.open_door(body.place_id, &body.device).await?;
    Ok(Json(json!({ "ok": true })))
}

/// Answer the current SIP call (200 OK) + open door via HTTP API.
/// This stops the call on other devices (phone app).
async fn answer_and_open(
    State(state): State<SharedState>,
    Json(body): Json<OpenDoorBody>,
) -> Result<Json<Value>, AppError> {
    tracing::info!(
        "[DOOR] answer-and-open place_id={} device_id={}",
        body.place_id,
        body.device.id,
    );

    // Check if there's an active call
    let has_invite = state.last_invite.read().await.is_some();
    if !has_invite {
        return Err(AppError::Api {
            status: 409,
            message: "No active call to answer".to_string(),
        });
    }

    // Tell SIP client to answer (200 OK)
    if state.sip_answer_tx.try_send(()).is_err() {
        tracing::warn!("[DOOR] SIP answer channel full or closed");
    }

    // Also open door via HTTP API
    let mut client = state.client.write().await;
    client.open_door(body.place_id, &body.device).await?;

    Ok(Json(json!({ "ok": true, "answered": true })))
}

async fn open_entrance(
    State(state): State<SharedState>,
    Json(body): Json<OpenEntranceBody>,
) -> Result<Json<Value>, AppError> {
    tracing::info!(
        "[DOOR] open entrance place_id={} access_control_id={} entrance_id={}",
        body.place_id,
        body.access_control_id,
        body.entrance_id,
    );
    let mut client = state.client.write().await;
    client
        .open_entrance(body.place_id, body.access_control_id, body.entrance_id)
        .await?;
    Ok(Json(json!({ "ok": true })))
}
