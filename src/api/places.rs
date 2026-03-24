use axum::extract::{Path, State};
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::error::AppError;
use crate::state::SharedState;

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/places", get(get_places))
        .route(
            "/api/places/{place_id}/accesscontrols",
            get(get_access_controls),
        )
        .route("/api/places/{place_id}/cameras", get(get_cameras))
}

async fn get_places(State(state): State<SharedState>) -> Result<Json<Value>, AppError> {
    tracing::info!("[PLACES] get places");
    let mut client = state.client.write().await;
    let places = client.get_places().await?;
    Ok(Json(json!({ "data": places })))
}

async fn get_access_controls(
    State(state): State<SharedState>,
    Path(place_id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("[PLACES] get access controls for place_id={}", place_id);
    let mut client = state.client.write().await;
    let devices = client.get_access_controls(place_id).await?;
    Ok(Json(json!({ "data": devices })))
}

async fn get_cameras(
    State(state): State<SharedState>,
    Path(place_id): Path<i64>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("[PLACES] get cameras for place_id={}", place_id);
    let mut client = state.client.write().await;
    let cameras = client.get_personal_cameras(place_id).await?;
    Ok(Json(json!({ "data": cameras })))
}
