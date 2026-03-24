use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::state::{self, SharedState};
use crate::types::ContractAddress;

#[derive(Deserialize)]
struct LoginBody {
    phone: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SelectContractBody {
    phone: String,
    contract: ContractAddress,
}

#[derive(Deserialize)]
struct ConfirmBody {
    phone: String,
    code: String,
    contract: Option<ContractAddress>,
}

pub fn router() -> Router<SharedState> {
    Router::new()
        .route("/api/login", post(login))
        .route("/api/select-contract", post(select_contract))
        .route("/api/confirm", post(confirm))
        .route("/api/session", get(session))
        .route("/api/refresh", post(refresh))
        .route("/api/logout", post(logout))
}

async fn login(
    State(state): State<SharedState>,
    Json(body): Json<LoginBody>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("[AUTH] login request for phone={}", body.phone);
    let mut client = state.client.write().await;
    let result = client.request_login(&body.phone).await?;
    Ok(Json(json!({ "ok": true, "data": result })))
}

async fn select_contract(
    State(state): State<SharedState>,
    Json(body): Json<SelectContractBody>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("[AUTH] select-contract for phone={}", body.phone);
    let mut client = state.client.write().await;
    client.select_contract(&body.phone, &body.contract).await?;
    Ok(Json(json!({ "ok": true })))
}

async fn confirm(
    State(state): State<SharedState>,
    Json(body): Json<ConfirmBody>,
) -> Result<Json<Value>, AppError> {
    tracing::info!("[AUTH] confirm for phone={}", body.phone);
    let mut client = state.client.write().await;
    let creds = client
        .confirm_login(&body.phone, &body.code, body.contract.as_ref())
        .await?;
    let tokens = client.get_tokens();
    state::save_tokens(&state::tokens_path(), &tokens);
    Ok(Json(json!({ "ok": true, "data": { "operatorId": creds.operator_id } })))
}

async fn session(State(state): State<SharedState>) -> Json<Value> {
    let client = state.client.read().await;
    let authenticated = client.is_authenticated();
    Json(json!({ "authenticated": authenticated }))
}

async fn refresh(State(state): State<SharedState>) -> Result<Json<Value>, AppError> {
    tracing::info!("[AUTH] refresh session");
    let mut client = state.client.write().await;
    let creds = client.refresh_session().await?;
    let tokens = client.get_tokens();
    state::save_tokens(&state::tokens_path(), &tokens);
    Ok(Json(json!({ "ok": true, "data": { "operatorId": creds.operator_id } })))
}

async fn logout(State(state): State<SharedState>) -> Result<Json<Value>, AppError> {
    tracing::info!("[AUTH] logout");
    let mut client = state.client.write().await;
    client.set_tokens(String::new(), String::new(), None);
    let empty_tokens = client.get_tokens();
    state::save_tokens(&state::tokens_path(), &empty_tokens);
    Ok(Json(json!({ "ok": true })))
}
