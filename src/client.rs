use base64::Engine;
use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

use crate::error::AppError;
use crate::types::{
    AccessDevice, ApiResponse, AuthCredentials, AuthResponse, ContractAddress, LoginInfo,
    LoginResult, PersonalCamera, SipDevice, SipDeviceResponse, SubscriberPlace, TokensData,
    VideoStream,
};

const BASE_URL: &str = "https://myhome.proptech.ru";
const AUTH_SECRET: &str = "789sdgHJs678wertv34712376";
// UA format: {model} | Android {ver} | {operatorCode} | {appVer} | | {operatorId} | {installationId} | {placeId}
const UA_TEMPLATE: &str = "Xiaomi M2012K11AG | Android 14 | erth | 9.3.0 (90300010) | ";

pub struct DomofonClient {
    http: reqwest::Client,
    access_token: Option<String>,
    refresh_token: Option<String>,
    operator_id: Option<i64>,
    place_id: Option<i64>,
}

impl DomofonClient {
    pub fn new() -> Self {
        let http = reqwest::Client::builder()
            .build()
            .expect("failed to build reqwest::Client");

        Self {
            http,
            access_token: None,
            refresh_token: None,
            operator_id: None,
            place_id: None,
        }
    }

    pub fn set_place_id(&mut self, place_id: i64) {
        self.place_id = Some(place_id);
    }

    fn user_agent(&self) -> String {
        let op = self.operator_id.unwrap_or(0);
        let pid = self.place_id.unwrap_or(0);
        format!("{UA_TEMPLATE}| {op} | domofon-api | {pid}")
    }

    pub fn set_tokens(
        &mut self,
        access_token: String,
        refresh_token: String,
        operator_id: Option<i64>,
    ) {
        self.access_token = Some(access_token);
        self.refresh_token = Some(refresh_token);
        self.operator_id = operator_id;
    }

    pub fn get_tokens(&self) -> TokensData {
        TokensData {
            access_token: self.access_token.clone().unwrap_or_default(),
            refresh_token: self.refresh_token.clone().unwrap_or_default(),
            operator_id: self.operator_id,
        }
    }

    pub fn is_authenticated(&self) -> bool {
        self.access_token.is_some()
    }

    fn basic_auth(&self, phone: &str) -> String {
        let raw = format!("{phone}:{AUTH_SECRET}");
        base64::engine::general_purpose::STANDARD.encode(raw.as_bytes())
    }

    // ─── Core request ────────────────────────────────────

    async fn request<T: DeserializeOwned>(
        &mut self,
        method: Method,
        path: &str,
        body: Option<Value>,
        extra_headers: Option<HeaderMap>,
    ) -> Result<T, AppError> {
        let url = format!("{BASE_URL}/{path}");

        let response = self
            .execute_request(method.clone(), &url, body.clone(), extra_headers.clone())
            .await?;

        if response.status() == StatusCode::UNAUTHORIZED && self.refresh_token.is_some() {
            tracing::info!("[API]   401 -> refreshing token...");
            self.refresh_session().await?;

            let retry = self
                .execute_request(method, &url, body, extra_headers)
                .await?;
            let status = retry.status();
            let text = retry.text().await?;

            tracing::info!("[API] <- retry {status}");
            tracing::info!("[API]   response: {}", truncate(&text, 500));

            let parsed: T = serde_json::from_str(&text)?;
            return Ok(parsed);
        }

        let status = response.status();

        if !status.is_success() && status != StatusCode::MULTIPLE_CHOICES {
            let text = response.text().await?;
            tracing::error!("[API]   error body: {}", truncate(&text, 500));
            return Err(AppError::Api {
                status: status.as_u16(),
                message: text,
            });
        }

        let text = response.text().await?;

        if text.is_empty() {
            tracing::info!("[API]   no json body");
            // For empty responses, try to deserialize an empty JSON object.
            // This works for `Value` and similar permissive types.
            let parsed: T = serde_json::from_str("{}")?;
            return Ok(parsed);
        }

        tracing::info!("[API]   response: {}", truncate(&text, 500));
        let parsed: T = serde_json::from_str(&text)?;
        Ok(parsed)
    }

