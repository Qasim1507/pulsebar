mod ble;
mod demo;
mod history;
mod hr;
mod settings;
mod whoop;

use serde::Serialize;
use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, State, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;
use tokio::sync::Notify;

const HISTORY_SECS: usize = 300;
/// A BLE reading older than this is no longer shown as live.
const LIVE_STALE: Duration = Duration::from_secs(5);
const STRESS_MARGIN_BPM: f64 = 20.0;
const STRESS_WINDOW: usize = 30;
/// At most one stress notification per this long.
const STRESS_NOTIFY_COOLDOWN: Duration = Duration::from_secs(15 * 60);

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Latest reading from the strap.
#[derive(Default)]
pub struct Live {
    bpm: Option<u16>,
    rr_ms: Option<u16>,
    at: Option<Instant>,
}

impl Live {
    pub fn set(&mut self, bpm: u16, rr_ms: Option<u16>) {
        self.bpm = Some(bpm);
        self.rr_ms = rr_ms;
        self.at = Some(Instant::now());
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn fresh(&self) -> Option<(u16, Option<u16>)> {
        match (self.bpm, self.at) {
            (Some(bpm), Some(at)) if at.elapsed() < LIVE_STALE => Some((bpm, self.rr_ms)),
            _ => None,
        }
    }
}

pub struct Shared {
    settings_path: PathBuf,
    pub settings: Mutex<settings::Settings>,
    pub client_secret: Mutex<Option<String>>,
    pub tokens: tokio::sync::Mutex<Option<whoop::Tokens>>,
    pub auth: Mutex<whoop::AuthState>,
    pub metrics: Mutex<whoop::MetricsState>,
    pub live: Mutex<Live>,
    pub ble: Mutex<ble::BleState>,
    history: Mutex<VecDeque<(i64, u16)>>,
    /// Persistent day history; `None` if the database could not be opened.
    db: Option<history::Db>,
    pub ble_kick: Notify,
    pub poll_kick: Notify,
    oauth_busy: AtomicBool,
    last_hidden: Mutex<Option<Instant>>,
    pub http: reqwest::Client,
}

type SharedState<'a> = State<'a, Arc<Shared>>;

// ------------------------------------------------------------ Commands ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SettingsView {
    client_id: String,
    has_secret: bool,
    device_id: Option<String>,
    device_name: Option<String>,
    max_hr: u16,
    demo_mode: bool,
    stress_notify: bool,
    redirect_uri: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Snapshot {
    settings: SettingsView,
    auth: whoop::AuthState,
    metrics: whoop::MetricsState,
    ble: ble::BleState,
    history: Vec<(i64, u16)>,
    autostart: bool,
}

fn snapshot(app: &AppHandle, shared: &Shared) -> Snapshot {
    let s = shared.settings.lock().unwrap().clone();
    Snapshot {
        settings: SettingsView {
            client_id: s.client_id,
            has_secret: shared.client_secret.lock().unwrap().is_some(),
            device_id: s.device_id,
            device_name: s.device_name,
            max_hr: s.max_hr,
            demo_mode: s.demo_mode,
            stress_notify: s.stress_notify,
            redirect_uri: whoop::REDIRECT_URI,
        },
        auth: shared.auth.lock().unwrap().clone(),
        metrics: shared.metrics.lock().unwrap().clone(),
        ble: shared.ble.lock().unwrap().clone(),
        history: shared.history.lock().unwrap().iter().copied().collect(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
    }
}

fn persist(shared: &Shared) -> Result<(), String> {
    let s = shared.settings.lock().unwrap().clone();
    settings::save(&shared.settings_path, &s)
}

async fn clear_session(app: &AppHandle, shared: &Shared) {
    *shared.tokens.lock().await = None;
    settings::secret_delete(settings::KEY_TOKENS).await;
    whoop::set_auth(app, shared, "none", None);
    shared.poll_kick.notify_one();
}

#[tauri::command]
async fn get_snapshot(app: AppHandle, shared: SharedState<'_>) -> Result<Snapshot, String> {
    Ok(snapshot(&app, &shared))
}

#[tauri::command]
async fn save_settings(
    app: AppHandle,
    shared: SharedState<'_>,
    client_id: String,
    client_secret: Option<String>,
    max_hr: u16,
    demo_mode: bool,
) -> Result<Snapshot, String> {
    let client_id = client_id.trim().to_string();
    let secret = client_secret.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());

    let credentials_changed = {
        let mut s = shared.settings.lock().unwrap();
        let changed = s.client_id != client_id || secret.is_some();
        s.client_id = client_id;
        s.max_hr = max_hr.clamp(120, 230);
        s.demo_mode = demo_mode;
        changed
    };
    persist(&shared)?;

    if let Some(secret) = secret {
        settings::secret_set(settings::KEY_CLIENT_SECRET, secret.clone()).await?;
        *shared.client_secret.lock().unwrap() = Some(secret);
    }
    // Tokens belong to the old client; a different one needs a fresh sign-in.
    if credentials_changed && shared.tokens.lock().await.is_some() {
        clear_session(&app, &shared).await;
    }
    shared.poll_kick.notify_one();
    Ok(snapshot(&app, &shared))
}

#[tauri::command]
async fn start_oauth(app: AppHandle, shared: SharedState<'_>) -> Result<(), String> {
    let client_id = shared.settings.lock().unwrap().client_id.clone();
    let secret = shared.client_secret.lock().unwrap().clone();
    let (false, Some(secret)) = (client_id.is_empty(), secret) else {
        return Err("Enter and save your WHOOP Client ID and Secret first".into());
    };
    if shared.oauth_busy.swap(true, Ordering::SeqCst) {
        return Err("Sign-in is already in progress — finish it in your browser".into());
    }

    whoop::set_auth(&app, &shared, "pending", None);
    let result = async {
        let tokens = whoop::authorize(&app, &client_id, &secret).await?;
        whoop::store_tokens(&shared, &tokens).await
    }
    .await;
    shared.oauth_busy.store(false, Ordering::SeqCst);

    match &result {
        Ok(()) => {
            whoop::set_auth(&app, &shared, "connected", None);
            shared.poll_kick.notify_one();
        }
        Err(e) => whoop::set_auth(&app, &shared, "error", Some(e.clone())),
    }
    result
}

#[tauri::command]
async fn sign_out(app: AppHandle, shared: SharedState<'_>) -> Result<Snapshot, String> {
    clear_session(&app, &shared).await;
    Ok(snapshot(&app, &shared))
}

#[tauri::command]
async fn scan_devices() -> Result<Vec<ble::Device>, String> {
    ble::scan(Duration::from_secs(5)).await
}

#[tauri::command]
async fn select_device(
    app: AppHandle,
    shared: SharedState<'_>,
    id: Option<String>,
    name: Option<String>,
) -> Result<Snapshot, String> {
    {
        let mut s = shared.settings.lock().unwrap();
        s.device_id = id;
        s.device_name = name;
    }
    persist(&shared)?;
    shared.history.lock().unwrap().clear();
    shared.ble_kick.notify_one();
    Ok(snapshot(&app, &shared))
}

fn notify(app: &AppHandle, title: &str, body: &str) -> Result<(), String> {
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|e| format!("Could not show a notification: {e}"))
}

