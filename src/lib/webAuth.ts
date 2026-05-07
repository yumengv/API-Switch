/**
 * Web Admin HTTP helper — login, token, request.
 * Used by LoginScreen and shared across web runtime.
 */

const ADMIN_PREFIX = "/admin";
const TOKEN_KEY = "api-switch-web-admin-token";

export interface LoginResponse {
  token: string;
  expires_at: number;
}

export interface AdminHttpError extends Error {
  status?: number;
  code?: string;
  retryAfterSeconds?: number;
  remainingAttempts?: number;
  isNetworkError: boolean;
  isAuthError: boolean;
  isRateLimitError: boolean;
}

function createAdminHttpError(
  status: number,
  fallbackMessage: string,
  bodyError?: { code?: string; message?: string; retry_after_seconds?: number; remaining_attempts?: number }
): AdminHttpError {
  const instance = new Error(bodyError?.message || fallbackMessage) as AdminHttpError;
  instance.name = "AdminHttpError";
  instance.status = status;
  instance.code = bodyError?.code;
  instance.retryAfterSeconds = bodyError?.retry_after_seconds;
  instance.remainingAttempts = bodyError?.remaining_attempts;
  instance.isNetworkError = status === 0 || bodyError?.code === "ENDPOINT_UNREACHABLE";
  instance.isAuthError = status === 401 || status === 403
    || bodyError?.code === "UNAUTHORIZED"
    || bodyError?.code === "FORBIDDEN"
    || bodyError?.code === "INVALID_CREDENTIALS";
  instance.isRateLimitError = status === 429 || bodyError?.code === "RATE_LIMITED";
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

export async function login(username: string, password: string): Promise<LoginResponse> {
  const response = await fetch(`${ADMIN_PREFIX}/login`, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ username, password }),
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw createAdminHttpError(response.status, `HTTP ${response.status}`, body.error);
  }
  return response.json();
}

export async function logout(): Promise<void> {
  const token = getToken();
  if (token) {
    await fetch(`${ADMIN_PREFIX}/logout`, {
      method: "POST",
      headers: { Authorization: `Bearer ${token}` },
    }).catch(() => {});
  }
  clearToken();
}

export async function validateToken(): Promise<boolean> {
  const token = getToken();
  if (!token) return false;
  try {
    const response = await fetch(`${ADMIN_PREFIX}/status`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    return response.ok;
  } catch {
    return true; // network error → assume valid
  }
}
