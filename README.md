# PulseBar

A menu-bar (macOS) / system-tray (Windows) widget that shows live heart rate
from a WHOOP strap over Bluetooth, plus today's recovery, HRV, resting HR,
sleep and strain from the WHOOP API. Built with Tauri 2 (Rust) and React.

## Run

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

1. In the WHOOP mobile app, turn on **HR Broadcast** in the device settings.
   Without it the strap does not advertise the standard Heart Rate Service.
2. In PulseBar: Settings → **Scan for straps** → pick your WHOOP.

The peripheral ID is saved and the app reconnects by itself when the strap
comes back in range or the laptop wakes. macOS asks for Bluetooth permission
on the first scan.

## Connect the WHOOP API (recovery, sleep, strain)

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
| `src-tauri/src/lib.rs` | Tray icon/title, popover window, 1 Hz ticker, commands |
| `src-tauri/src/demo.rs` | Simulated heart rate and metrics |
| `src-tauri/src/settings.rs` | Settings file and credential-store access |
| `src/` | React popover: `Dashboard`, `Settings`, `Sparkline` |

Notes:

- The Desk Stress Alert fires when the 30-second average heart rate is above
  resting HR + 20 bpm. BLE carries no motion data, so "stationary" is assumed.
- A notification is sent when the Desk Stress Alert starts, at most once per
  15 minutes and only on real data (strap + WHOOP resting HR). Toggle it in
  Settings, along with **Start at login**.
- Every push to `main` builds Windows (`.exe` installer) and macOS artifacts via
  GitHub Actions (`.github/workflows/build.yml`).
- Windows has no tray text, so `68 bpm • 84%` appears in the tray tooltip there.
- HR zones are percentages of the max heart rate set in Settings (default 190).
