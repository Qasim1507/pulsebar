// Browser-only stand-in for the Rust backend (see api.ts). The real demo
// mode lives in src-tauri/src/demo.rs; this mirrors it closely enough to
// design against.
import type { Device, Sample, Snapshot, Tick } from "./types";

const handlers = new Map<string, Set<(payload: unknown) => void>>();
const emit = (event: string, payload: unknown) => handlers.get(event)?.forEach((h) => h(payload));

const state: Snapshot = {
  settings: {
    clientId: "",
    hasSecret: false,
    deviceId: null,
    deviceName: null,
    maxHr: 190,
    demoMode: true,
    stressNotify: true,
    redirectUri: "http://localhost:8765/callback",
  },
  auth: { status: "none", detail: null },
  metrics: {
    data: { recoveryPct: 84, hrvMs: 62.4, rhr: 54, sleepPct: 91, strain: 8.7 },
    source: "demo",
    updatedAt: Date.now(),
    error: null,
  },
  ble: { status: "idle", detail: null },
  history: [],
  autostart: false,
};

let step = 0;
let value = 66;
function nextBpm(): number {
  step += 1;
  const spike = step % 180 >= 100 && step % 180 < 140 ? 18 : 0;
  const target = 66 + 4 * Math.sin(step / 37) + 2 * Math.sin(step / 11) + spike;
  value += (target - value) * 0.18 + (Math.random() - 0.5) * 1.6;
  return Math.round(value);
}

// Pre-fill five minutes so the chart isn't empty on first paint.
const start = Date.now();
for (let i = 300; i > 0; i--) state.history.push([start - i * 1000, nextBpm()]);

setInterval(() => {
  const t = Date.now();
  const bpm = state.settings.demoMode ? nextBpm() : null;
  if (bpm !== null) state.history.push([t, bpm]);
  state.history = state.history.filter(([ts]) => ts >= t - 300_000);
  const recent = state.history.slice(-30).map(([, b]) => b);
  const avg = recent.reduce((a, b) => a + b, 0) / Math.max(recent.length, 1);
  const rhr = state.metrics.data?.rhr;
  const tick: Tick = {
    t,
    bpm,
    rrMs: null,
    source: bpm === null ? "none" : "demo",
    ble: state.ble,
    stress: bpm !== null && rhr != null && avg > rhr + 20,
  };
  emit("tick", tick);
}, 1000);

const snapshot = (): Snapshot => structuredClone(state);
const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

export async function mockCall<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  switch (command) {
    case "get_snapshot":
      return snapshot() as T;
    case "save_settings": {
      const s = state.settings;
      s.clientId = String(args.clientId ?? "");
      s.hasSecret = s.hasSecret || Boolean(args.clientSecret);
      s.maxHr = Number(args.maxHr);
      s.demoMode = Boolean(args.demoMode);
      state.metrics = s.demoMode
        ? { ...state.metrics, source: "demo", updatedAt: Date.now() }
        : { data: null, source: "none", updatedAt: null, error: null };
      if (s.demoMode && !state.metrics.data) {
        state.metrics.data = { recoveryPct: 84, hrvMs: 62.4, rhr: 54, sleepPct: 91, strain: 8.7 };
      }
      emit("metrics", state.metrics);
      return snapshot() as T;
    }
    case "scan_devices":
      await wait(1500);
      return [
        { id: "mock-whoop", name: "WHOOP 4C0123456", rssi: -58 },
        { id: "mock-strap", name: "Heart rate sensor", rssi: -74 },
      ] satisfies Device[] as T;
    case "start_oauth":
      throw "WHOOP sign-in only works inside the desktop app";
    case "set_stress_notify":
      state.settings.stressNotify = Boolean(args.enabled);
      return snapshot() as T;
    case "set_autostart":
      state.autostart = Boolean(args.enabled);
      return snapshot() as T;
    case "select_device":
    case "sign_out":
      return snapshot() as T;
    default:
      return undefined as T;
  }
}

export function mockOn(event: string, handler: (payload: unknown) => void): () => void {
  if (!handlers.has(event)) handlers.set(event, new Set());
  handlers.get(event)!.add(handler);
  return () => handlers.get(event)?.delete(handler);
}

export type { Sample };
