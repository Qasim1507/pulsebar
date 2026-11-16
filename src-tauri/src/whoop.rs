//! WHOOP API v2: one-time OAuth (authorization code + PKCE, loopback
//! redirect), silent token refresh, and a 15-minute metrics poll.

use crate::{demo, now_ms, settings, Shared};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use reqwest::{StatusCode, Url};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Emitter};
use tauri_plugin_opener::OpenerExt;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    time::{sleep, timeout},
};

const AUTH_URL: &str = "https://api.prod.whoop.com/oauth/oauth2/auth";
const TOKEN_URL: &str = "https://api.prod.whoop.com/oauth/oauth2/token";
const API_BASE: &str = "https://api.prod.whoop.com/developer";
const SCOPES: &str = "offline read:recovery read:cycles read:sleep read:workout read:profile";

pub const REDIRECT_PORT: u16 = 8765;
/// Must be registered as a Redirect URL on the WHOOP developer app.
pub const REDIRECT_URI: &str = "http://localhost:8765/callback";

/// The shared WHOOP app that lets people sign in without creating their own
/// developer keys. Fill in both values after deploying `broker/` (see
/// broker/README.md). While either is empty, the app asks for the user's own
/// Client ID and Secret instead. Neither value is secret.
const SHARED_CLIENT_ID: &str = "e002d840-76b7-492c-ba89-4108b1532548";
/// Base URL of the deployed token broker, e.g. "https://pulsebar-whoop-broker.example.workers.dev".
const BROKER_URL: &str = "https://pulsebar-whoop-broker.pulsebar1.workers.dev";

pub fn shared_app_available() -> bool {
    !SHARED_CLIENT_ID.is_empty() && !BROKER_URL.is_empty()
}

/// Which WHOOP developer app a sign-in or refresh goes through.
pub enum Client {
    /// The user's own keys, talking to WHOOP directly.
    Own { id: String, secret: String },
    /// The built-in app; the broker adds the secret server-side.
    Shared,
}

impl Client {
    /// The user's own keys win if they entered any; otherwise the shared app.
    pub fn current(shared: &Shared) -> Option<Self> {
        let id = shared.settings.lock().unwrap().client_id.clone();
        if !id.is_empty() {
            let secret = shared.client_secret.lock().unwrap().clone()?;
            return Some(Self::Own { id, secret });
        }
        shared_app_available().then_some(Self::Shared)
    }

    fn id(&self) -> &str {
        match self {
            Self::Own { id, .. } => id,
            Self::Shared => SHARED_CLIENT_ID,
        }
    }

    fn token_url(&self) -> String {
        match self {
            Self::Own { .. } => TOKEN_URL.to_string(),
            Self::Shared => format!("{}/oauth/token", BROKER_URL.trim_end_matches('/')),
        }
    }

    /// Token request fields, with credentials added only for the user's own app.
    fn form<'a>(&'a self, mut fields: Vec<(&'a str, &'a str)>) -> Vec<(&'a str, &'a str)> {
        if let Self::Own { id, secret } = self {
            fields.push(("client_id", id));
            fields.push(("client_secret", secret));
        }
        fields
    }
}

const POLL_INTERVAL: Duration = Duration::from_secs(15 * 60);
const OAUTH_TIMEOUT: Duration = Duration::from_secs(5 * 60);

#[derive(Clone, Serialize, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    /// Unix seconds.
    pub expires_at: i64,
}

#[derive(Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metrics {
    pub recovery_pct: Option<f64>,
    pub hrv_ms: Option<f64>,
    pub rhr: Option<f64>,
    pub sleep_pct: Option<f64>,
    pub strain: Option<f64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsState {
    pub data: Option<Metrics>,
    /// whoop | demo | none
    pub source: &'static str,
    pub updated_at: Option<i64>,
    pub error: Option<String>,
}

