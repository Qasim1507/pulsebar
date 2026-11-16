//! Live heart rate over the standard BLE Heart Rate Service.
//!
//! A WHOOP strap only advertises this service while "HR Broadcast" is turned
//! on in the WHOOP mobile app.

use crate::{hr, Shared};
use btleplug::api::{bleuuid::uuid_from_u16, Central, Manager as _, Peripheral as _, ScanFilter};
use btleplug::platform::{Adapter, Manager, Peripheral};
use futures::StreamExt;
use serde::Serialize;
use std::{sync::Arc, time::Duration};
use tokio::{
    sync::OnceCell,
    time::{sleep, timeout, Instant},
};
use uuid::Uuid;

/// 0000180d-0000-1000-8000-00805f9b34fb
const HR_SERVICE: Uuid = uuid_from_u16(0x180D);
/// 00002a37-0000-1000-8000-00805f9b34fb
const HR_MEASUREMENT: Uuid = uuid_from_u16(0x2A37);

const FIND_TIMEOUT: Duration = Duration::from_secs(20);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// The strap notifies about once a second; this long without data means the
/// link is dead (out of range, or the laptop slept).
const SILENCE_TIMEOUT: Duration = Duration::from_secs(8);
const RETRY_DELAY: Duration = Duration::from_secs(3);

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BleState {
    /// idle | scanning | connecting | connected | reconnecting
    pub status: &'static str,
    pub detail: Option<String>,
}

impl BleState {
    pub fn idle() -> Self {
        Self { status: "idle", detail: None }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub id: String,
    pub name: String,
    pub rssi: Option<i16>,
}

// The adapter is created lazily so the OS Bluetooth permission prompt only
// appears once the user actually scans or has a saved device.
static ADAPTER: OnceCell<Adapter> = OnceCell::const_new();

async fn adapter() -> Result<Adapter, String> {
    ADAPTER
        .get_or_try_init(|| async {
            let manager = Manager::new().await.map_err(|e| format!("Bluetooth unavailable: {e}"))?;
            manager
                .adapters()
                .await
                .map_err(|e| format!("Bluetooth unavailable: {e}"))?
                .into_iter()
                .next()
                .ok_or_else(|| "No Bluetooth adapter found".to_string())
        })
        .await
        .cloned()
}

fn hr_filter() -> ScanFilter {
    ScanFilter { services: vec![HR_SERVICE] }
}

/// Scan for a few seconds and list peripherals advertising the Heart Rate Service.
pub async fn scan(duration: Duration) -> Result<Vec<Device>, String> {
    let adapter = adapter().await?;
    adapter
        .start_scan(hr_filter())
        .await
        .map_err(|e| format!("Scan failed: {e}"))?;
    sleep(duration).await;

    let mut devices = Vec::new();
    for p in adapter.peripherals().await.map_err(|e| e.to_string())? {
        let Ok(Some(props)) = p.properties().await else { continue };
        let name = props.local_name.unwrap_or_default();
        let is_hr = props.services.contains(&HR_SERVICE);
        if !is_hr && !name.to_uppercase().contains("WHOOP") {
            continue;
        }
        devices.push(Device {
            id: p.id().to_string(),
            name: if name.is_empty() { "Heart rate sensor".into() } else { name },
            rssi: props.rssi,
        });
    }
    // WHOOP straps first, then strongest signal.
    devices.sort_by_key(|d| {
        (!d.name.to_uppercase().contains("WHOOP"), -(d.rssi.unwrap_or(-127) as i32))
    });
    Ok(devices)
}

fn set_state(shared: &Shared, status: &'static str, detail: Option<String>) {
    *shared.ble.lock().unwrap() = BleState { status, detail };
}

/// Background task: keep a connection to the saved device alive forever.
/// `shared.ble_kick` restarts it immediately (device changed, laptop woke up).
pub async fn run_loop(shared: Arc<Shared>) {
    loop {
        let target = shared.settings.lock().unwrap().device_id.clone();
        let Some(id) = target else {
            set_state(&shared, "idle", None);
            shared.live.lock().unwrap().clear();
            shared.ble_kick.notified().await;
            continue;
        };

        let mut connected: Option<Peripheral> = None;
        let outcome = tokio::select! {
            r = session(&shared, &id, &mut connected) => Some(r),
            _ = shared.ble_kick.notified() => None,
        };
        if let Some(p) = connected.take() {
            let _ = timeout(Duration::from_secs(3), p.disconnect()).await;
        }
        shared.live.lock().unwrap().clear();

        match outcome {
            // Kicked: loop straight back round and re-read the settings.
            None | Some(Ok(())) => continue,
            Some(Err(reason)) => {
                set_state(&shared, "reconnecting", Some(reason));
                tokio::select! {
                    _ = sleep(RETRY_DELAY) => {}
                    _ = shared.ble_kick.notified() => {}
                }
            }
        }
    }
}

/// One scan → connect → subscribe → stream cycle. Only returns on failure.
async fn session(
    shared: &Shared,
    id: &str,
    connected: &mut Option<Peripheral>,
) -> Result<(), String> {
    let adapter = adapter().await?;

    set_state(shared, "scanning", None);
    adapter
        .start_scan(hr_filter())
        .await
        .map_err(|e| format!("Scan failed: {e}"))?;
    let deadline = Instant::now() + FIND_TIMEOUT;
    let peripheral = loop {
        let found = adapter
            .peripherals()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|p| p.id().to_string() == id);
        if let Some(p) = found {
            break p;
        }
        if Instant::now() > deadline {
            let _ = adapter.stop_scan().await;
            return Err("Strap not in range".into());
        }
        sleep(Duration::from_millis(500)).await;
    };
    let _ = adapter.stop_scan().await;

    set_state(shared, "connecting", None);
    timeout(CONNECT_TIMEOUT, peripheral.connect())
        .await
        .map_err(|_| "Connection timed out".to_string())?
        .map_err(|e| format!("Connection failed: {e}"))?;
    *connected = Some(peripheral.clone());

    peripheral
        .discover_services()
        .await
        .map_err(|e| format!("Service discovery failed: {e}"))?;
    let characteristic = peripheral
        .characteristics()
        .into_iter()
        .find(|c| c.uuid == HR_MEASUREMENT)
        .ok_or("Device has no Heart Rate Measurement characteristic")?;
    peripheral
        .subscribe(&characteristic)
        .await
        .map_err(|e| format!("Subscribe failed: {e}"))?;
    let mut notifications = peripheral.notifications().await.map_err(|e| e.to_string())?;

    set_state(shared, "connected", None);
    loop {
        match timeout(SILENCE_TIMEOUT, notifications.next()).await {
            Ok(Some(n)) if n.uuid == HR_MEASUREMENT => {
                if let Some(m) = hr::parse(&n.value) {
                    // A strap that is off-wrist reports 0.
                    if m.bpm > 0 {
                        shared.live.lock().unwrap().set(m.bpm, m.rr_ms.last().copied());
                    }
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => return Err("Strap disconnected".into()),
            Err(_) => return Err("No heart rate data — is HR Broadcast on?".into()),
        }
    }
}