/// Turn Desk Stress Alert notifications on or off. Turning them on sends a
/// confirmation, which also triggers the OS permission prompt up front.
#[tauri::command]
async fn set_stress_notify(app: AppHandle, shared: SharedState<'_>, enabled: bool) -> Result<Snapshot, String> {
    shared.settings.lock().unwrap().stress_notify = enabled;
    persist(&shared)?;
    if enabled {
        notify(&app, "Stress notifications are on", "PulseBar will tell you when your heart rate runs high at your desk.")?;
    }
    Ok(snapshot(&app, &shared))
}

/// Register or remove the login item that starts PulseBar when you sign in.
#[tauri::command]
async fn set_autostart(app: AppHandle, shared: SharedState<'_>, enabled: bool) -> Result<Snapshot, String> {
    let launcher = app.autolaunch();
    let result = if enabled { launcher.enable() } else { launcher.disable() };
    result.map_err(|e| format!("Could not change the login item: {e}"))?;
    Ok(snapshot(&app, &shared))
}

/// Minute-by-minute heart rate and stress episodes between two timestamps (ms).
#[tauri::command]
async fn get_history(shared: SharedState<'_>, from: i64, to: i64) -> Result<history::DayHistory, String> {
    match &shared.db {
        Some(db) => db.range(from, to),
        None => Err("History database is unavailable".into()),
    }
}

#[tauri::command]
async fn refresh_now(shared: SharedState<'_>) -> Result<(), String> {
    shared.poll_kick.notify_one();
    Ok(())
}

#[tauri::command]
fn quit(app: AppHandle) {
    app.exit(0);
}

// ---------------------------------------------------------- Live ticker ----

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Tick {
    t: i64,
    bpm: Option<u16>,
    rr_ms: Option<u16>,
    /// ble | demo | none
    source: &'static str,
    ble: ble::BleState,
    stress: bool,
}

