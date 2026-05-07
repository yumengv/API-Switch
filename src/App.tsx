import { useState, useEffect, Suspense, lazy } from "react";
import { useTranslation } from "react-i18next";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getVersion } from "@tauri-apps/api/app";
import { WelcomeGuide } from "@/components/WelcomeGuide";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { useQuery } from "@tanstack/react-query";
import { getSettings, updateSettings, checkUpdate, getProxyStatus } from "@/lib/api";
import { MainShell, type MainPage } from "@/features/shell/MainShell";

const ApiPoolPage = lazy(() => import("@/pages/ApiPoolPage").then((m) => ({ default: m.ApiPoolPage })));
const ChannelPage = lazy(() => import("@/pages/ChannelPage").then((m) => ({ default: m.ChannelPage })));
const TokenPage = lazy(() => import("@/pages/TokenPage").then((m) => ({ default: m.TokenPage })));
const LogPage = lazy(() => import("@/pages/LogPage").then((m) => ({ default: m.LogPage })));
const DashboardPage = lazy(() => import("@/pages/DashboardPage").then((m) => ({ default: m.DashboardPage })));
const SettingsPage = lazy(() => import("@/pages/SettingsPage").then((m) => ({ default: m.SettingsPage })));
const TranslationRelayPanel = lazy(() => import("@/features/translator/TranslationRelayPanel").then((m) => ({ default: m.TranslationRelayPanel })));

const GUIDE_BASE = "https://github.com/wang1970/API-Switch/blob/master/";

export default function App() {
  const { i18n } = useTranslation();
  const [currentPage, setCurrentPage] = useState<MainPage>("apiPool");

  const { data: settings } = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });

  const { data: proxyStatus } = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: getProxyStatus,
    refetchInterval: 2000,
  });

  const [guideOpen, setGuideOpen] = useState(false);
  const [updateChecked, setUpdateChecked] = useState(false);
  const [updateInfo, setUpdateInfo] = useState<{ current: string; latest: string; url: string } | null>(null);
  const [appVersion, setAppVersion] = useState("");

  useEffect(() => {
    getVersion().then((v) => {
      setAppVersion(v);
      document.title = `API-Switch - ${v}`;
    });
  }, []);

  // Show guide only after settings confirm it should be shown; check updates if guide won't show
  useEffect(() => {
    if (!settings) return;
    if (settings.show_guide !== false) {
      setGuideOpen(true);
    } else {
      doCheckUpdate();
    }
  }, [settings?.show_guide]);

  // Check for updates once after guide is dismissed (or immediately if no guide)
  const doCheckUpdate = async () => {
    if (updateChecked) return;
    setUpdateChecked(true);
    const result = await checkUpdate();
    if (result) setUpdateInfo(result);
  };

  const handleGuideDismiss = (dontShowAgain: boolean) => {
    if (dontShowAgain) {
      updateSettings({ ...settings!, show_guide: false });
    }
    doCheckUpdate();
  };

  // Apply locale and theme
  useEffect(() => {
    if (!settings) return;

    // Apply locale from DB
    const saved = localStorage.getItem("api-switch-locale");
    if (!saved && settings.locale) {
      i18n.changeLanguage(settings.locale);
    }

    // Apply theme
    const root = document.documentElement;
    if (settings.theme === "dark") {
      root.classList.add("dark");
    } else if (settings.theme === "light") {
      root.classList.remove("dark");
    } else {
      if (window.matchMedia("(prefers-color-scheme: dark)").matches) {
        root.classList.add("dark");
      } else {
        root.classList.remove("dark");
      }
    }
  }, [settings]);

  const renderPage = () => {
    const page = (() => {
      switch (currentPage) {
        case "apiPool":
          return <ApiPoolPage />;
        case "channels":
          return <ChannelPage />;
        case "tokens":
          return <TokenPage />;
        case "logs":
          return <LogPage />;
        case "dashboard":
          return <DashboardPage />;
        case "translator":
          return <TranslationRelayPanel />;
        case "settings":
          return <SettingsPage />;
      }
    })();

    return (
      <ErrorBoundary key={currentPage}>
        {page}
      </ErrorBoundary>
    );
  };

  return (
    <MainShell
      currentPage={currentPage}
      proxyStatus={proxyStatus}
      settings={settings}
      updateInfo={updateInfo}
      onUpdateDismiss={() => setUpdateInfo(null)}
      onUpdateOpen={(url) => openUrl(url)}
      onNavigate={setCurrentPage}
      onOpenGuide={(path) => openUrl(GUIDE_BASE + path)}
      renderPage={() => (
        <Suspense fallback={<div className="flex items-center justify-center min-h-screen">Loading...</div>}>
          {renderPage()}
        </Suspense>
      )}
    >
      {settings?.show_guide !== false && (
        <WelcomeGuide
          open={guideOpen}
          onOpenChange={setGuideOpen}
          onDismiss={handleGuideDismiss}
        />
      )}
    </MainShell>
  );
}
