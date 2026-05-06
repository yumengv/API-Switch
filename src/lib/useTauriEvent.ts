import { useEffect, useRef } from "react";
import { isTauriRuntime } from "./useApiAdapter";

/**
 * Subscribe to a Tauri desktop event.
 *
 * No-op when running outside Tauri (e.g. web translation path).
 * Future web implementations can replace this hook with SSE / WebSocket /
 * polling without touching any page-level code.
 *
 * @param eventName  Tauri event name (e.g. "new-usage-log")
 * @param callback   Handler invoked each time the event fires
 */
export function useTauriEvent(eventName: string, callback: () => void): void {
  // Keep a stable ref so the Tauri listener always calls the latest callback
  // without re-subscribing on every render.
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    if (!isTauriRuntime()) return;

    let cancelled = false;
    let unlistenPromise: Promise<() => void> | undefined;

    import("@tauri-apps/api/event")
      .then(({ listen }) => {
        if (cancelled) return;
        unlistenPromise = listen(eventName, () => {
          callbackRef.current();
        });
      })
      .catch(() => {
        // ignore when event API is unavailable (e.g. web build)
      });

    return () => {
      cancelled = true;
      void unlistenPromise?.then((unlisten) => unlisten()).catch(() => {});
    };
  }, [eventName]);
}