/// Once a second: pick the current heart rate, record it, update the tray
/// title and push a `tick` event to the popover.
async fn tick_loop(app: AppHandle, shared: Arc<Shared>) {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut sim = demo::HrSim::new();
    let mut last_wall = now_ms();
    let mut last_title = String::new();
    let mut was_stressed = false;
    let mut last_notified: Option<Instant> = None;
    // Day history: 5-second averages of real strap data, plus stress episodes.
    let (mut acc_sum, mut acc_n, mut acc_started) = (0u32, 0u32, now_ms());
    let mut episode: Option<(i64, u16)> = None;
    let mut last_prune = 0i64;

    loop {
        interval.tick().await;
        let now = now_ms();

        // A jump in wall-clock time means the laptop was asleep: reconnect
        // the strap and refresh WHOOP data straight away.
        if now - last_wall > 10_000 {
            shared.ble_kick.notify_one();
            shared.poll_kick.notify_one();
        }
        last_wall = now;

        let (has_device, demo_mode, stress_notify) = {
            let s = shared.settings.lock().unwrap();
            (s.device_id.is_some(), s.demo_mode, s.stress_notify)
        };
        let (bpm, rr_ms, source) = if has_device {
            match shared.live.lock().unwrap().fresh() {
                Some((bpm, rr)) => (Some(bpm), rr, "ble"),
                None => (None, None, "ble"),
            }
        } else if demo_mode {
            (Some(sim.next()), None, "demo")
        } else {
            (None, None, "none")
        };

        let (recovery, rhr, real_metrics) = {
            let m = shared.metrics.lock().unwrap();
            let data = m.data.as_ref();
            (data.and_then(|d| d.recovery_pct), data.and_then(|d| d.rhr), m.source == "whoop")
        };

        let stress = {
            let mut history = shared.history.lock().unwrap();
            if let Some(bpm) = bpm {
                history.push_back((now, bpm));
            }
            let cutoff = now - (HISTORY_SECS as i64) * 1000;
            while history.front().is_some_and(|(t, _)| *t < cutoff) {
                history.pop_front();
            }
            // Averaged over 30 s so a single stray beat doesn't trip the alert.
            let recent: Vec<f64> = history
                .iter()
                .rev()
                .take_while(|(t, _)| *t >= now - (STRESS_WINDOW as i64) * 1000)
                .map(|(_, b)| *b as f64)
                .collect();
            match rhr {
                Some(rhr) if bpm.is_some() && recent.len() >= STRESS_WINDOW / 2 => {
                    recent.iter().sum::<f64>() / recent.len() as f64 > rhr + STRESS_MARGIN_BPM
                }
                _ => false,
            }
        };

        // Notify when the alert starts, but only on real data (strap + WHOOP
        // resting HR) and not more often than the cooldown.
        let real_stress = stress && source == "ble" && real_metrics;
        if real_stress && !was_stressed && stress_notify {
            let cooled = last_notified.is_none_or(|t| t.elapsed() >= STRESS_NOTIFY_COOLDOWN);
            if let (true, Some(bpm), Some(rhr)) = (cooled, bpm, rhr) {
                let body = format!(
                    "Heart rate is {bpm} bpm, more than 20 above your resting {rhr:.0}. Take a breath or a short walk."
                );
                if notify(&app, "Desk Stress Alert", &body).is_ok() {
                    last_notified = Some(Instant::now());
                }
            }
        }
        was_stressed = real_stress;

        if let Some(db) = &shared.db {
            if let (Some(bpm), "ble") = (bpm, source) {
                acc_sum += bpm as u32;
                acc_n += 1;
            }
            if now - acc_started >= 5_000 {
                if acc_n > 0 {
                    db.add_sample(now, (acc_sum as f64 / acc_n as f64).round() as u16);
                }
                (acc_sum, acc_n, acc_started) = (0, 0, now);
            }
            if real_stress {
                let current = episode.get_or_insert((now, 0));
                current.1 = current.1.max(bpm.unwrap_or(0));
            } else if let Some((start, peak)) = episode.take() {
                db.add_episode(start, now, peak);
            }
            if now - last_prune >= 3_600_000 {
                db.prune(now);
                last_prune = now;
            }
        }

        let hr_part = bpm.map_or("-- bpm".to_string(), |b| format!("{b} bpm"));
        let title = match recovery {
            Some(r) => format!("{hr_part} • {r:.0}%"),
            None => hr_part,
        };
        if title != last_title {
            if let Some(tray) = app.tray_by_id("main") {
                // The title shows beside the icon on macOS; Windows has no
                // tray text, so the same string goes in the tooltip.
                let _ = tray.set_title(Some(&title));
                let _ = tray.set_tooltip(Some(&format!("PulseBar — {title}")));
            }
            last_title = title;
        }

        let _ = app.emit(
            "tick",
            Tick { t: now, bpm, rr_ms, source, ble: shared.ble.lock().unwrap().clone(), stress },
        );
    }
}

// -------------------------------------------------------------- Popover ----

