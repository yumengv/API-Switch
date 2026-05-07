/**
 * Event Abstraction Layer Interface
 *
 * Provides a unified interface for desktop-only realtime events.
 * Page components use this abstraction instead of directly depending on Tauri event internals.
 *
 * Web fallback: All operations are no-ops when running outside Tauri runtime.
 * Future web implementations can replace the desktop manager with SSE/WebSocket/polling
 * without touching any page-level code.
 */

import { isTauriRuntime } from "./useApiAdapter";

// ============================================================================
// Event Types
// ============================================================================

/**
 * Supported event names for the application.
 * Extend this type when adding new events.
 */
export type EventName = "new-usage-log" | "entries-changed" | "channels-changed" | "channel-updated" | "settings-changed";

/**
 * Event handler callback type.
 */
export type EventHandler = () => void;

// ============================================================================
// Event Manager Interface
// ============================================================================

/**
 * Abstract event manager interface.
 * Implementations can vary by runtime environment (desktop vs web).
 */
export interface EventManager {
  /**
   * Subscribe to an event.
   * @param eventName The event name to listen for
   * @param callback Handler invoked when the event fires
   * @returns Unsubscribe function
   */
  subscribe(eventName: EventName, callback: EventHandler): () => void;

  /**
   * Check if the manager is active (e.g., running in Tauri runtime).
   */
  isActive(): boolean;
}

// ============================================================================
// Desktop Event Manager Implementation
// ============================================================================

class DesktopEventManager implements EventManager {
  private subscriptions = new Map<EventName, Set<EventHandler>>();
  private unlistenFns = new Map<EventName, () => void>();

  isActive(): boolean {
    return isTauriRuntime();
  }

  subscribe(eventName: EventName, callback: EventHandler): () => void {
    if (!this.isActive()) {
      // No-op when not in Tauri runtime
      return () => {};
    }

    // Initialize subscription set if needed
    if (!this.subscriptions.has(eventName)) {
      this.subscriptions.set(eventName, new Set());
    }
    this.subscriptions.get(eventName)!.add(callback);

    // Set up Tauri listener only once per event name
    if (!this.unlistenFns.has(eventName)) {
      import("@tauri-apps/api/event")
        .then(({ listen }) =>
          listen(eventName, () => {
            this.subscriptions.get(eventName)?.forEach((cb) => cb());
          })
        )
        .catch(() => {
          // Silently fail when event API is unavailable
        });
    }

    // Return unsubscribe function
    return () => {
      const handlers = this.subscriptions.get(eventName);
      if (handlers) {
        handlers.delete(callback);
        // Clean up Tauri listener if no more subscribers
        if (handlers.size === 0) {
          this.subscriptions.delete(eventName);
          const unlisten = this.unlistenFns.get(eventName);
          if (unlisten) {
            unlisten();
            this.unlistenFns.delete(eventName);
          }
        }
      }
    };
  }
}

// ============================================================================
// Singleton Event Manager
// ============================================================================

let eventManagerInstance: EventManager | null = null;

function getEventManager(): EventManager {
  if (!eventManagerInstance) {
    eventManagerInstance = isTauriRuntime()
      ? new DesktopEventManager()
      : {
          subscribe: () => () => {},
          isActive: () => false,
        };
  }
  return eventManagerInstance;
}

// ============================================================================
// Public API (Hook-free version for flexibility)
// ============================================================================

/**
 * Subscribe to an event. Returns an unsubscribe function.
 * @param eventName The event name to listen for
 * @param callback Handler invoked when the event fires
 * @returns Unsubscribe function
 */
export function onEvent(eventName: EventName, callback: EventHandler): () => void {
  return getEventManager().subscribe(eventName, callback);
}

/**
 * Check if event system is active (running in Tauri).
 */
export function isEventSystemActive(): boolean {
  return getEventManager().isActive();
}

// ============================================================================
// React Hook Wrapper
// ============================================================================

import { useEffect } from "react";

/**
 * React hook for subscribing to events.
 * Automatically handles subscription lifecycle.
 *
 * @param eventName The event name to listen for
 * @param callback Handler invoked when the event fires
 */
export function useEvent(eventName: EventName, callback: EventHandler): void {
  useEffect(() => {
    const unsubscribe = onEvent(eventName, callback);
    return unsubscribe;
  }, [eventName, callback]);
}