impl MetricsState {
    pub fn none() -> Self {
        Self { data: None, source: "none", updated_at: None, error: None }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthState {
    /// none | pending | connected | error
    pub status: &'static str,
    pub detail: Option<String>,
}

impl AuthState {
    pub fn new(status: &'static str, detail: Option<String>) -> Self {
        Self { status, detail }
    }
}

pub enum ApiError {
    Unauthorized,
    Other(String),
}

// ---------------------------------------------------------------- OAuth ----

fn random_urlsafe(bytes: usize) -> Result<String, String> {
    let mut buf = vec![0u8; bytes];
    getrandom::getrandom(&mut buf).map_err(|e| e.to_string())?;
    Ok(URL_SAFE_NO_PAD.encode(buf))
}

/// Open the browser on WHOOP's consent page, wait for the redirect on the
/// loopback listener, and exchange the code for tokens.
pub async fn authorize(app: &AppHandle, client: &Client) -> Result<Tokens, String> {
    // Browsers may resolve "localhost" to either stack, so listen on both.
    let v4 = TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).await.map_err(|e| {
        format!("Could not listen on port {REDIRECT_PORT} for the OAuth callback: {e}")
    })?;
    let v6 = TcpListener::bind(("::1", REDIRECT_PORT)).await.ok();

    let state = random_urlsafe(16)?;
    let verifier = random_urlsafe(48)?;
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));

    let mut url = Url::parse(AUTH_URL).map_err(|e| e.to_string())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", client.id())
        .append_pair("redirect_uri", REDIRECT_URI)
        .append_pair("scope", SCOPES)
        .append_pair("state", &state)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");

    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|e| format!("Could not open the browser: {e}"))?;

    let code = timeout(OAUTH_TIMEOUT, wait_for_callback(&v4, v6.as_ref(), &state))
        .await
        .map_err(|_| "Timed out waiting for WHOOP sign-in".to_string())??;

    let http = reqwest::Client::new();
    let form = client.form(vec![
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", &verifier),
    ]);
    request_tokens(&http, &client.token_url(), &form)
    .await
    .map_err(|e| match e {
        ApiError::Unauthorized => match client {
            Client::Own { .. } => "WHOOP rejected the Client ID or Secret".to_string(),
            Client::Shared => "WHOOP rejected the sign-in — please try again".to_string(),
        },
        ApiError::Other(e) => e,
    })
}

async fn accept(v4: &TcpListener, v6: Option<&TcpListener>) -> std::io::Result<TcpStream> {
    match v6 {
        Some(v6) => tokio::select! {
            r = v4.accept() => r.map(|(s, _)| s),
            r = v6.accept() => r.map(|(s, _)| s),
        },
        None => v4.accept().await.map(|(s, _)| s),
    }
}

async fn wait_for_callback(
    v4: &TcpListener,
    v6: Option<&TcpListener>,
    expected_state: &str,
) -> Result<String, String> {
    loop {
        let mut stream = accept(v4, v6).await.map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 8192];
        let n = match timeout(Duration::from_secs(5), stream.read(&mut buf)).await {
            Ok(Ok(n)) => n,
            _ => continue,
        };
        let request = String::from_utf8_lossy(&buf[..n]);
        let path = request.split_whitespace().nth(1).unwrap_or("");
        if !path.starts_with("/callback") {
            respond(&mut stream, "404 Not Found", "Not found", "").await;
            continue;
        }

        let Ok(url) = Url::parse(&format!("http://localhost{path}")) else { continue };
        let param = |key: &str| {
            url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.into_owned())
        };

        let result = if let Some(err) = param("error") {
            Err(param("error_description").unwrap_or(err))
        } else if param("state").as_deref() != Some(expected_state) {
            Err("OAuth state mismatch — please try again".to_string())
        } else {
            param("code").ok_or_else(|| "WHOOP did not return an authorization code".to_string())
        };

        match &result {
            Ok(_) => {
                respond(
                    &mut stream,
                    "200 OK",
                    "WHOOP connected",
                    "You can close this tab and return to PulseBar in your menu bar.",
                )
                .await
            }
            Err(e) => respond(&mut stream, "400 Bad Request", "Sign-in failed", e).await,
        }
        return result;
    }
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

