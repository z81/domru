mod api;
mod client;
mod error;
mod sip;
mod state;
mod types;

use std::net::SocketAddr;
use tower_http::services::ServeDir;
use tracing_subscriber::EnvFilter;

use crate::client::DomofonClient;
use crate::sip::SipClient;
use crate::state::SharedState;

#[tokio::main]
async fn main() {
    // Init logging
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    // Load persisted data
    let tokens = state::load_tokens(&state::tokens_path());
    let config = state::load_config(&state::config_path());
    let sip_creds = state::load_sip_credentials(&state::sip_credentials_path());

    // Create client
    let mut client = DomofonClient::new();
    if let Some(ref t) = tokens {
        client.set_tokens(
            t.access_token.clone(),
            t.refresh_token.clone(),
            t.operator_id,
        );
        tracing::info!("Loaded tokens (operator_id: {:?})", t.operator_id);
    }

    // Create shared state
    let app_state = state::AppState::new(client, config);

    // Fetch place_id for User-Agent (required for video API)
    if tokens.is_some() {
        let mut client = app_state.client.write().await;
        if let Ok(places) = client.get_places().await {
            if let Some(first) = places.first() {
                client.set_place_id(first.place.id);
                tracing::info!("Using place_id={} for User-Agent", first.place.id);
            }
        }
    }

    // Spawn token auto-refresh (every 10 min)
    spawn_token_refresh(app_state.clone());

    // Spawn SIP client for call detection
    spawn_sip_client(app_state.clone(), sip_creds).await;

    // Spawn webhook listener (fires webhook on ANY call event, including SIP)
    api::sse::spawn_webhook_listener(app_state.clone());

    // Build Axum app
    let app = api::router()
        .fallback_service(ServeDir::new("public").append_index_html_on_directories(true))
        .with_state(app_state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("Server running at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

fn spawn_token_refresh(state: SharedState) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));
        interval.tick().await; // skip first immediate tick
        loop {
            interval.tick().await;
            let mut client = state.client.write().await;
            if !client.is_authenticated() {
                continue;
            }
            match client.refresh_session().await {
                Ok(creds) => {
                    let tokens = client.get_tokens();
                    state::save_tokens(&state::tokens_path(), &tokens);
                    tracing::info!(
                        "[AUTH] Token refreshed, operator_id: {:?}",
                        creds.operator_id
                    );
                }
                Err(e) => {
                    tracing::error!("[AUTH] Token refresh failed: {}", e);
                }
            }
        }
    });
}

async fn spawn_sip_client(state: SharedState, saved_creds: Option<types::SipCredentials>) {
    let call_tx = state.call_tx.clone();

    tokio::spawn(async move {
        // Wait for server to boot
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        tracing::info!("[SIP] Starting SIP client initialization...");

        // Check auth
        {
            let client = state.client.read().await;
            if !client.is_authenticated() {
                tracing::info!("[SIP] Not authenticated, skipping SIP registration");
                return;
            }
        }

        // Refresh token first (with timeout)
        tracing::info!("[SIP] Refreshing token...");
        {
            let refresh_result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                async {
                    let mut client = state.client.write().await;
                    let result = client.refresh_session().await;
                    if result.is_ok() {
                        let tokens = client.get_tokens();
                        state::save_tokens(&state::tokens_path(), &tokens);
                    }
                    result
                }
            ).await;

            match refresh_result {
                Ok(Ok(_)) => tracing::info!("[SIP] Token refreshed"),
                Ok(Err(e)) => tracing::error!("[SIP] Token refresh failed: {}", e),
                Err(_) => tracing::error!("[SIP] Token refresh timed out"),
            }
        }

        // Load or create SIP credentials
        let creds = if let Some(c) = saved_creds {
            tracing::info!("[SIP] Loaded credentials: {}@{}", c.login, c.realm);
            c
        } else {
            // Try to create SIP device
            match create_sip_device(&state).await {
                Some(c) => c,
                None => {
                    tracing::info!("[SIP] No SIP credentials available. Call detection disabled.");
                    return;
                }
            }
        };

        // Run SIP client (loops forever with re-registration)
        let sip = SipClient::new(creds, call_tx, state.clone());
        if let Err(e) = sip.run().await {
            tracing::error!("[SIP] SIP client error: {}", e);
        }
    });
}

async fn create_sip_device(state: &SharedState) -> Option<types::SipCredentials> {
    let mut client = state.client.write().await;
    let places = match client.get_places().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("[SIP] Failed to get places: {}", e);
            return None;
        }
    };

    for place in &places {
        let devices = match client.get_access_controls(place.place.id).await {
            Ok(d) => d,
            Err(_) => continue,
        };

        let sip_device = devices.iter().find(|d| d.type_ == "SIP");
        if let Some(device) = sip_device {
            let installation_id = format!("rust-domofon-{}", std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis());

            match client
                .create_sip_device(place.place.id, device.id, &installation_id)
                .await
            {
                Ok(sip_dev) => {
                    let creds = types::SipCredentials {
                        login: sip_dev.login.clone(),
                        password: sip_dev.password.clone(),
                        realm: sip_dev.realm.clone(),
                    };
                    state::save_sip_credentials(&state::sip_credentials_path(), &creds);
                    tracing::info!("[SIP] Created SIP device: {}@{}", creds.login, creds.realm);
                    return Some(creds);
                }
                Err(e) => {
                    tracing::error!("[SIP] Failed to create SIP device: {}", e);
                }
            }
        }
    }

    None
}
