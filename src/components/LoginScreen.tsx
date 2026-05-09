import { useState } from "react";
import { Power } from "lucide-react";
import { toast } from "sonner";
import { login, setToken, clearToken, type AdminHttpError } from "@/lib/webAuth";

function formatSeconds(seconds?: number): string {
  if (!seconds || seconds <= 0) return "稍后再试";
  if (seconds < 60) return `${seconds} 秒后再试`;
  return `${Math.ceil(seconds / 60)} 分钟后再试`;
}

function getErrorMessage(error: unknown, fallback: string): string {
  if (!error || !(error instanceof Error)) return fallback;
  const adminError = error as AdminHttpError;
  if (adminError.isRateLimitError) return `登录尝试过于频繁，请在 ${formatSeconds(adminError.retryAfterSeconds)}。`;
  if (adminError.isNetworkError) return "无法连接 Web Admin 服务，请确认服务正在运行。";
  if (adminError.code === "INVALID_CREDENTIALS") {
    if (typeof adminError.remainingAttempts === "number") {
      return `用户名或密码错误，还可尝试 ${adminError.remainingAttempts} 次。`;
    }
    return "用户名或密码错误。";
  }
  if (adminError.isAuthError) return "登录已失效，请重新登录。";
  return adminError.message || fallback;
}

interface LoginScreenProps {
  onAuthenticated: () => void;
  message?: string;
  onRetry?: () => void;
}

export function LoginScreen({ onAuthenticated, message, onRetry }: LoginScreenProps) {
  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(event: React.FormEvent) {
    event.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const response = await login(username, password);
      setToken(response.token);
      toast.success("登录成功");
      onAuthenticated();
    } catch (err) {
      clearToken();
      const message = getErrorMessage(err, "登录失败");
      setError(message);
      const adminError = err as AdminHttpError;
      if (adminError?.isRateLimitError) toast.warning(message);
      else toast.error(message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="flex min-h-screen items-center justify-center bg-background px-4">
      <div className="w-full max-w-md rounded-xl border border-border bg-card p-6 shadow-sm">
        <div className="mb-6 flex items-center gap-3">
          <Power className="h-6 w-6 text-primary" />
          <div>
            <h1 className="text-xl font-semibold">API Switch Web Admin</h1>
            <p className="mt-1 text-sm text-muted-foreground">使用 Web 管理账号登录</p>
          </div>
        </div>
        <form onSubmit={handleSubmit} className="space-y-4">
          {message && (
            <div className="rounded-md bg-muted px-3 py-2 text-sm text-muted-foreground">
              <div>{message}</div>
              {onRetry && (
                <button type="button" onClick={onRetry} className="mt-2 text-primary underline-offset-2 hover:underline">
                  重试连接
                </button>
              )}
            </div>
          )}
          <label className="block space-y-1.5">
            <span className="text-sm font-medium">用户名</span>
            <input
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              autoComplete="username"
            />
          </label>
          <label className="block space-y-1.5">
            <span className="text-sm font-medium">密码</span>
            <input
              className="w-full rounded-md border border-input bg-background px-3 py-2 text-sm outline-none focus:ring-2 focus:ring-ring"
              value={password}
              onChange={(event) => setPassword(event.target.value)}
              type="password"
              autoComplete="current-password"
            />
          </label>
          {error && <div className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</div>}
          <button
            type="submit"
            disabled={submitting}
            className="w-full rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:cursor-not-allowed disabled:opacity-60"
          >
            {submitting ? "登录中..." : "登录"}
          </button>
        </form>
      </div>
    </div>
  );
}
