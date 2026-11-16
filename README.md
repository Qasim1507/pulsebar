# PulseBar

A menu-bar (macOS) / system-tray (Windows) widget that shows live heart rate
from any Bluetooth heart rate strap, and warns you when it runs high at your
desk. WHOOP owners can also connect their account to see today's recovery,
HRV, resting HR, sleep and strain. Built with Tauri 2 (Rust) and React.

## Install

Download the latest build from the
[Releases page](https://github.com/Qasim1507/pulsebar/releases/latest):

- **macOS** (Apple Silicon and Intel): open the `.dmg` and drag PulseBar to
  Applications.
- **Windows**: run the `-setup.exe` installer.

The builds are not code-signed, so the operating system warns on first launch:

- **macOS**: if it says the app "is damaged" or "cannot be opened", run
  `xattr -cr /Applications/PulseBar.app` in Terminal once, then open it again.
  (Or try to open it, then go to System Settings → Privacy & Security →
  **Open Anyway**.)
- **Windows**: on the SmartScreen dialog click **More info → Run anyway**.

Live heart rate, the stress alert and history need only a strap. The WHOOP
recovery card is optional and needs your own free WHOOP developer keys (see
below).

## Run from source

Requires Node 20+ and a Rust toolchain (`cargo` on your `PATH`).

```bash
npm install
npm run tauri dev      # run with hot reload
npm run tauri build    # produce an installable app
npm run dev            # UI only, in a browser, against a JS simulator
```

With nothing configured the app starts in **demo mode**: simulated heart rate
(including a spike every three minutes that trips the Desk Stress Alert) and
sample WHOOP numbers. Each channel switches to real data on its own as soon as
it is set up; turn demo mode off in Settings to show blanks instead.

## Connect a strap (live heart rate)

PulseBar works with anything that speaks the standard Bluetooth Heart Rate
Service: Polar, Garmin, Wahoo and Coros chest straps and armbands, WHOOP, and
most others. An Apple Watch does not broadcast heart rate this way.

1. Put the strap on so it is awake. Some wearables must be told to broadcast:
   - **WHOOP**: turn on **HR Broadcast** in the mobile app's device settings.
   - **Garmin watches**: start **Broadcast Heart Rate** on the watch.
2. In PulseBar: Settings → **Scan for straps** → pick yours.
3. If you are not connecting a WHOOP account, enter your **Resting heart
   rate** under Preferences so the Desk Stress Alert has a baseline.

Many straps allow only one or two Bluetooth connections at a time, so
disconnect the strap from other apps if it does not show up.

The peripheral ID is saved and the app reconnects by itself when the strap
comes back in range or the laptop wakes. macOS asks for Bluetooth permission
on the first scan.

## Optional: connect the WHOOP API (recovery, sleep, strain)

> Builds that have a shared WHOOP app configured (see `broker/`) show a single
> **Connect WHOOP** button and need none of the steps below.

1. Create an app at <https://developer-dashboard.whoop.com> with the scopes
   `read:recovery read:cycles read:sleep read:workout read:profile offline`
   and the redirect URL `http://localhost:8765/callback`.
2. In PulseBar: Settings → paste the Client ID and Secret → **Connect WHOOP**.
   Your browser opens once for consent; after that tokens refresh silently.

The Client Secret and OAuth tokens are stored in the macOS Keychain / Windows
Credential Manager, never on disk in plain text. Other settings live in
`settings.json` in the app's config directory.

## How it is put together

| File | Role |
| --- | --- |
| `src-tauri/src/ble.rs` | Scan, connect, subscribe to `0x2A37`, reconnect loop |
| `src-tauri/src/hr.rs` | Heart Rate Measurement parser (UINT8/16 BPM, R-R) + tests |
| `src-tauri/src/whoop.rs` | OAuth (auth code + PKCE, loopback listener), refresh, 15-min poll |
| `broker/` | Optional Cloudflare Worker that holds the WHOOP client secret so users need no developer keys |
| `src-tauri/src/lib.rs` | Tray icon/title, popover window, 1 Hz ticker, commands |
| `src-tauri/src/history.rs` | SQLite day history (5-second averages, stress episodes, 7-day retention) |
| `src-tauri/src/demo.rs` | Simulated heart rate and metrics |
| `src-tauri/src/settings.rs` | Settings file and credential-store access |
| `src/` | React popover: `Dashboard`, `Settings`, `Sparkline` |

Notes:

- The Desk Stress Alert fires when the 30-second average heart rate is above
  resting HR + 20 bpm. BLE carries no motion data, so "stationary" is assumed.
- A notification is sent when the Desk Stress Alert starts, at most once per
  15 minutes and only on real data (a strap, plus a resting HR from WHOOP or Settings). Toggle it in
  Settings, along with **Start at login**.
- Every push to `main` builds Windows (`.exe` installer) and macOS artifacts via
  GitHub Actions (`.github/workflows/build.yml`).
- The History tab shows the last 7 days from `history.db` in the app data
  directory (`~/Library/Application Support/com.pulsebar.widget/` or
  `%APPDATA%\com.pulsebar.widget\`). Only real strap data is recorded; older
  rows are deleted automatically and nothing is uploaded.
- Pushing a tag such as `v0.1.0` publishes the DMG and Windows installer on a
  GitHub Release.
- Windows has no tray text, so `68 bpm • 84%` appears in the tray tooltip there.
- HR zones are percentages of the max heart rate set in Settings (default 190).
