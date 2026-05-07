import { useState, useEffect, Suspense, lazy } from "react";
import { useTranslation } from "react-i18next";
import { WelcomeGuide } from "@/components/WelcomeGuide";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { useQuery } from "@tanstack/react-query";
import { MainShell, type MainPage } from "@/features/shell/MainShell";
import { useApiAdapter, isTauriRuntime } from "@/lib/useApiAdapter";
import { LoginScreen } from "@/components/LoginScreen";
import { getToken, validateToken, clearToken } from "@/lib/webAuth";

const ApiPoolPage = lazy(() => import("@/pages/ApiPoolPage").then((m) => ({ default: m.ApiPoolPage })));
const ChannelPage = lazy(() => import("@/pages/ChannelPage").then((m) => ({ default: m.ChannelPage })));
const TokenPage = lazy(() => import("@/pages/TokenPage").then((m) => ({ default: m.TokenPage })));
const LogPage = lazy(() => import("@/pages/LogPage").then((m) => ({ default: m.LogPage })));
const DashboardPage = lazy(() => import("@/pages/DashboardPage").then((m) => ({ default: m.DashboardPage })));
const SettingsPage = lazy(() => import("@/pages/SettingsPage").then((m) => ({ default: m.SettingsPage })));
const GUIDE_BASE = "https://github.com/wang1970/API-Switch/blob/master/";

function MainApp() {
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

  const [guideOpen, setGuideOpen] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<{ current: string; latest: string; url: string } | null>(null);

  useEffect(() => {
    if (!isDesktop) return;
    import("@tauri-apps/api/app").then(({ getVersion }) => {
      getVersion().then((v: string) => { document.title = `API-Switch - ${v}`; });
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
      settings={settings}
      updateInfo={updateInfo}
      onUpdateDismiss={() => setUpdateInfo(null)}
      onUpdateOpen={(url) => openExternal(url)}
      onNavigate={setCurrentPage}
      onOpenGuide={(path) => openExternal(GUIDE_BASE + path)}
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
  const [webAuth, setWebAuth] = useState<"checking" | "authenticated" | "login">(() =>
    isDesktop ? "authenticated" : (getToken() ? "checking" : "login")
  );

  // Validate existing token on mount (web only)
  useEffect(() => {
    if (isDesktop || webAuth !== "checking") return;
    validateToken().then((valid) => {
      if (valid) setWebAuth("authenticated");
      else { clearToken(); setWebAuth("login"); }
    });
  }, [isDesktop, webAuth]);

  if (isDesktop) return <MainApp />;

  if (webAuth === "checking") {
    return <div className="flex items-center justify-center min-h-screen text-muted-foreground">Loading...</div>;
  }

  if (webAuth === "login") {
    return <LoginScreen onAuthenticated={() => setWebAuth("authenticated")} />;
  }

  return <MainApp />;
}
