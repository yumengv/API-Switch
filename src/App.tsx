import { useState, useEffect, Suspense, lazy, useRef } from "react";
import { useTranslation } from "react-i18next";
import { WelcomeGuide } from "@/components/WelcomeGuide";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { MainShell, type MainPage } from "@/features/shell/MainShell";
import { useApiAdapter, isTauriRuntime } from "@/lib/useApiAdapter";
import { LoginScreen } from "@/components/LoginScreen";
import { AUTH_EXPIRED_EVENT, clearToken, getToken, logout, validateToken, type AuthExpiredDetail, TOKEN_KEY } from "@/lib/webAuth";
import { checkUpdate, type UpdateInfo } from "@/lib/api";

const ApiPoolPage = lazy(() => import("@/pages/ApiPoolPage").then((m) => ({ default: m.ApiPoolPage })));
const ChannelPage = lazy(() => import("@/pages/ChannelPage").then((m) => ({ default: m.ChannelPage })));
const TokenPage = lazy(() => import("@/pages/TokenPage").then((m) => ({ default: m.TokenPage })));
const LogPage = lazy(() => import("@/pages/LogPage").then((m) => ({ default: m.LogPage })));
const DashboardPage = lazy(() => import("@/pages/DashboardPage").then((m) => ({ default: m.DashboardPage })));
const SettingsPage = lazy(() => import("@/pages/SettingsPage").then((m) => ({ default: m.SettingsPage })));
type WebAuthViewState =
  | { state: "checking" }
  | { state: "authenticated" }
  | { state: "login"; message?: string }
  | { state: "server_unreachable"; message: string }
  | { state: "expired"; message: string };

const GUIDE_BASE = "https://github.com/wang1970/API-Switch/blob/master/";

function MainApp({ onLogout }: { onLogout?: () => void }) {
  const { i18n } = useTranslation();
  const api = useApiAdapter();
  const isDesktop = isTauriRuntime();
  const [currentPage, setCurrentPage] = useState<MainPage>("apiPool");

  const { data: settings } = useQuery({
    queryKey: ["settings"],
    queryFn: () => api.settings.get(),
  });

  const { data: proxyStatus } = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: () => api.proxy.getStatus(),
    refetchInterval: 2000,
  });

  const { data: adminStatus } = useQuery({
    queryKey: ["adminStatus"],
    queryFn: () => api.getAdminStatus(),
    refetchInterval: 2000,
  });

  // 状态版本检测：组件挂载时检测一次，不轮询
  // 数据看板等页面不再 2 秒自动刷新，进去有一次数据即可
  const queryClient = useQueryClient();
  const lastVersion = useRef<number | null>(null);
  useQuery({
    queryKey: ["state-version"],
    queryFn: async () => {
      const res = await api.getStateVersion();
      if (lastVersion.current !== null && res.version !== lastVersion.current) {
        queryClient.invalidateQueries({ predicate: (q) => q.queryKey[0] !== 'state-version' });
      }
      lastVersion.current = res.version;
      return res;
    },
  });

  const [guideOpen, setGuideOpen] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<{ current: string; latest: string; url: string } | null>(null);

  useEffect(() => {
    if (!isDesktop) return;
    import("@tauri-apps/api/app").then(({ getVersion }) => {
      getVersion().then((v: string) => { document.title = `API-Switch - ${v}`; });
    });
  }, [isDesktop]);

  useEffect(() => {
    if (!isDesktop) return;
    checkUpdate().then((info) => {
      if (info) setUpdateInfo(info);
    });
  }, [isDesktop]);

  useEffect(() => {
    if (!settings) return;
    if (isDesktop && settings.show_guide !== false) setGuideOpen(true);
  }, [settings?.show_guide, isDesktop]);

  const handleGuideDismiss = (dontShowAgain: boolean) => {
    if (dontShowAgain && settings) api.settings.update({ ...settings, show_guide: false });
  };

  useEffect(() => {
    if (!settings) return;
    const saved = localStorage.getItem("api-switch-locale");
    if (!saved && settings.locale) i18n.changeLanguage(settings.locale);
    const root = document.documentElement;
    if (settings.theme === "dark") root.classList.add("dark");
    else if (settings.theme === "light") root.classList.remove("dark");
    else if (window.matchMedia("(prefers-color-scheme: dark)").matches) root.classList.add("dark");
    else root.classList.remove("dark");
  }, [settings]);

  const openExternal = (url: string) => {
    if (isDesktop) import("@tauri-apps/plugin-opener").then(({ openUrl }) => openUrl(url));
    else window.open(url, "_blank", "noopener,noreferrer");
  };

  const renderPage = () => {
    const page = (() => {
      switch (currentPage) {
        case "apiPool": return <ApiPoolPage />;
        case "channels": return <ChannelPage />;
        case "tokens": return <TokenPage />;
        case "logs": return <LogPage />;
        case "dashboard": return <DashboardPage />;
        case "settings": return <SettingsPage />;
      }
    })();
    return <ErrorBoundary key={currentPage}>{page}</ErrorBoundary>;
  };

  return (
    <MainShell
      currentPage={currentPage}
      proxyStatus={proxyStatus}
      adminStatus={adminStatus}
      settings={settings}
      updateInfo={updateInfo}
      onUpdateDismiss={() => setUpdateInfo(null)}
      onUpdateOpen={(url) => openExternal(url)}
      onNavigate={setCurrentPage}
      onOpenGuide={(path) => openExternal(GUIDE_BASE + path)}
      onLogout={onLogout}
      renderPage={() => (
        <Suspense fallback={<div className="flex items-center justify-center min-h-screen">Loading...</div>}>
          {renderPage()}
        </Suspense>
      )}
    >
      {isDesktop && settings?.show_guide !== false && (
        <WelcomeGuide open={guideOpen} onOpenChange={setGuideOpen} onDismiss={handleGuideDismiss} />
      )}
    </MainShell>
  );
}

