import { useEffect, useState } from "react";
import { call } from "./api";
import Sparkline from "./Sparkline";
import type { BleState, Sample, Snapshot, Tick } from "./types";

interface Zone {
  label: string;
  name: string;
  color: string;
}

function zoneFor(bpm: number, maxHr: number): Zone {
  const pct = bpm / maxHr;
  if (pct >= 0.9) return { label: "Zone 5", name: "Max", color: "#ff4d5e" };
  if (pct >= 0.8) return { label: "Zone 4", name: "Hard", color: "#ff9a3d" };
  if (pct >= 0.7) return { label: "Zone 3", name: "Moderate", color: "#3ddc84" };
  if (pct >= 0.6) return { label: "Zone 2", name: "Light", color: "#4da3ff" };
  if (pct >= 0.5) return { label: "Zone 1", name: "Very light", color: "#8fb4d9" };
  return { label: "Rest", name: "Below zone 1", color: "#9aa0aa" };
}

function recoveryColor(pct: number): string {
  if (pct >= 67) return "#3ddc84";
  if (pct >= 34) return "#ffd23d";
  return "#ff4d5e";
}

function ago(ms: number): string {
  const mins = Math.floor(ms / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  return `${Math.floor(mins / 60)} h ago`;
}

function hrStatus(source: Tick["source"] | undefined, ble: BleState, deviceName: string | null): string {
  if (source === "demo") return "Simulated heart rate";
  if (source === "none" || source === undefined) return "No strap selected";
  const name = deviceName ?? "strap";
  switch (ble.status) {
    case "connected":
      return `Live from ${name}`;
    case "scanning":
      return `Looking for ${name}…`;
    case "connecting":
      return `Connecting to ${name}…`;
    case "reconnecting":
      return ble.detail ? `${ble.detail} — retrying` : "Reconnecting…";
    default:
      return "Waiting for Bluetooth…";
  }
}

const fmt = (v: number | null | undefined, digits = 0) => (v == null ? "—" : v.toFixed(digits));

interface Props {
  snap: Snapshot;
  tick: Tick | null;
  history: Sample[];
  onOpenSettings: () => void;
}

export default function Dashboard({ snap, tick, history, onOpenSettings }: Props) {
  const { settings, metrics, auth } = snap;
  const m = metrics.data;
  const bpm = tick?.bpm ?? null;
  const zone = bpm !== null ? zoneFor(bpm, settings.maxHr) : null;
  const lineColor = zone?.color ?? "#9aa0aa";
  const threshold = m?.rhr != null ? m.rhr + 20 : null;
  const stress = tick?.stress ?? false;

  const values = history.map(([, b]) => b);
  const stats = values.length
    ? {
        min: Math.min(...values),
        avg: Math.round(values.reduce((a, b) => a + b, 0) / values.length),
        max: Math.max(...values),
      }
    : null;

  // Re-render the "updated … ago" label once a minute.
  const [, setMinute] = useState(0);
  useEffect(() => {
    const id = setInterval(() => setMinute((n) => n + 1), 60_000);
    return () => clearInterval(id);
  }, []);

  const recovery = m?.recoveryPct ?? null;
  const ringColor = recovery !== null ? recoveryColor(recovery) : "#3a3d45";
  const R = 34;
  const circumference = 2 * Math.PI * R;

  return (
    <main className="content">
      <section className={`card hero ${stress ? "hero-stress" : ""}`}>
        <div className="hero-top">
          <div className="bpm">
            <span className="bpm-value">{bpm ?? "--"}</span>
            <span className="bpm-unit">
              <span
                className={`heart ${bpm !== null ? "beating" : ""}`}
                style={bpm !== null ? { animationDuration: `${(60 / bpm).toFixed(2)}s` } : undefined}
              >
                ♥
              </span>
              bpm
            </span>
          </div>
          {zone && (
            <div className="zone" style={{ color: zone.color, borderColor: `${zone.color}55`, background: `${zone.color}14` }}>
              <strong>{zone.label}</strong>
              <span>{zone.name}</span>
            </div>
          )}
        </div>
        <div className="hero-status">{hrStatus(tick?.source, tick?.ble ?? snap.ble, settings.deviceName)}</div>

        <Sparkline history={history} now={tick?.t ?? Date.now()} color={lineColor} threshold={threshold} />
        <div className="spark-meta">
          <span>5 min</span>
          {stats && (
            <span>
              min {stats.min} · avg {stats.avg} · max {stats.max}
            </span>
          )}
          <span>now</span>
        </div>
      </section>

      <section className={`alert ${stress ? "alert-on" : ""}`} role="status">
        <span className="alert-icon">{stress ? "!" : "✓"}</span>
        <div>
          <strong>{stress ? "Desk Stress Alert" : "Desk stress: calm"}</strong>
          <span>
            {threshold === null
              ? "Needs your resting heart rate from WHOOP"
              : stress
                ? `Heart rate is above ${Math.round(threshold)} bpm (resting + 20) while seated`
                : `Alerts above ${Math.round(threshold)} bpm (resting ${Math.round(m!.rhr!)} + 20)`}
          </span>
        </div>
      </section>

      <section className="card recovery">
        <div className="ring">
          <svg viewBox="0 0 84 84">
            <circle cx="42" cy="42" r={R} className="ring-track" />
            <circle
              cx="42"
              cy="42"
              r={R}
              fill="none"
              stroke={ringColor}
              strokeWidth="7"
              strokeLinecap="round"
              strokeDasharray={circumference}
              strokeDashoffset={circumference * (1 - (recovery ?? 0) / 100)}
              transform="rotate(-90 42 42)"
            />
          </svg>
          <div className="ring-label">
            <strong style={{ color: recovery !== null ? ringColor : undefined }}>
              {fmt(recovery)}
              {recovery !== null && <small>%</small>}
            </strong>
            <span>Recovery</span>
          </div>
        </div>
        <dl className="stats">
          <div>
            <dt>HRV</dt>
            <dd>
              {fmt(m?.hrvMs)} <small>ms</small>
            </dd>
          </div>
          <div>
            <dt>Resting HR</dt>
            <dd>
              {fmt(m?.rhr)} <small>bpm</small>
            </dd>
          </div>
          <div>
            <dt>Sleep</dt>
            <dd>
              {fmt(m?.sleepPct)} <small>%</small>
            </dd>
          </div>
          <div>
            <dt>Day Strain</dt>
            <dd>
              {fmt(m?.strain, 1)} <small>/ 21</small>
            </dd>
          </div>
        </dl>
      </section>

      <footer className="foot">
        {metrics.source === "whoop" && metrics.updatedAt ? (
          <>
            <span className={metrics.error ? "warn" : ""}>
              {metrics.error ?? `WHOOP updated ${ago(Date.now() - metrics.updatedAt)}`}
            </span>
            <button className="link" onClick={() => call("refresh_now")}>
              Refresh
            </button>
          </>
        ) : metrics.source === "demo" ? (
          <>
            <span>Sample WHOOP data</span>
            <button className="link" onClick={onOpenSettings}>
              Connect WHOOP
            </button>
          </>
        ) : (
          <>
            <span className={metrics.error || auth.status === "error" ? "warn" : ""}>
              {metrics.error ?? auth.detail ?? "WHOOP not connected"}
            </span>
            <button className="link" onClick={onOpenSettings}>
              Settings
            </button>
          </>
        )}
      </footer>
    </main>
  );
}
