import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { mockCall, mockOn } from "./mock";

// Outside the Tauri shell (plain `npm run dev` in a browser) the backend is
// replaced by a small in-page simulator so the UI can be worked on alone.
const inTauri = "__TAURI_INTERNALS__" in window;

export function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return inTauri ? invoke<T>(command, args) : mockCall<T>(command, args);
}

/** Subscribe to a backend event; returns an unsubscribe function. */
export function on<T>(event: string, handler: (payload: T) => void): () => void {
  if (!inTauri) return mockOn(event, handler as (p: unknown) => void);
  const pending = listen<T>(event, (e) => handler(e.payload));
  return () => void pending.then((unlisten) => unlisten());
}