    async fn execute_request(
        &self,
        method: Method,
        url: &str,
        body: Option<Value>,
        extra_headers: Option<HeaderMap>,
    ) -> Result<reqwest::Response, AppError> {
        let mut headers = HeaderMap::new();

        if let Some(extra) = extra_headers {
            headers.extend(extra);
        }

        if let Some(ref token) = self.access_token {
            if !headers.contains_key(AUTHORIZATION) {
                let val = format!("Bearer {token}");
                if let Ok(hv) = HeaderValue::from_str(&val) {
                    headers.insert(AUTHORIZATION, hv);
                }
            }
        }

        if let Some(id) = self.operator_id {
            if let Ok(hv) = HeaderValue::from_str(&id.to_string()) {
                headers.insert("Operator", hv);
            }
        }

        if body.is_some() {
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }

        if let Ok(hv) = HeaderValue::from_str(&self.user_agent()) {
            headers.insert(USER_AGENT, hv);
        }

        tracing::info!("[API] -> {method} {url}");
        if let Some(ref b) = body {
            tracing::info!("[API]   body: {}", truncate(&b.to_string(), 500));
        }

        let mut builder = self.http.request(method, url).headers(headers);
        if let Some(b) = body {
            builder = builder.body(b.to_string());
        }

        let response = builder.send().await?;
        tracing::info!("[API] <- {}", response.status());
        Ok(response)
    }

    // ─── Auth ────────────────────────────────────────────

    pub async fn request_login(&mut self, phone: &str) -> Result<LoginResult, AppError> {
        let path = format!("auth/v2/login/{}", urlencoding::encode(phone));
        let mut headers = HeaderMap::new();
        let basic = format!("Basic {}", self.basic_auth(phone));
        if let Ok(hv) = HeaderValue::from_str(&basic) {
            headers.insert(AUTHORIZATION, hv);
        }

        let url = format!("{BASE_URL}/{path}");
        let response = self
            .execute_request(Method::GET, &url, None, Some(headers))
            .await?;

        let status = response.status();

        if !status.is_success() && status != StatusCode::MULTIPLE_CHOICES {
            let text = response.text().await?;
            tracing::error!("[API]   error body: {}", truncate(&text, 500));
            return Err(AppError::Api {
                status: status.as_u16(),
                message: text,
            });
        }

        let text = response.text().await?;
        tracing::info!("[API]   response: {}", truncate(&text, 500));

        if status == StatusCode::MULTIPLE_CHOICES {
            let contracts: Vec<ContractAddress> = serde_json::from_str(&text)?;
            return Ok(LoginResult::NeedsContract { contracts });
        }

        let auth_response: AuthResponse = serde_json::from_str(&text)?;
        let login_info: LoginInfo = serde_json::from_value(auth_response.data)?;
        Ok(LoginResult::Ready { data: login_info })
    }

    pub async fn select_contract(
        &mut self,
        phone: &str,
        contract: &ContractAddress,
    ) -> Result<AuthCredentials, AppError> {
        let path = format!("auth/v2/confirmation/{}", urlencoding::encode(phone));
        let mut headers = HeaderMap::new();
        let basic = format!("Basic {}", self.basic_auth(phone));
        if let Ok(hv) = HeaderValue::from_str(&basic) {
            headers.insert(AUTHORIZATION, hv);
        }

        let body = serde_json::to_value(contract)?;
        self.request(Method::POST, &path, Some(body), Some(headers))
            .await
    }

    pub async fn confirm_login(
        &mut self,
        phone: &str,
        code: &str,
        contract: Option<&ContractAddress>,
    ) -> Result<AuthCredentials, AppError> {
        let path = format!(
            "auth/v3/auth/{}/confirmation",
            urlencoding::encode(phone)
        );
        let mut headers = HeaderMap::new();
        let basic = format!("Basic {}", self.basic_auth(phone));
        if let Ok(hv) = HeaderValue::from_str(&basic) {
            headers.insert(AUTHORIZATION, hv);
        }

        let mut body = json!({
            "confirm1": code,
            "confirm2": null,
            "login": phone,
        });

        if let Some(c) = contract {
            let obj = body.as_object_mut().expect("body is an object");
            obj.insert("operatorId".to_string(), json!(c.operator_id));
            obj.insert("accountId".to_string(), json!(c.account_id));
            obj.insert("profileId".to_string(), json!(c.profile_id));
            obj.insert(
                "subscriberId".to_string(),
                json!(c.subscriber_id.to_string()),
            );
        }

        let creds: AuthCredentials = self
            .request(Method::POST, &path, Some(body), Some(headers))
            .await?;

        self.access_token = Some(creds.access_token.clone());
        self.refresh_token = Some(creds.refresh_token.clone());
        self.operator_id = creds.operator_id;

        Ok(creds)
    }

