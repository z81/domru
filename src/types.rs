use serde::{Deserialize, Serialize};

// ─── Auth ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCredentials {
    #[serde(default)]
    pub operator_id: Option<i64>,
    #[serde(default)]
    pub token_type: Option<String>,
    pub access_token: String,
    #[serde(default)]
    pub expires_in: Option<i64>,
    pub refresh_token: String,
    #[serde(default)]
    pub refresh_expires_in: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginInfo {
    pub operator_id: i64,
    pub login: String,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub profile_id: Option<String>,
    #[serde(default)]
    pub confirm1: Option<String>,
    #[serde(default)]
    pub confirm2: Option<String>,
    #[serde(default)]
    pub subscriber_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthResponse {
    pub code: i64,
    pub data: serde_json::Value,
    #[serde(default)]
    pub headers: std::collections::HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContractAddress {
    pub operator_id: i64,
    pub subscriber_id: i64,
    pub account_id: String,
    pub place_id: i64,
    pub address: String,
    #[serde(default)]
    pub profile_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum LoginResult {
    NeedsContract {
        #[serde(rename = "needsContract")]
        needs_contract: bool,
        contracts: Vec<ContractAddress>,
    },
    Ready {
        #[serde(rename = "needsContract")]
        needs_contract: bool,
        data: LoginInfo,
    },
}

// ─── Places ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriberPlace {
    pub id: i64,
    pub subscriber_state: String,
    pub subscriber_type: String,
    #[serde(default)]
    pub provider: Option<String>,
    pub place: Place,
    pub subscriber: Subscriber,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Place {
    pub id: i64,
    pub operator_id: i64,
    pub address: Address,
    #[serde(default)]
    pub location: Option<Location>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Address {
    pub city: String,
    pub visible_address: String,
    #[serde(default)]
    pub apartment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscriber {
    pub id: i64,
    #[serde(default)]
    pub account_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub nick_name: Option<String>,
}

// ─── Access Controls (Intercoms) ─────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccessDevice {
    pub id: i64,
    pub operator_id: i64,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: String,
    pub open_method: String,
    pub entrances: Vec<Entrance>,
    pub allow_open: bool,
    pub allow_video: bool,
    pub allow_slideshow: bool,
    pub allow_call_mobile: bool,
    #[serde(default)]
    pub preview_available: Option<bool>,
    #[serde(default)]
    pub video_download_available: Option<bool>,
    #[serde(default)]
    pub quota: Option<i64>,
    #[serde(default)]
    pub time_zone: Option<i64>,
    #[serde(default)]
    pub external_camera_id: Option<String>,
    #[serde(default)]
    pub external_device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entrance {
    pub id: i64,
    pub name: String,
    pub allow_open: bool,
    pub allow_video: bool,
    pub allow_slideshow: bool,
    pub allow_call_mobile: bool,
    #[serde(default)]
    pub preview_available: Option<bool>,
    #[serde(default)]
    pub video_download_available: Option<bool>,
    #[serde(default)]
    pub quota: Option<i64>,
    #[serde(default)]
    pub time_zone: Option<i64>,
    #[serde(default)]
    pub external_camera_id: Option<String>,
    #[serde(default)]
    pub external_device_id: Option<String>,
}

// ─── Cameras ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonalCamera {
    pub id: i64,
    pub name: String,
    pub state: String,
    #[serde(default)]
    pub external_camera_id: Option<i64>,
    pub status: String,
    pub recording: String,
    pub blocked: bool,
    pub preview_available: bool,
    pub video_download_available: bool,
    #[serde(default)]
    pub mac: Option<String>,
    #[serde(default)]
    pub serial_number: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStream {
    pub data: VideoStreamData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStreamData {
    #[serde(rename = "URL")]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntercomInfo {
    pub access_device: AccessDevice,
    #[serde(default)]
    pub camera_id: Option<String>,
    #[serde(default)]
    pub device_id: Option<String>,
}

// ─── Events / Incoming Calls ─────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSource {
    pub id: i64,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventAction {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventPayload {
    pub message: String,
    pub timestamp: i64,
    pub id: String,
    pub source: EventSource,
    pub place_id: i64,
    pub event_type_name: String,
    pub actions: Vec<EventAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushEvent {
    pub event: PushEventInner,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushEventInner {
    pub payload: EventPayload,
    #[serde(rename = "type")]
    pub type_: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEvent {
    pub uuid: String,
    pub date: String,
    pub event_type: String,
    pub camera_id: i64,
    #[serde(default)]
    pub preview_url: Option<String>,
    #[serde(default)]
    pub detail_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StompEvent {
    #[serde(rename = "type")]
    pub type_: String,
    pub payload: String,
}

// ─── SIP ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SipDevice {
    pub id: String,
    pub login: String,
    pub password: String,
    pub realm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SipDeviceResponse {
    pub data: SipDevice,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SipCredentials {
    pub login: String,
    pub password: String,
    pub realm: String,
}

// ─── App-level types ─────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default = "default_polling_interval")]
    pub call_polling_interval_ms: u64,
    #[serde(default)]
    pub call_webhook_url: String,
}

fn default_polling_interval() -> u64 {
    10_000
}

impl Default for AppConfig {
    fn default() -> Self {
        let webhook_from_env = std::env::var("WEBHOOK_URL").unwrap_or_default();
        Self {
            call_polling_interval_ms: default_polling_interval(),
            call_webhook_url: webhook_from_env,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokensData {
    pub access_token: String,
    pub refresh_token: String,
    #[serde(default)]
    pub operator_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallEvent {
    pub event_type: String,
    pub date: String,
    pub from: String,
    pub sip_message: String,
}

/// Generic wrapper for API responses containing a `data` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
}
