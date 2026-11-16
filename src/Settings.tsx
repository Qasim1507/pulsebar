import { useState } from "react";
import { call } from "./api";
import type { BleState, Device, Snapshot } from "./types";

interface Props {
  snap: Snapshot;
  ble: BleState;
  onSnapshot: (s: Snapshot) => void;
}

const BLE_LABEL: Record<BleState["status"], string> = {
  idle: "Not connected",
  scanning: "Searching…",
  connecting: "Connecting…",
  connected: "Connected",
  reconnecting: "Reconnecting…",
};

export default function Settings({ snap, ble, onSnapshot }: Props) {
  const { settings, auth } = snap;
  const [clientId, setClientId] = useState(settings.clientId);
  const [clientSecret, setClientSecret] = useState("");
  const [maxHr, setMaxHr] = useState(String(settings.maxHr));
  const [restingHr, setRestingHr] = useState(settings.restingHr ? String(settings.restingHr) : "");
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [devices, setDevices] = useState<Device[] | null>(null);
  const [scanning, setScanning] = useState(false);
  const [scanError, setScanError] = useState<string | null>(null);
  const [prefError, setPrefError] = useState<string | null>(null);

  async function save(overrides: { demoMode?: boolean } = {}) {
    setError(null);
    try {
      const next = await call<Snapshot>("save_settings", {
        clientId,
        clientSecret: clientSecret || null,
        maxHr: Number(maxHr) || settings.maxHr,
        restingHr: Number(restingHr) || null,
        demoMode: overrides.demoMode ?? settings.demoMode,
      });
      onSnapshot(next);
      setClientSecret("");
      setMaxHr(String(next.settings.maxHr));
      setRestingHr(next.settings.restingHr ? String(next.settings.restingHr) : "");
      setSaved(true);
      setTimeout(() => setSaved(false), 1500);
      return next;
    } catch (e) {
      setError(String(e));
      return null;
    }
  }

  async function connect() {
    // Save first so freshly typed credentials are the ones used.
    const next = await save();
    if (!next) return;
    try {
      await call("start_oauth");
    } catch (e) {
      setError(String(e));
    }
  }

  async function scan() {
    setScanning(true);
    setScanError(null);
    try {
      setDevices(await call<Device[]>("scan_devices"));
    } catch (e) {
      setScanError(String(e));
    } finally {
      setScanning(false);
    }
  }

  async function pick(device: Device | null) {
    onSnapshot(await call<Snapshot>("select_device", { id: device?.id ?? null, name: device?.name ?? null }));
    setDevices(null);
  }

  // With a shared WHOOP app built in, keys are only for people who prefer their own.
  const usingOwnKeys = clientId.trim() !== "";
  const canConnect = usingOwnKeys ? settings.hasSecret || clientSecret.trim() !== "" : settings.sharedApp;
  const dirty =
    clientId !== settings.clientId || clientSecret !== "" || Number(maxHr) !== settings.maxHr;

  return (
    <main className="content settings">
      <section className="card">
        <h2>
          WHOOP integration
          <span className={`chip chip-${auth.status}`}>
            {{ none: "Not connected", pending: "Waiting for browser…", connected: "Connected", error: "Needs attention" }[auth.status]}
          </span>
        </h2>

        {settings.sharedApp ? (
          <>
            <p className="hint">
              Sign in with your WHOOP account to show today's recovery, HRV, sleep and strain. Your browser opens once;
              after that it stays connected.
            </p>
            <details className="advanced" open={usingOwnKeys}>
              <summary>Use my own WHOOP developer keys</summary>
          <label>
            Client ID
            <input
              value={clientId}
              onChange={(e) => setClientId(e.target.value)}
              placeholder="From developer-dashboard.whoop.com"
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <label>
            Client Secret
            <input
              type="password"
              value={clientSecret}
              onChange={(e) => setClientSecret(e.target.value)}
              placeholder={settings.hasSecret ? "Saved — type to replace" : "Stored in your system keychain"}
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <p className="hint">
            Add this Redirect URL to your WHOOP app:
            <code onClick={() => navigator.clipboard?.writeText(settings.redirectUri)} title="Click to copy">
              {settings.redirectUri}
            </code>
          </p>

            </details>
          </>
        ) : (
          <>
          <label>
            Client ID
            <input
              value={clientId}
              onChange={(e) => setClientId(e.target.value)}
              placeholder="From developer-dashboard.whoop.com"
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <label>
            Client Secret
            <input
              type="password"
              value={clientSecret}
              onChange={(e) => setClientSecret(e.target.value)}
              placeholder={settings.hasSecret ? "Saved — type to replace" : "Stored in your system keychain"}
              spellCheck={false}
              autoComplete="off"
            />
          </label>
          <p className="hint">
            Add this Redirect URL to your WHOOP app:
            <code onClick={() => navigator.clipboard?.writeText(settings.redirectUri)} title="Click to copy">
              {settings.redirectUri}
            </code>
          </p>

          </>
        )}

        {(error || (auth.status === "error" && auth.detail)) && <p className="error">{error ?? auth.detail}</p>}

        <div className="row">
          <button className="btn primary" onClick={connect} disabled={!canConnect || auth.status === "pending"}>
            {auth.status === "connected" ? "Re-authenticate" : auth.status === "pending" ? "Check your browser…" : "Connect WHOOP"}
          </button>
          {auth.status === "connected" && (
            <button className="btn" onClick={async () => onSnapshot(await call<Snapshot>("sign_out"))}>
              Sign out
            </button>
          )}
          {dirty && (
            <button className="btn" onClick={() => save()}>
              Save
            </button>
          )}
          {saved && <span className="saved">Saved</span>}
        </div>
      </section>

      <section className="card">
        <h2>
          Strap (Bluetooth)
          {settings.deviceId && (
            <span className={`chip chip-${ble.status === "connected" ? "connected" : "none"}`}>{BLE_LABEL[ble.status]}</span>
          )}
        </h2>
        {settings.deviceId ? (
          <div className="device current">
            <div>
              <strong>{settings.deviceName ?? "Heart rate sensor"}</strong>
              <span>{ble.status === "reconnecting" && ble.detail ? ble.detail : "Reconnects automatically"}</span>
            </div>
            <button className="btn" onClick={() => pick(null)}>
              Forget
            </button>
          </div>
        ) : (
          <p className="hint">
            Works with any Bluetooth heart rate strap or armband — Polar, Garmin, Wahoo, Coros, WHOOP and others.
            Put it on, then scan. On a WHOOP, turn on <em>HR Broadcast</em> in the app's device settings first; on a
            Garmin watch, start <em>Broadcast Heart Rate</em>.
          </p>
        )}

        <div className="row">
          <button className="btn" onClick={scan} disabled={scanning}>
            {scanning ? "Scanning…" : settings.deviceId ? "Scan for another" : "Scan for straps"}
          </button>
        </div>
        {scanError && <p className="error">{scanError}</p>}
        {devices && devices.length === 0 && <p className="hint">No heart rate devices found nearby.</p>}
        {devices?.map((d) => (
          <button key={d.id} className="device pick" onClick={() => pick(d)}>
            <div>
              <strong>{d.name}</strong>
              <span>{d.rssi !== null ? `${d.rssi} dBm` : "Signal unknown"}</span>
            </div>
            <span className="pick-cta">{d.id === settings.deviceId ? "Selected" : "Use"}</span>
          </button>
        ))}
      </section>

      <section className="card">
        <h2>Preferences</h2>
        <label className="inline">
          <span>
            Max heart rate
            <small>Sets the HR zone badge</small>
          </span>
          <input
            type="number"
            min={120}
            max={230}
            value={maxHr}
            onChange={(e) => setMaxHr(e.target.value)}
            onBlur={() => Number(maxHr) !== settings.maxHr && save()}
          />
        </label>
        <label className="inline">
          <span>
            Resting heart rate
            <small>
              {auth.status === "connected"
                ? "WHOOP's value is used while connected"
                : "For the stress alert: your pulse sitting calmly"}
            </small>
          </span>
          <input
            type="number"
            min={30}
            max={120}
            placeholder="e.g. 60"
            value={restingHr}
            onChange={(e) => setRestingHr(e.target.value)}
            onBlur={() => (Number(restingHr) || null) !== settings.restingHr && save()}
          />
        </label>
        <label className="inline">
          <span>
            Stress notifications
            <small>Alert me when desk stress starts (max one per 15 min)</small>
          </span>
          <input
            type="checkbox"
            className="switch"
            checked={settings.stressNotify}
            onChange={async (e) => {
              setPrefError(null);
              try {
                onSnapshot(await call<Snapshot>("set_stress_notify", { enabled: e.target.checked }));
              } catch (err) {
                setPrefError(String(err));
              }
            }}
          />
        </label>
        <label className="inline">
          <span>
            Start at login
            <small>Open PulseBar when you sign in</small>
          </span>
          <input
            type="checkbox"
            className="switch"
            checked={snap.autostart}
            onChange={async (e) => {
              setPrefError(null);
              try {
                onSnapshot(await call<Snapshot>("set_autostart", { enabled: e.target.checked }));
              } catch (err) {
                setPrefError(String(err));
              }
            }}
          />
        </label>
        <label className="inline">
          <span>
            Demo mode
            <small>Simulate anything that isn't connected yet</small>
          </span>
          <input
            type="checkbox"
            className="switch"
            checked={settings.demoMode}
            onChange={(e) => save({ demoMode: e.target.checked })}
          />
        </label>
        {prefError && <p className="error pref-error">{prefError}</p>}
      </section>

      <button className="btn quit" onClick={() => call("quit")}>
        Quit PulseBar
      </button>
    </main>
  );
}
