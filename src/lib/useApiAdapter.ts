/**
 * Platform-agnostic API adapter selector.
 *
 * Returns the single unified adapter that handles both desktop (Tauri invoke)
 * and web (HTTP fetch) transparently at runtime.
 */
import { apiAdapter } from './unifiedApiAdapter';
import type { ApiAdapter } from './apiAdapter';

export function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') {
    return false;
  }

  const candidate = window as Window & {
    __TAURI__?: unknown;
    __TAURI_INTERNALS__?: unknown;
  };

  return typeof candidate.__TAURI__ !== 'undefined' || typeof candidate.__TAURI_INTERNALS__ !== 'undefined';
}

export function useApiAdapter(): ApiAdapter {
  return apiAdapter;
}
