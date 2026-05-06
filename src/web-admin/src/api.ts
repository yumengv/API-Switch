import type { AppSettings, VersionedAppSettings } from "@/types";

// Centralized admin API prefix — matches Rust backend routes in src-tauri/src/admin/router.rs.
// To migrate away from /admin, change this single value (coordinated with backend + vite base).
const ADMIN_PREFIX = "/admin";

const TOKEN_KEY = "api-switch-web-admin-token";

export interface AuditLogItem {
  id: number;
  action: string;
  detail: string;
  created_at: number;
}

export interface LoginResponse {
  token: string;
  expires_at: number;
}

export interface AdminStatus {
  running: boolean;
  port: number;
}

export interface HealthResponse {
  ok: boolean;
}

export interface SettingsResponse {
  data: VersionedAppSettings;
  _version: number;
}

export interface UpdateSettingsResponse {
  ok: boolean;
  _version?: number;
  restart?: {
    admin_relocated: boolean;
    new_admin_base_url?: string | null;
    proxy_restarted: boolean;
  } | null;
}

interface AdminErrorEnvelope {
  error?: {
    code?: string;
    message?: string;
    details?: Record<string, unknown>;
    retry_after_seconds?: number;
    remaining_attempts?: number;
    locked_until?: number;
  };
}

export interface AdminHttpError extends Error {
  status?: number;
  code?: string;
  details?: Record<string, unknown>;
  retryAfterSeconds?: number;
  remainingAttempts?: number;
  lockedUntil?: number;
  isNetworkError: boolean;
  isAuthError: boolean;
  isTimeoutError: boolean;
  isRateLimitError: boolean;
}

function createAdminHttpError(status: number, fallbackMessage: string, bodyError?: AdminErrorEnvelope['error']): AdminHttpError {
  const instance = new Error(bodyError?.message || fallbackMessage) as AdminHttpError;
  instance.name = 'AdminHttpError';
  instance.status = status;
  instance.code = bodyError?.code;
  instance.details = bodyError?.details;
  instance.retryAfterSeconds = bodyError?.retry_after_seconds;
  instance.remainingAttempts = bodyError?.remaining_attempts;
  instance.lockedUntil = bodyError?.locked_until;
  // Unified error classification - matches webAdminApiAdapter.ts
  instance.isNetworkError = status === 0 || bodyError?.code === 'ENDPOINT_UNREACHABLE';
  instance.isAuthError = status === 401 || status === 403 
    || bodyError?.code === 'UNAUTHORIZED' 
    || bodyError?.code === 'FORBIDDEN'
    || bodyError?.code === 'INVALID_CREDENTIALS';
  instance.isTimeoutError = bodyError?.code === 'TIMEOUT' || bodyError?.code === 'ENDPOINT_UNREACHABLE';
  instance.isRateLimitError = status === 429 || bodyError?.code === 'RATE_LIMITED';
  return instance;
}

export function getToken(): string | null {
  return localStorage.getItem(TOKEN_KEY);
}

export function setToken(token: string) {
  localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken() {
  localStorage.removeItem(TOKEN_KEY);
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  headers.set("Accept", "application/json");

  if (init.body && !headers.has("Content-Type")) {
    headers.set("Content-Type", "application/json");
  }

  const token = getToken();
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }

  let response: Response;
  try {
    response = await fetch(path, { ...init, headers });
  } catch (cause) {
    const rawMessage = cause instanceof Error ? cause.message : String(cause);
    throw createAdminHttpError(0, rawMessage);
  }

  if (!response.ok) {
    let message = `HTTP ${response.status}`;
    let bodyError: AdminErrorEnvelope['error'];
    try {
      const body = (await response.json()) as AdminErrorEnvelope;
      bodyError = body.error;
      message = bodyError?.message || message;
    } catch {
      // Keep default message when response is not JSON.
    }
    throw createAdminHttpError(response.status, message, bodyError);
  }

  return response.json() as Promise<T>;
}

export async function login(username: string, password: string): Promise<LoginResponse> {
  return request<LoginResponse>(`${ADMIN_PREFIX}/login`, {
    method: "POST",
    body: JSON.stringify({ username, password }),
  });
}

export async function logout(): Promise<void> {
  await request<{ ok: boolean }>(`${ADMIN_PREFIX}/logout`, { method: "POST" });
  clearToken();
}

export async function getHealth(): Promise<HealthResponse> {
  return request<HealthResponse>(`${ADMIN_PREFIX}/health`);
}

export async function getStatus(): Promise<AdminStatus> {
  return request<AdminStatus>(`${ADMIN_PREFIX}/status`);
}

export async function getSettings(): Promise<SettingsResponse> {
  const response = await request<SettingsResponse>(`${ADMIN_PREFIX}/settings`);
  return {
    ...response,
    data: {
      ...response.data,
      _version: response._version,
    },
  };
}

export async function updateSettings(settings: VersionedAppSettings): Promise<UpdateSettingsResponse> {
  const completeSettings: VersionedAppSettings = {
    ...settings,
    _version: settings._version,
  };
  return request<UpdateSettingsResponse>(`${ADMIN_PREFIX}/settings`, {
    method: "PUT",
    body: JSON.stringify({
      data: completeSettings,
      _version: completeSettings._version,
    }),
  });
}

export async function getAuditLogs(): Promise<AuditLogItem[]> {
  return request<AuditLogItem[]>(`${ADMIN_PREFIX}/audit-logs`);
}
