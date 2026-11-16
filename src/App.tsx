import { useEffect, useState } from "react";
import { call, on } from "./api";
import Dashboard from "./Dashboard";
import History from "./History";
import Settings from "./Settings";
import type { AuthState, MetricsState, Sample, Snapshot, Tick } from "./types";

const WINDOW_MS = 5 * 60 * 1000;

export default function App() {
  const [tab, setTab] = useState<"today" | "history" | "settings">("today");
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [history, setHistory] = useState<Sample[]>([]);
  const [tick, setTick] = useState<Tick | null>(null);

  useEffect(() => {
    let alive = true;
    call<Snapshot>("get_snapshot").then((s) => {
      if (!alive) return;
      setSnap(s);
      setHistory(s.history);
    });

    const offs = [
      on<Tick>("tick", (t) => {
        setTick(t);
        setHistory((h) => {
          const cutoff = t.t - WINDOW_MS;
          const next = h.filter(([ts]) => ts >= cutoff && ts < t.t);
          if (t.bpm !== null) next.push([t.t, t.bpm]);
          return next;
        });
      }),
      on<MetricsState>("metrics", (metrics) => setSnap((s) => s && { ...s, metrics })),
      on<AuthState>("auth", (auth) => setSnap((s) => s && { ...s, auth })),
    ];
    return () => {
      alive = false;
      offs.forEach((off) => off());
    };
  }, []);

  if (!snap) return <div className="shell" />;

  const isDemo = tick?.source === "demo" || snap.metrics.source === "demo";

  return (
    <div className="shell">
      <header className="topbar">
        <div className="brand">
          <span className="brand-dot" />
          PulseBar
          {isDemo && <span className="chip chip-demo">Demo</span>}
        </div>
        <nav className="tabs">
          <button className={tab === "today" ? "active" : ""} onClick={() => setTab("today")}>
            Today
          </button>
          <button className={tab === "history" ? "active" : ""} onClick={() => setTab("history")}>
            History
          </button>
          <button className={tab === "settings" ? "active" : ""} onClick={() => setTab("settings")}>
            Settings
          </button>
        </nav>
      </header>

      {tab === "today" ? (
        <Dashboard snap={snap} tick={tick} history={history} onOpenSettings={() => setTab("settings")} />
      ) : tab === "history" ? (
        <History rhr={snap.metrics.data?.rhr ?? null} />
      ) : (
        <Settings
          snap={snap}
          ble={tick?.ble ?? snap.ble}
          onSnapshot={(s) => {
            setSnap(s);
            setHistory(s.history);
          }}
        />
      )}
    </div>
  );
}