async fn respond(stream: &mut TcpStream, status: &str, title: &str, message: &str) {
    let body = format!(
        "<!doctype html><meta charset=utf-8><title>PulseBar</title>\
         <body style=\"margin:0;height:100vh;display:grid;place-items:center;background:#0b0c0f;\
         color:#f2f3f5;font-family:-apple-system,Segoe UI,sans-serif;text-align:center\">\
         <div><h1 style=\"font-weight:600\">{}</h1><p style=\"color:#9aa0aa\">{}</p></div>",
        escape_html(title),
        escape_html(message)
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

async fn request_tokens(http: &reqwest::Client, url: &str, form: &[(&str, &str)]) -> Result<Tokens, ApiError> {
    let res = http
        .post(url)
        .timeout(Duration::from_secs(30))
        .form(form)
        .send()
        .await
        .map_err(|e| ApiError::Other(format!("Could not reach WHOOP: {e}")))?;
    let status = res.status();
    if !status.is_success() {
        let body = res.text().await.unwrap_or_default();
        // 400 invalid_grant / 401 invalid_client both mean "sign in again".
        if status == StatusCode::UNAUTHORIZED || body.contains("invalid_grant") || body.contains("invalid_client") {
            return Err(ApiError::Unauthorized);
        }
        return Err(ApiError::Other(format!("Token request failed ({status})")));
    }
    let t: TokenResponse = res
        .json()
        .await
        .map_err(|e| ApiError::Other(format!("Unexpected token response: {e}")))?;
    Ok(Tokens {
        access_token: t.access_token,
        refresh_token: t.refresh_token,
        expires_at: now_ms() / 1000 + t.expires_in.unwrap_or(3600),
    })
}

async fn refresh(shared: &Shared, tokens: &Tokens) -> Result<Tokens, ApiError> {
    let refresh_token = tokens.refresh_token.as_deref().ok_or(ApiError::Unauthorized)?;
    let client = Client::current(shared).ok_or(ApiError::Unauthorized)?;
    let form = client.form(vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("scope", "offline"),
    ]);
    let mut fresh = request_tokens(&shared.http, &client.token_url(), &form).await?;
    // WHOOP rotates refresh tokens; keep the old one only if none came back.
    if fresh.refresh_token.is_none() {
        fresh.refresh_token = tokens.refresh_token.clone();
    }
    Ok(fresh)
}

pub async fn store_tokens(shared: &Shared, tokens: &Tokens) -> Result<(), String> {
    let json = serde_json::to_string(tokens).map_err(|e| e.to_string())?;
    settings::secret_set(settings::KEY_TOKENS, json).await?;
    *shared.tokens.lock().await = Some(tokens.clone());
    Ok(())
}

// ------------------------------------------------------------- Metrics ----

async fn get_json(http: &reqwest::Client, access: &str, path: &str) -> Result<Value, ApiError> {
    let res = http
        .get(format!("{API_BASE}{path}"))
        .bearer_auth(access)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| ApiError::Other(format!("Could not reach WHOOP: {e}")))?;
    match res.status() {
        StatusCode::UNAUTHORIZED => Err(ApiError::Unauthorized),
        StatusCode::TOO_MANY_REQUESTS => Err(ApiError::Other("WHOOP rate limit hit".into())),
        s if !s.is_success() => Err(ApiError::Other(format!("WHOOP API error ({s}) on {path}"))),
        _ => res
            .json()
            .await
            .map_err(|e| ApiError::Other(format!("Unexpected response from {path}: {e}"))),
    }
}