    pub async fn refresh_session(&mut self) -> Result<AuthCredentials, AppError> {
        let refresh_token = self
            .refresh_token
            .as_ref()
            .ok_or_else(|| AppError::Auth("No refresh token available".to_string()))?
            .clone();

        let url = format!("{BASE_URL}/auth/v2/session/refresh");

        let mut headers = HeaderMap::new();
        if let Ok(hv) = HeaderValue::from_str(&refresh_token) {
            headers.insert("Bearer", hv);
        }
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        tracing::info!("[API] -> GET {url} (refresh)");

        let response = self
            .http
            .get(&url)
            .headers(headers)
            .send()
            .await?;

        let status = response.status();
        tracing::info!("[API] <- {status}");

        if !status.is_success() {
            let text = response.text().await?;
            tracing::error!("[API]   refresh error: {}", truncate(&text, 500));
            return Err(AppError::Auth(format!("Token refresh failed: {status}")));
        }

        let creds: AuthCredentials = response.json().await?;
        self.access_token = Some(creds.access_token.clone());
        self.refresh_token = Some(creds.refresh_token.clone());

        Ok(creds)
    }

    // ─── Places ──────────────────────────────────────────

    pub async fn get_places(&mut self) -> Result<Vec<SubscriberPlace>, AppError> {
        let resp: ApiResponse<Vec<SubscriberPlace>> =
            self.request(Method::GET, "rest/v3/subscriber-places", None, None).await?;
        Ok(resp.data)
    }

    // ─── Access Controls (Intercoms) ─────────────────────

    pub async fn get_access_controls(
        &mut self,
        place_id: i64,
    ) -> Result<Vec<AccessDevice>, AppError> {
        let path = format!("rest/v1/places/{place_id}/accesscontrols");
        let resp: ApiResponse<Vec<AccessDevice>> =
            self.request(Method::GET, &path, None, None).await?;
        Ok(resp.data)
    }

    pub async fn open_door(
        &mut self,
        place_id: i64,
        device: &AccessDevice,
    ) -> Result<(), AppError> {
        if device.open_method == "FORPOST" {
            let camera_id = device
                .external_camera_id
                .as_ref()
                .ok_or_else(|| {
                    AppError::Api {
                        status: 400,
                        message: "Forpost device is missing externalCameraId".to_string(),
                    }
                })?;
            let device_id = device
                .external_device_id
                .as_ref()
                .ok_or_else(|| {
                    AppError::Api {
                        status: 400,
                        message: "Forpost device is missing externalDeviceId".to_string(),
                    }
                })?;

            let path = format!("rest/v1/forpost/cameras/{camera_id}/devices/{device_id}/open");
            let mut headers = HeaderMap::new();
            if let Ok(hv) = HeaderValue::from_str(&place_id.to_string()) {
                headers.insert("X-Payment-PlaceId", hv);
            }

            let _: Value = self
                .request(Method::POST, &path, None, Some(headers))
                .await?;
        } else {
            let path = format!(
                "rest/v1/places/{place_id}/accesscontrols/{}/actions",
                device.id
            );
            let body = json!({ "name": "accessControlOpen" });
            let _: Value = self
                .request(Method::POST, &path, Some(body), None)
                .await?;
        }

        Ok(())
    }

    pub async fn open_entrance(
        &mut self,
        place_id: i64,
        access_control_id: i64,
        entrance_id: i64,
    ) -> Result<(), AppError> {
        let path = format!(
            "rest/v1/places/{place_id}/accesscontrols/{access_control_id}/entrances/{entrance_id}/actions"
        );
        let body = json!({ "name": "accessControlOpen" });
        let _: Value = self
            .request(Method::POST, &path, Some(body), None)
            .await?;
        Ok(())
    }

    // ─── SIP Devices ─────────────────────────────────────

    pub async fn create_sip_device(
        &mut self,
        place_id: i64,
        access_control_id: i64,
        installation_id: &str,
    ) -> Result<SipDevice, AppError> {
        let path = format!(
            "rest/v1/places/{place_id}/accesscontrols/{access_control_id}/sipdevices"
        );
        let body = json!({ "installationId": installation_id });
        let resp: SipDeviceResponse = self
            .request(Method::POST, &path, Some(body), None)
            .await?;
        Ok(resp.data)
    }

    // ─── Cameras ─────────────────────────────────────────