/**
 * Gate: in web mode, validate token before rendering MainApp.
 * In desktop mode, skip directly to MainApp.
 * This wrapper avoids React Hooks ordering issues —
 * no hooks are called conditionally.
 */
export default function App() {
  const isDesktop = isTauriRuntime();
  const [webAuth, setWebAuth] = useState<WebAuthViewState>(() =>
    isDesktop ? { state: "authenticated" } : (getToken() ? { state: "checking" } : { state: "login" })
  );

  const checkWebTokenIdRef = useRef(0);

  const invalidateWebTokenCheck = () => {
    checkWebTokenIdRef.current += 1;
  };

  const checkWebToken = () => {
    setWebAuth({ state: "checking" });
    const currentId = ++checkWebTokenIdRef.current;
    validateToken().then((result) => {
      if (currentId !== checkWebTokenIdRef.current) return;
      if (result.status === "valid") {
        setWebAuth({ state: "authenticated" });
        return;
      }

      if (result.status === "unreachable") {
        setWebAuth({ state: "server_unreachable", message: "无法连接 Web Admin 服务，请确认服务正在运行。" });
        return;
      }

      clearToken();
      if (result.status === "invalid") {
        setWebAuth({ state: "expired", message: "登录已过期，请重新登录。" });
      } else {
        setWebAuth({ state: "login", message: result.message });
      }
    });
  };

  const handleLogout = () => {
    invalidateWebTokenCheck();
    logout().then((result) => {
      invalidateWebTokenCheck();
      setWebAuth({ state: "login" });
      if (result.confirmed) toast.success("已退出登录");
      else toast.warning("已清除本地登录状态，服务器登出未确认");
    });
  };

  // Validate existing token on mount (web only)
  useEffect(() => {
    if (isDesktop || webAuth.state !== "checking") return;
    checkWebToken();
  }, [isDesktop, webAuth.state]);

  useEffect(() => {
    if (isDesktop) return;
    const handler = (event: Event) => {
      const detail = (event as CustomEvent<AuthExpiredDetail>).detail;
      // 仅在当前仍处于认证或校验中时处理，避免重复状态抖动。
      if (webAuth.state !== "authenticated" && webAuth.state !== "checking") return;
      invalidateWebTokenCheck();
      clearToken();
      setWebAuth({ state: "expired", message: detail?.message || "登录已过期，请重新登录。" });
      toast.error("登录已过期，请重新登录", { id: "web-admin-auth-expired" });
    };
    window.addEventListener(AUTH_EXPIRED_EVENT, handler);
    return () => window.removeEventListener(AUTH_EXPIRED_EVENT, handler);
  }, [isDesktop, webAuth.state]);

  useEffect(() => {
    if (isDesktop) return;
    const handler = (event: StorageEvent) => {
      if (event.key !== TOKEN_KEY) return;
      if (!event.newValue) {
        invalidateWebTokenCheck();
        // 仅在当前已认证时切换状态，避免覆盖有效状态流转
        if (webAuth.state === "authenticated") {
          setWebAuth({ state: "login", message: "已在其他页面退出登录。" });
          toast.info("已在其他页面退出登录", { id: "web-admin-storage-logout" });
        }
        return;
      }
      // token 被替换：重新校验以同步其他标签页的登录状态
      if (webAuth.state !== "checking") {
        checkWebToken();
      }
    };
    window.addEventListener("storage", handler);
    return () => window.removeEventListener("storage", handler);
  }, [isDesktop, webAuth.state]);

  if (isDesktop) return <MainApp />;

  if (webAuth.state === "checking") {
    return <div className="flex items-center justify-center min-h-screen text-muted-foreground">Loading...</div>;
  }

  if (webAuth.state === "login" || webAuth.state === "server_unreachable" || webAuth.state === "expired") {
    return (
      <LoginScreen
        message={webAuth.message}
        onRetry={webAuth.state === "server_unreachable" ? checkWebToken : undefined}
        onAuthenticated={() => setWebAuth({ state: "authenticated" })}
      />
    );
  }

  return <MainApp onLogout={handleLogout} />;
}
