import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { getSettings, updateSettings, getProxyStatus, startProxy, stopProxy } from "@/lib/api";
import { toast } from "sonner";
import { DEFAULT_SETTINGS, type AppSettings } from "@/types";
import { SettingsEditor } from "@/features/settings/SettingsEditor";

const APP_VERSION = "0.4.10";

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();

  const { data: settings } = useQuery({
    queryKey: ["settings"],
    queryFn: getSettings,
  });

  const { data: proxyStatus } = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: getProxyStatus,
  });

  const updateMutation = useMutation({
    mutationFn: updateSettings,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: ["settings"] }),
  });

  const s = { ...DEFAULT_SETTINGS, ...settings };

  const update = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    if (key === "locale") {
      i18n.changeLanguage(value as string);
      localStorage.setItem("api-switch-locale", value as string);
    }
    if (key === "default_sort_mode") {
      localStorage.setItem("api-switch-sort-mode", value as string);
    }
    updateMutation.mutate({ ...s, [key]: value });
  };

  const toggleProxy = async (enabled: boolean) => {
    try {
      if (enabled) {
        await startProxy();
      } else {
        await stopProxy();
      }
      queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
      queryClient.invalidateQueries({ queryKey: ["settings"] });
    } catch (err) {
      const action = enabled ? t("settings.proxy.start") : t("settings.proxy.stop");
      toast.error(`${action} ${t("common.failed")}: ${err}`, { duration: Infinity });
    }
  };

  return (
    <div className="p-6 max-w-2xl">
      <h1 className="text-xl font-semibold mb-6">{t("settings.title")}</h1>
      <SettingsEditor
        settings={s}
        proxyStatus={proxyStatus}
        appVersion={APP_VERSION}
        onChange={update}
        onProxyToggle={toggleProxy}
      />
    </div>
  );
}
