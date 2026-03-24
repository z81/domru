use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{Path, Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Redirect};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::state::{self, SharedState};
use crate::types::SipCredentials;

const DEFAULT_SNAPSHOT_WIDTH: u32 = 640;
const DEFAULT_SNAPSHOT_HEIGHT: u32 = 360;
const ONE_DAY_SECS: u64 = 86_400;

#[derive(Deserialize)]
struct SnapshotParams {
    #[serde(rename = "type", default)]
    device_type: Option<String>,
    w: Option<u32>,
    h: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsParams {
    date_from: Option<String>,
    date_to: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SipDeviceBody {
    place_id: i64,
    access_control_id: i64,
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route(
            "/api/snapshot/{place_id}/{device_id}",
            get(proxy_snapshot),
        )
        .route("/api/stream", get(redirect_stream_default))
        .route("/api/stream/{camera_id}", get(get_stream))
        .route("/api/stream/{camera_id}/redirect", get(redirect_stream))
        .route("/api/archive/{camera_id}", get(get_archive))
        .route("/api/events/{camera_id}", get(get_events))
        .route("/api/sip-device", post(create_sip_device))
}

async fn proxy_snapshot(
    State(state): State<SharedState>,
    Path((place_id, device_id)): Path<(i64, i64)>,
    Query(params): Query<SnapshotParams>,
) -> Result<impl IntoResponse, AppError> {
    let device_type = params.device_type.as_deref().unwrap_or("SIP");
    let width = params.w.unwrap_or(DEFAULT_SNAPSHOT_WIDTH);
    let height = params.h.unwrap_or(DEFAULT_SNAPSHOT_HEIGHT);

    tracing::info!(
        "[MEDIA] snapshot place_id={} device_id={} type={} {}x{}",
        place_id,
        device_id,
        device_type,
        width,
        height,
    );

    let mut client = state.client.write().await;

    // Retry once on 5xx (upstream API is flaky)
    let data = match client
        .proxy_snapshot(place_id, device_id, device_type, width, height)
        .await
    {
        Ok(d) => d,
        Err(_) => {
            tracing::warn!("[MEDIA] snapshot failed, retrying once...");
            client
                .proxy_snapshot(place_id, device_id, device_type, width, height)
                .await?
        }
    };

    let headers = [
        (header::CONTENT_TYPE, "image/jpeg"),
        (header::CACHE_CONTROL, "no-cache"),
    ];
    Ok((headers, data))
}

async fn get_stream(
    State(state): State<SharedState>,
    Path(camera_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("[MEDIA] stream camera_id={}", camera_id);
    let mut client = state.client.write().await;
    client.refresh_video_session(&camera_id).await?;
    let stream = client.get_video_stream(&camera_id).await?;
    Ok(Json(json!({ "url": stream.data.url })))
}

async fn redirect_stream_default(
    State(state): State<SharedState>,
) -> Result<Redirect, AppError> {
    let mut client = state.client.write().await;
    let places = client.get_places().await?;
    let place = places.first().ok_or_else(|| AppError::NotFound("No places".into()))?;
    let devices = client.get_access_controls(place.place.id).await?;
    let camera_id = devices
        .iter()
        .find_map(|d| d.external_camera_id.as_deref())
        .ok_or_else(|| AppError::NotFound("No camera found".into()))?
        .to_string();

    tracing::info!("[MEDIA] stream redirect default -> camera_id={}", camera_id);
    client.refresh_video_session(&camera_id).await?;
    let stream = client.get_video_stream(&camera_id).await?;
    Ok(Redirect::temporary(&stream.data.url))
}

async fn redirect_stream(
    State(state): State<SharedState>,
    Path(camera_id): Path<String>,
) -> Result<Redirect, AppError> {
    tracing::info!("[MEDIA] stream redirect camera_id={}", camera_id);
    let mut client = state.client.write().await;
    client.refresh_video_session(&camera_id).await?;
    let stream = client.get_video_stream(&camera_id).await?;
    Ok(Redirect::temporary(&stream.data.url))
}

#[derive(Deserialize)]
struct ArchiveParams {
    ts: i64,
    tz: Option<i64>,
}

async fn get_archive(
    State(state): State<SharedState>,
    Path(camera_id): Path<String>,
    Query(params): Query<ArchiveParams>,
) -> Result<Json<Value>, AppError> {
    let tz = params.tz.unwrap_or(10800); // default Moscow +3h in seconds
    tracing::info!(
        "[MEDIA] archive camera_id={} ts={} tz={}",
        camera_id,
        params.ts,
        tz,
    );
    let mut client = state.client.write().await;
    client.refresh_video_session(&camera_id).await?;
    let url = client
        .get_archive_stream(&camera_id, params.ts, tz)
        .await?;
    Ok(Json(json!({ "url": url })))
}

async fn get_events(
    State(state): State<SharedState>,
    Path(camera_id): Path<String>,
    Query(params): Query<EventsParams>,
) -> Result<Json<Value>, AppError> {
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis();

    let date_from = params.date_from.unwrap_or_else(|| {
        let millis = now_millis.saturating_sub(u128::from(ONE_DAY_SECS) * 1000);
        format_iso_utc(millis)
    });
    let date_to = params
        .date_to
        .unwrap_or_else(|| format_iso_utc(now_millis));

    tracing::info!(
        "[MEDIA] events camera_id={} from={} to={}",
        camera_id,
        date_from,
        date_to,
    );

    let mut client = state.client.write().await;
    let data = client
        .get_camera_events(&camera_id, &date_from, &date_to)
        .await?;
    Ok(Json(data))
}

async fn create_sip_device(
    State(state): State<SharedState>,
    Json(body): Json<SipDeviceBody>,
) -> Result<Json<Value>, AppError> {
    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis();
    let installation_id = format!("rust-domofon-{now_millis}");

    tracing::info!(
        "[MEDIA] create sip device place_id={} access_control_id={}",
        body.place_id,
        body.access_control_id,
    );

    let mut client = state.client.write().await;
    let sip_device = client
        .create_sip_device(body.place_id, body.access_control_id, &installation_id)
        .await?;

    let creds = SipCredentials {
        login: sip_device.login.clone(),
        password: sip_device.password.clone(),
        realm: sip_device.realm.clone(),
    };
    state::save_sip_credentials(&state::sip_credentials_path(), &creds);

    Ok(Json(json!({ "ok": true, "data": sip_device })))
}

/// Format milliseconds since UNIX epoch as an ISO 8601 UTC string (no fractional seconds).
fn format_iso_utc(epoch_millis: u128) -> String {
    let secs = (epoch_millis / 1000) as i64;
    let days = secs / 86_400;
    let day_secs = secs % 86_400;
    let hours = day_secs / 3600;
    let mins = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    // Convert days since 1970-01-01 to year-month-day using a standard civil calendar algorithm.
    let (y, m, d) = civil_from_days(days);

    format!("{y:04}-{m:02}-{d:02}T{hours:02}:{mins:02}:{s:02}Z")
}

/// Converts days since 1970-01-01 to (year, month, day).
/// Algorithm from Howard Hinnant's `chrono`-compatible date library.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
