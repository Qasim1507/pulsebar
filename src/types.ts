// Mirrors the structs serialized by src-tauri/src/{lib,ble,whoop}.rs

export type Sample = [t: number, bpm: number];

export interface SettingsView {
  clientId: string;
  hasSecret: boolean;
  deviceId: string | null;
  deviceName: string | null;
  maxHr: number;
  demoMode: boolean;
  stressNotify: boolean;
  redirectUri: string;
}

export interface AuthState {
  status: "none" | "pending" | "connected" | "error";
  detail: string | null;
}

export interface Metrics {
  recoveryPct: number | null;
  hrvMs: number | null;
  rhr: number | null;
  sleepPct: number | null;
  strain: number | null;
}

export interface MetricsState {
  data: Metrics | null;
  source: "whoop" | "demo" | "none";
  updatedAt: number | null;
  error: string | null;
}

export interface BleState {
  status: "idle" | "scanning" | "connecting" | "connected" | "reconnecting";
  detail: string | null;
}

export interface Tick {
  t: number;
  bpm: number | null;
  rrMs: number | null;
  source: "ble" | "demo" | "none";
  ble: BleState;
  stress: boolean;
}

export interface Snapshot {
  settings: SettingsView;
  auth: AuthState;
  metrics: MetricsState;
  ble: BleState;
  history: Sample[];
  autostart: boolean;
}

export interface Device {
  id: string;
  name: string;
  rssi: number | null;
}
