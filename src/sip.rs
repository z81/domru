use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::sync::broadcast;
use tokio::time;

use crate::error::AppError;
use crate::state::SharedState;
use crate::types::{CallEvent, SipCredentials};

const SIP_PORT: u16 = 5060;
const LOCAL_PORT: u16 = 15060;
const RE_REGISTER_INTERVAL: Duration = Duration::from_secs(25); // Keep NAT pinhole alive (app uses Expires=30)
const REGISTER_TIMEOUT: Duration = Duration::from_secs(10);
const INVITE_AUTO_REJECT_DELAY: Duration = Duration::from_secs(30);
const USER_AGENT: &str = "RustDomofon/1.0";

pub struct SipClient {
    credentials: SipCredentials,
    call_tx: broadcast::Sender<CallEvent>,
    state: SharedState,
}

impl SipClient {
    pub fn new(credentials: SipCredentials, call_tx: broadcast::Sender<CallEvent>, state: SharedState) -> Self {
        Self {
            credentials,
            call_tx,
            state,
        }
    }

    pub async fn run(&self) -> Result<(), AppError> {
        let realm = &self.credentials.realm;

        // Resolve SIP server IP from realm
        let sip_server = resolve_sip_server(realm).await?;
        tracing::info!(
            "[SIP] Registering {}@{} -> {}:{}",
            self.credentials.login,
            realm,
            sip_server,
            SIP_PORT,
        );

        let socket = UdpSocket::bind(("0.0.0.0", LOCAL_PORT)).await?;
        let dest = format!("{sip_server}:{SIP_PORT}");

        let mut call_id = generate_call_id();
        let mut tag = random_tag();
        let mut cseq: u32 = 0;
        let mut public_ip = String::from("0.0.0.0");
        #[allow(unused_assignments)]
        let mut registered = false;

        // Initial REGISTER (no auth) to get nonce from 401 challenge
        cseq += 1;
        let register_msg = build_register(
            &self.credentials,
            &public_ip,
            LOCAL_PORT,
            &call_id,
            &tag,
            cseq,
            None,
        );
        socket.send_to(register_msg.as_bytes(), &dest).await?;

        // Wait for 401 challenge and complete registration with timeout
        let registration_result = time::timeout(
            REGISTER_TIMEOUT,
            complete_registration(
                &socket,
                &dest,
                &self.credentials,
                &mut public_ip,
                &call_id,
                &tag,
                &mut cseq,
            ),
        )
        .await;

        match registration_result {
            Ok(Ok(())) => {
                registered = true;
                tracing::info!(
                    "[SIP] Registered successfully as {}@{}",
                    self.credentials.login,
                    realm,
                );
            }
            Ok(Err(err)) => {
                return Err(err);
            }
            Err(_) => {
                return Err(AppError::Sip("SIP REGISTER timeout".to_string()));
            }
        }

        // Main loop: recv messages + periodic re-registration
        let mut re_register_interval = time::interval(RE_REGISTER_INTERVAL);
        // Consume the first immediate tick
        re_register_interval.tick().await;

        let mut buf = vec![0u8; 4096];

        loop {
            tokio::select! {
                result = socket.recv_from(&mut buf) => {
                    match result {
                        Ok((len, _addr)) => {
                            let text = String::from_utf8_lossy(&buf[..len]).to_string();
                            self.handle_message(
                                &text,
                                &socket,
                                &dest,
                                &self.credentials,
                                &mut public_ip,
                                &call_id,
                                &tag,
                                &mut cseq,
                                &mut registered,
                            ).await;
                        }
                        Err(err) => {
                            tracing::error!("[SIP] Socket recv error: {}", err);
                        }
                    }
                }
                _ = re_register_interval.tick() => {
                    tracing::info!("[SIP] Re-registering...");
                    call_id = generate_call_id();
                    tag = random_tag();
                    cseq = 0;
                    registered = false;

                    cseq += 1;
                    let msg = build_register(
                        &self.credentials,
                        &public_ip,
                        LOCAL_PORT,
                        &call_id,
                        &tag,
                        cseq,
                        None,
                    );
                    if let Err(err) = socket.send_to(msg.as_bytes(), &dest).await {
                        tracing::error!("[SIP] Re-register send error: {}", err);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_message(
        &self,
        text: &str,
        socket: &UdpSocket,
        dest: &str,
        credentials: &SipCredentials,
        public_ip: &mut String,
        call_id: &str,
        tag: &str,
        cseq: &mut u32,
        registered: &mut bool,
    ) {
        let first_line = text.lines().next().unwrap_or("");

        // Extract received IP from Via header
        if let Some(ip) = extract_received_ip(text) {
            *public_ip = ip;
        }

        // Skip provisional 100 Trying
        if first_line.contains("100 ") {
            return;
        }

        // 401 Unauthorized — need Digest auth
        if first_line.contains("401") {
            if let Some(nonce) = extract_nonce(text) {
                tracing::info!("[SIP] Got 401 challenge, authenticating...");
                *cseq += 1;
                let msg = build_register(
                    credentials,
                    public_ip,
                    LOCAL_PORT,
                    call_id,
                    tag,
                    *cseq,
                    Some(&nonce),
                );
                if let Err(err) = socket.send_to(msg.as_bytes(), dest).await {
                    tracing::error!("[SIP] Send error: {}", err);
                }
            }
            return;
        }

        // 200 OK on REGISTER
        if first_line.starts_with("SIP/2.0 200") && text.contains("REGISTER") {
            if !*registered {
                tracing::info!(
                    "[SIP] Registered successfully as {}@{}",
                    credentials.login,
                    credentials.realm,
                );
                *registered = true;
            }
            return;
        }

        // INVITE — incoming call
        if first_line.starts_with("INVITE ") {
            tracing::info!("[SIP] INCOMING CALL detected!");
            let from = extract_from(text);

            let call_event = CallEvent {
                event_type: "incoming_call".to_string(),
                date: chrono_now_iso(),
                from,
                sip_message: first_line.to_string(),
            };

            // Store raw INVITE for later answer
            {
                let mut invite = self.state.last_invite.write().await;
                *invite = Some(text.to_string());
            }

            // Broadcast the call event (ignore error if no receivers)
            let _ = self.call_tx.send(call_event);

            // Send 180 Ringing
            let ringing = build_response(text, "180 Ringing");
            if let Err(err) = socket.send_to(ringing.as_bytes(), dest).await {
                tracing::error!("[SIP] Send 180 error: {}", err);
            }

            // Wait for answer command or auto-reject after timeout
            let answer_rx = &self.state.sip_answer_rx;
            let answered = tokio::time::timeout(
                INVITE_AUTO_REJECT_DELAY,
                answer_rx.lock().await.recv(),
            ).await;

            if answered.is_ok() {
                // Answer: send 200 OK
                tracing::info!("[SIP] Answering call (200 OK)");
                let ok = build_response(text, "200 OK");
                if let Err(err) = socket.send_to(ok.as_bytes(), dest).await {
                    tracing::error!("[SIP] Send 200 OK error: {}", err);
                }
                // Wait a bit then send BYE to hang up
                time::sleep(Duration::from_secs(2)).await;
                let bye = build_bye(text, &self.credentials, public_ip, LOCAL_PORT);
                if let Err(err) = socket.send_to(bye.as_bytes(), dest).await {
                    tracing::error!("[SIP] Send BYE error: {}", err);
                }
                tracing::info!("[SIP] Call answered and hung up");
            } else {
                // Timeout: auto-reject with 486
                tracing::info!("[SIP] No answer, auto-rejecting (486)");
                let busy = build_response(text, "486 Busy Here");
                if let Err(err) = socket.send_to(busy.as_bytes(), dest).await {
                    tracing::error!("[SIP] Send 486 error: {}", err);
                }
            }

            // Clear stored invite
            {
                let mut invite = self.state.last_invite.write().await;
                *invite = None;
            }
            return;
        }

        // NOTIFY — respond 200 OK
        if first_line.starts_with("NOTIFY ") {
            let resp = build_response(text, "200 OK");
            if let Err(err) = socket.send_to(resp.as_bytes(), dest).await {
                tracing::error!("[SIP] Send NOTIFY response error: {}", err);
            }
            return;
        }

        // BYE — respond 200 OK
        if first_line.starts_with("BYE ") {
            let resp = build_response(text, "200 OK");
            if let Err(err) = socket.send_to(resp.as_bytes(), dest).await {
                tracing::error!("[SIP] Send BYE response error: {}", err);
            }
            return;
        }

        // OPTIONS — respond 200 OK
        if first_line.starts_with("OPTIONS ") {
            let resp = build_response(text, "200 OK");
            if let Err(err) = socket.send_to(resp.as_bytes(), dest).await {
                tracing::error!("[SIP] Send OPTIONS response error: {}", err);
            }
        }
    }
}

// ─── Initial registration handshake ──────────────────

/// Waits for the 401 challenge, sends authenticated REGISTER, and waits for 200 OK.
async fn complete_registration(
    socket: &UdpSocket,
    dest: &str,
    credentials: &SipCredentials,
    public_ip: &mut String,
    call_id: &str,
    tag: &str,
    cseq: &mut u32,
) -> Result<(), AppError> {
    let mut buf = vec![0u8; 4096];

    loop {
        let (len, _addr) = socket.recv_from(&mut buf).await?;
        let text = String::from_utf8_lossy(&buf[..len]).to_string();
        let first_line = text.lines().next().unwrap_or("");

        if let Some(ip) = extract_received_ip(&text) {
            *public_ip = ip;
        }

        // Skip provisional
        if first_line.contains("100 ") {
            continue;
        }

        // 401 — extract nonce and re-register with Digest
        if first_line.contains("401") {
            if let Some(nonce) = extract_nonce(&text) {
                tracing::info!("[SIP] Got 401 challenge, authenticating...");
                *cseq += 1;
                let msg = build_register(
                    credentials,
                    public_ip,
                    LOCAL_PORT,
                    call_id,
                    tag,
                    *cseq,
                    Some(&nonce),
                );
                socket.send_to(msg.as_bytes(), dest).await?;
            } else {
                return Err(AppError::Sip(
                    "401 response without nonce".to_string(),
                ));
            }
            continue;
        }

        // 200 OK on REGISTER — success
        if first_line.starts_with("SIP/2.0 200") && text.contains("REGISTER") {
            return Ok(());
        }
    }
}

// ─── DNS resolution ──────────────────────────────────

async fn resolve_sip_server(realm: &str) -> Result<String, AppError> {
    let lookup_addr = format!("{realm}:{SIP_PORT}");
    match tokio::net::lookup_host(lookup_addr).await {
        Ok(mut addrs) => {
            if let Some(addr) = addrs.next() {
                Ok(addr.ip().to_string())
            } else {
                Ok(realm.to_string())
            }
        }
        Err(_) => Ok(realm.to_string()),
    }
}

// ─── SIP message builders ────────────────────────────

fn build_register(
    creds: &SipCredentials,
    public_ip: &str,
    local_port: u16,
    call_id: &str,
    tag: &str,
    cseq: u32,
    nonce: Option<&str>,
) -> String {
    let login = &creds.login;
    let realm = &creds.realm;
    let uri = format!("sip:{realm}");
    let br = random_branch();

    let mut lines = vec![
        format!("REGISTER {uri} SIP/2.0"),
        format!("Via: SIP/2.0/UDP {public_ip}:{local_port};branch={br};rport"),
        "Max-Forwards: 70".to_string(),
        format!("From: <sip:{login}@{realm}>;tag={tag}"),
        format!("To: <sip:{login}@{realm}>"),
        format!("Call-ID: {call_id}"),
        format!("CSeq: {cseq} REGISTER"),
        format!("Contact: <sip:{login}@{public_ip}:{local_port};transport=udp>"),
        "Expires: 30".to_string(),
        format!("User-Agent: {USER_AGENT}"),
    ];

    if let Some(nonce_val) = nonce {
        let response = compute_digest(login, realm, &creds.password, nonce_val, "REGISTER", &uri);
        lines.push(format!(
            "Authorization: Digest username=\"{login}\", realm=\"{realm}\", nonce=\"{nonce_val}\", uri=\"{uri}\", response=\"{response}\", algorithm=MD5"
        ));
    }

    lines.push("Content-Length: 0".to_string());
    lines.push(String::new());
    lines.push(String::new());

    lines.join("\r\n")
}

fn build_response(request_text: &str, status: &str) -> String {
    let lines: Vec<&str> = request_text.split("\r\n").collect();

    let mut via_lines: Vec<&str> = Vec::new();
    let mut from_line = "";
    let mut to_line = "";
    let mut call_id_line = "";
    let mut cseq_line = "";

    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("via:") || lower.starts_with("v:") {
            via_lines.push(line);
        } else if from_line.is_empty() && (lower.starts_with("from:") || lower.starts_with("f:")) {
            from_line = line;
        } else if to_line.is_empty() && (lower.starts_with("to:") || lower.starts_with("t:")) {
            to_line = line;
        } else if call_id_line.is_empty()
            && (lower.starts_with("call-id:") || lower.starts_with("i:"))
        {
            call_id_line = line;
        } else if cseq_line.is_empty() && lower.starts_with("cseq:") {
            cseq_line = line;
        }
    }

    let mut resp = vec![format!("SIP/2.0 {status}")];
    for via in &via_lines {
        resp.push(via.to_string());
    }
    resp.push(from_line.to_string());
    resp.push(to_line.to_string());
    resp.push(call_id_line.to_string());
    resp.push(cseq_line.to_string());
    resp.push("Content-Length: 0".to_string());
    resp.push(String::new());
    resp.push(String::new());

    resp.join("\r\n")
}

// ─── Digest authentication ───────────────────────────

fn compute_digest(
    login: &str,
    realm: &str,
    password: &str,
    nonce: &str,
    method: &str,
    uri: &str,
) -> String {
    let ha1 = md5_hex(&format!("{login}:{realm}:{password}"));
    let ha2 = md5_hex(&format!("{method}:{uri}"));
    md5_hex(&format!("{ha1}:{nonce}:{ha2}"))
}

fn md5_hex(input: &str) -> String {
    format!("{:x}", md5::compute(input))
}

// ─── SIP header parsers ──────────────────────────────

fn extract_received_ip(text: &str) -> Option<String> {
    let idx = text.find("received=")?;
    let after = &text[idx + "received=".len()..];
    let end = after.find(|c: char| !c.is_ascii_digit() && c != '.').unwrap_or(after.len());
    let ip = &after[..end];
    if ip.is_empty() {
        None
    } else {
        Some(ip.to_string())
    }
}

fn extract_nonce(text: &str) -> Option<String> {
    let idx = text.find("nonce=\"")?;
    let after = &text[idx + "nonce=\"".len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

fn extract_from(text: &str) -> String {
    // Parse From or compact form f header, case-insensitively
    for line in text.split("\r\n") {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("from:") || lower.starts_with("f:") {
            // Try to extract URI from angle brackets first
            if let Some(start) = line.find('<') {
                if let Some(end) = line.find('>') {
                    if start < end {
                        return line[start + 1..end].to_string();
                    }
                }
            }
            // Otherwise return the value part trimmed
            let colon_pos = line.find(':').unwrap_or(0);
            let value = line[colon_pos + 1..].trim();
            // Strip any parameters (;tag=...)
            let semi = value.find(';').unwrap_or(value.len());
            return value[..semi].trim().to_string();
        }
    }
    "unknown".to_string()
}

// ─── Random generators ───────────────────────────────

fn random_branch() -> String {
    format!("z9hG4bK-{}", random_alphanum())
}

fn random_tag() -> String {
    random_alphanum()
}

fn random_alphanum() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = (0..12)
        .map(|_| {
            let idx: u8 = rng.gen_range(0..36);
            if idx < 10 {
                char::from(b'0' + idx)
            } else {
                char::from(b'a' + idx - 10)
            }
        })
        .collect();
    chars.into_iter().collect()
}

fn generate_call_id() -> String {
    format!("rust-domofon-{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis())
}

/// Returns the current time in ISO 8601 format.
fn chrono_now_iso() -> String {
    // Use a simple approach without pulling in the chrono crate.
    // Format: 2024-01-15T10:30:00.000Z
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0));
    let secs = duration.as_secs();
    let millis = duration.subsec_millis();

    // Calculate date/time components
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Days since epoch to year/month/day
    let (year, month, day) = days_to_date(days);

    format!(
        "{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z"
    )
}

/// Convert days since Unix epoch to (year, month, day).
fn days_to_date(days_since_epoch: u64) -> (u64, u64, u64) {
    // Algorithm from Howard Hinnant's civil_from_days
    let z = days_since_epoch + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Build a SIP BYE from the original INVITE to hang up.
fn build_bye(invite_text: &str, creds: &SipCredentials, public_ip: &str, local_port: u16) -> String {
    let lines: Vec<&str> = invite_text.split("\r\n").collect();

    // Extract To, From, Call-ID from the INVITE
    let mut to_line = String::new();
    let mut from_line = String::new();
    let mut call_id_line = String::new();

    for line in &lines {
        let lower = line.to_ascii_lowercase();
        if to_line.is_empty() && (lower.starts_with("to:") || lower.starts_with("t:")) {
            to_line = line.to_string();
        } else if from_line.is_empty() && (lower.starts_with("from:") || lower.starts_with("f:")) {
            from_line = line.to_string();
        } else if call_id_line.is_empty() && (lower.starts_with("call-id:") || lower.starts_with("i:")) {
            call_id_line = line.to_string();
        }
    }

    // For BYE we swap From/To (we are the callee sending BYE)
    let br = random_branch();
    let bye = vec![
        format!("BYE sip:{}@{} SIP/2.0", creds.login, creds.realm),
        format!("Via: SIP/2.0/UDP {public_ip}:{local_port};branch={br};rport"),
        "Max-Forwards: 70".to_string(),
        to_line,
        from_line,
        call_id_line,
        "CSeq: 1 BYE".to_string(),
        format!("User-Agent: {USER_AGENT}"),
        "Content-Length: 0".to_string(),
        String::new(),
        String::new(),
    ];
    bye.join("\r\n")
}