async fn fetch_metrics(http: &reqwest::Client, access: &str) -> Result<Metrics, ApiError> {
    let (recovery, cycle, sleep) = tokio::try_join!(
        get_json(http, access, "/v2/recovery?limit=1"),
        get_json(http, access, "/v2/cycle?limit=1"),
        get_json(http, access, "/v2/activity/sleep?limit=5"),
    )?;

    let latest_score = |v: &Value| v["records"].get(0).map(|r| r["score"].clone());
    let recovery = latest_score(&recovery).unwrap_or(Value::Null);
    let cycle = latest_score(&cycle).unwrap_or(Value::Null);
    // Most recent main sleep, skipping naps.
    let sleep = sleep["records"]
        .as_array()
        .and_then(|records| {
            records.iter().find(|r| r["nap"].as_bool() != Some(true) && r["score"].is_object())
        })
        .map(|r| r["score"].clone())
        .unwrap_or(Value::Null);

    Ok(Metrics {
        recovery_pct: recovery["recovery_score"].as_f64(),
        hrv_ms: recovery["hrv_rmssd_milli"].as_f64(),
        rhr: recovery["resting_heart_rate"].as_f64(),
        sleep_pct: sleep["sleep_performance_percentage"].as_f64(),
        strain: cycle["strain"].as_f64(),
    })
}

fn publish(app: &AppHandle, shared: &Shared, state: MetricsState) {
    *shared.metrics.lock().unwrap() = state.clone();
    let _ = app.emit("metrics", state);
}

pub fn set_auth(app: &AppHandle, shared: &Shared, status: &'static str, detail: Option<String>) {
    let state = AuthState::new(status, detail);
    *shared.auth.lock().unwrap() = state.clone();
    let _ = app.emit("auth", state);
}

async fn poll_once(app: &AppHandle, shared: &Shared) {
    let Some(mut tokens) = shared.tokens.lock().await.clone() else {
        // Not signed in: show simulated numbers in demo mode, otherwise nothing.
        let demo_mode = shared.settings.lock().unwrap().demo_mode;
        publish(
            app,
            shared,
            if demo_mode {
                MetricsState {
                    data: Some(demo::metrics()),
                    source: "demo",
                    updated_at: Some(now_ms()),
                    error: None,
                }
            } else {
                MetricsState::none()
            },
        );
        return;
    };

    // Up to two attempts: the second one follows a forced token refresh.
    for attempt in 0..2 {
        let expired = tokens.expires_at - 60 <= now_ms() / 1000;
        if expired || attempt == 1 {
            match refresh(shared, &tokens).await {
                Ok(fresh) => {
                    let _ = store_tokens(shared, &fresh).await;
                    tokens = fresh;
                }
                Err(ApiError::Unauthorized) => {
                    *shared.tokens.lock().await = None;
                    settings::secret_delete(settings::KEY_TOKENS).await;
                    set_auth(app, shared, "error", Some("WHOOP session expired — reconnect in Settings".into()));
                    publish(app, shared, MetricsState::none());
                    return;
                }
                Err(ApiError::Other(e)) => {
                    // Offline or WHOOP is down: keep showing the last values.
                    let mut state = shared.metrics.lock().unwrap().clone();
                    state.error = Some(e);
                    publish(app, shared, state);
                    return;
                }
            }
        }

        match fetch_metrics(&shared.http, &tokens.access_token).await {
            Ok(metrics) => {
                publish(
                    app,
                    shared,
                    MetricsState {
                        data: Some(metrics),
                        source: "whoop",
                        updated_at: Some(now_ms()),
                        error: None,
                    },
                );
                return;
            }
            Err(ApiError::Unauthorized) if attempt == 0 => continue,
            Err(ApiError::Unauthorized) => {
                let mut state = shared.metrics.lock().unwrap().clone();
                state.error = Some("WHOOP rejected the access token".into());
                publish(app, shared, state);
                return;
            }
            Err(ApiError::Other(e)) => {
                let mut state = shared.metrics.lock().unwrap().clone();
                if state.source != "whoop" {
                    state = MetricsState::none();
                }
                state.error = Some(e);
                publish(app, shared, state);
                return;
            }
        }
    }
}

/// Background task: poll every 15 minutes, or immediately when kicked
/// (sign-in, sign-out, manual refresh, wake from sleep).
pub async fn poll_loop(app: AppHandle, shared: Arc<Shared>) {
    loop {
        poll_once(&app, &shared).await;
        tokio::select! {
            _ = sleep(POLL_INTERVAL) => {}
            _ = shared.poll_kick.notified() => {}
        }
    }
}