fn toggle_popover(app: &AppHandle, icon: tauri::Rect) {
    let Some(window) = app.get_webview_window("main") else { return };
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    // Clicking the tray icon while the popover is open first blurs (and so
    // hides) it; don't immediately reopen it on the same click.
    let shared = app.state::<Arc<Shared>>();
    let just_hidden = shared
        .last_hidden
        .lock()
        .unwrap()
        .is_some_and(|t| t.elapsed() < Duration::from_millis(250));
    if just_hidden {
        return;
    }

    let scale = window.scale_factor().unwrap_or(1.0);
    let icon_pos: PhysicalPosition<f64> = icon.position.to_physical(scale);
    let icon_size: PhysicalSize<f64> = icon.size.to_physical(scale);
    let size = window.outer_size().unwrap_or(PhysicalSize::new(360, 620));
    let (w, h) = (size.width as f64, size.height as f64);
    let gap = 6.0 * scale;

    let mut x = icon_pos.x + icon_size.width / 2.0 - w / 2.0;
    // Menu bar is at the top on macOS; the Windows taskbar is usually at the bottom.
    let mut y = if cfg!(target_os = "macos") {
        icon_pos.y + icon_size.height + gap
    } else {
        icon_pos.y - h - gap
    };
    if let Ok(Some(monitor)) = app.monitor_from_point(icon_pos.x, icon_pos.y) {
        let (mx, my) = (monitor.position().x as f64, monitor.position().y as f64);
        let (mw, mh) = (monitor.size().width as f64, monitor.size().height as f64);
        if y < my {
            y = icon_pos.y + icon_size.height + gap;
        }
        x = x.clamp(mx + gap, (mx + mw - w - gap).max(mx + gap));
        y = y.clamp(my, (my + mh - h - gap).max(my));
    }

    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    let _ = window.show();
    let _ = window.set_focus();
}

// ----------------------------------------------------------------- Run ----

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_snapshot,
            save_settings,
            start_oauth,
            sign_out,
            scan_devices,
            select_device,
            refresh_now,
            get_history,
            set_autostart,
            set_stress_notify,
            quit
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::Focused(false) = event {
                let _ = window.hide();
                let shared = window.state::<Arc<Shared>>();
                *shared.last_hidden.lock().unwrap() = Some(Instant::now());
            }
        })
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let shared = Arc::new(Shared {
                settings: Mutex::new(settings::load(&settings_path)),
                settings_path,
                client_secret: Mutex::new(None),
                tokens: tokio::sync::Mutex::new(None),
                auth: Mutex::new(whoop::AuthState::new("none", None)),
                metrics: Mutex::new(whoop::MetricsState::none()),
                live: Mutex::new(Live::default()),
                ble: Mutex::new(ble::BleState::idle()),
                history: Mutex::new(VecDeque::with_capacity(HISTORY_SECS + 8)),
                db: history::Db::open(&app.path().app_data_dir()?.join("history.db")).ok(),
                ble_kick: Notify::new(),
                poll_kick: Notify::new(),
                oauth_busy: AtomicBool::new(false),
                last_hidden: Mutex::new(None),
                http: reqwest::Client::new(),
            });
            app.manage(shared.clone());

            // Tray icon: left click toggles the popover, right click shows a menu.
            let quit_item = MenuItem::with_id(app, "quit", "Quit PulseBar", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&quit_item])?;
            let icon_bytes: &[u8] = if cfg!(target_os = "macos") {
                include_bytes!("../icons/tray.png")
            } else {
                include_bytes!("../icons/tray-color.png")
            };
            TrayIconBuilder::with_id("main")
                .icon(tauri::image::Image::from_bytes(icon_bytes)?)
                .icon_as_template(cfg!(target_os = "macos"))
                .tooltip("PulseBar")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    if event.id.as_ref() == "quit" {
                        app.exit(0);
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        rect,
                        ..
                    } = event
                    {
                        toggle_popover(tray.app_handle(), rect);
                    }
                })
                .build(app)?;

            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_visible_on_all_workspaces(true);
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                // Only touch the credential store once WHOOP keys exist, so
                // demo mode never triggers a keychain prompt.
                let has_client = !shared.settings.lock().unwrap().client_id.is_empty();
                if has_client {
                    let secret = settings::secret_get(settings::KEY_CLIENT_SECRET).await;
                    *shared.client_secret.lock().unwrap() = secret;
                    let tokens = settings::secret_get(settings::KEY_TOKENS)
                        .await
                        .and_then(|json| serde_json::from_str::<whoop::Tokens>(&json).ok());
                    if tokens.is_some() {
                        *shared.tokens.lock().await = tokens;
                        whoop::set_auth(&handle, &shared, "connected", None);
                    }
                }
                tokio::spawn(ble::run_loop(shared.clone()));
                tokio::spawn(whoop::poll_loop(handle.clone(), shared.clone()));
                tick_loop(handle, shared).await;
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running PulseBar");
}