    pub async fn get_personal_cameras(
        &mut self,
        place_id: i64,
    ) -> Result<Vec<PersonalCamera>, AppError> {
        let path = format!("rest/v1/places/{place_id}/cameras");
        let resp: ApiResponse<Vec<PersonalCamera>> =
            self.request(Method::GET, &path, None, None).await?;
        Ok(resp.data)
    }

    pub fn get_sip_snapshot_url(&self, place_id: i64, device_id: i64) -> String {
        format!("{BASE_URL}/rest/v1/places/{place_id}/accesscontrols/{device_id}/videosnapshots")
    }

    pub fn get_forpost_snapshot_url(&self, camera_id: &str, width: u32, height: u32) -> String {
        format!(
            "{BASE_URL}/rest/v1/forpost/cameras/{camera_id}/snapshots?width={width}&height={height}"
        )
    }

    pub async fn refresh_video_session(&mut self, camera_id: &str) -> Result<(), AppError> {
        let path = format!(
            "api/mh-camera-personal/mobile/v1/video/refresh-user-session?externalCameraId={camera_id}"
        );
        let _: Value = self.request(Method::PUT, &path, None, None).await?;
        Ok(())
    }

    pub async fn get_video_stream(
        &mut self,
        camera_id: &str,
    ) -> Result<VideoStream, AppError> {
        let path = format!(
            "rest/v1/forpost/cameras/{camera_id}/video?LightStream=0&Format=HLS"
        );
        self.request(Method::GET, &path, None, None).await
    }

    pub async fn get_archive_stream(
        &mut self,
        camera_id: &str,
        ts: i64,
        tz: i64,
    ) -> Result<String, AppError> {
        let path = format!(
            "rest/v1/forpost/cameras/{camera_id}/video?TS={ts}&TZ={tz}&LightStream=0&Format=HLS"
        );
        let stream: VideoStream = self.request(Method::GET, &path, None, None).await?;
        Ok(stream.data.url)
    }

    pub async fn get_camera_events(
        &mut self,
        camera_id: &str,
        from: &str,
        to: &str,
    ) -> Result<Value, AppError> {
        let path = format!(
            "rest/v2/forpost/cameras/{camera_id}/events?LowerDate={}&UpperDate={}&Count=200&orderByTime=DESC",
            urlencoding::encode(from),
            urlencoding::encode(to),
        );
        self.request(Method::GET, &path, None, None).await
    }

    // ─── Proxy snapshot ──────────────────────────────────

    pub async fn proxy_snapshot(
        &mut self,
        place_id: i64,
        device_id: i64,
        device_type: &str,
        width: u32,
        height: u32,
    ) -> Result<Bytes, AppError> {
        let url = if device_type == "BUP" {
            self.get_forpost_snapshot_url(&device_id.to_string(), width, height)
        } else {
            self.get_sip_snapshot_url(place_id, device_id)
        };

        let response = self.fetch_snapshot(&url).await?;

        if response.status().is_server_error() {
            tracing::info!("[API]   snapshot 5xx, retrying once...");
            let retry = self.fetch_snapshot(&url).await?;
            if !retry.status().is_success() {
                return Err(AppError::Api {
                    status: retry.status().as_u16(),
                    message: "Snapshot retry failed".to_string(),
                });
            }
            let data = retry.bytes().await?;
            return Ok(data);
        }

        if !response.status().is_success() {
            return Err(AppError::Api {
                status: response.status().as_u16(),
                message: "Snapshot request failed".to_string(),
            });
        }

        let data = response.bytes().await?;
        Ok(data)
    }

    async fn fetch_snapshot(&self, url: &str) -> Result<reqwest::Response, AppError> {
        let mut headers = HeaderMap::new();

        if let Some(ref token) = self.access_token {
            let val = format!("Bearer {token}");
            if let Ok(hv) = HeaderValue::from_str(&val) {
                headers.insert(AUTHORIZATION, hv);
            }
        }

        if let Some(id) = self.operator_id {
            if let Ok(hv) = HeaderValue::from_str(&id.to_string()) {
                headers.insert("Operator", hv);
            }
        }

        let response = self.http.get(url).headers(headers).send().await?;
        Ok(response)
    }
}

/// Truncate a string to at most `max_len` characters for logging.
fn truncate(s: &str, max_len: usize) -> &str {
    if s.len() <= max_len {
        s
    } else {
        // Find a valid char boundary at or before max_len
        let mut end = max_len;
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        &s[..end]
    }
}
