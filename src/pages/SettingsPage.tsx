import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { useApiAdapter, isTauriRuntime } from "@/lib/useApiAdapter";
import { toast } from "sonner";
import { DEFAULT_SETTINGS, type AppSettings } from "@/types";
import { SettingsEditor } from "@/features/settings/SettingsEditor";

const APP_VERSION = "0.4.10";

export function SettingsPage() {
  const { t, i18n } = useTranslation();
  const queryClient = useQueryClient();
  const adapter = useApiAdapter();

  const { data: settings } = useQuery({
    queryKey: ["settings"],
    queryFn: adapter.settings.get,
  });

  const { data: groups = ["auto"] } = useQuery({
    queryKey: ["pool-groups"],
    queryFn: () => adapter.pool.getGroups(),
  });

  const { data: proxyStatus } = useQuery({
    queryKey: ["proxyStatus"],
    queryFn: adapter.proxy.getStatus,
  });

  const updateMutation = useMutation({
    mutationFn: adapter.settings.update,
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["settings"] });
      queryClient.invalidateQueries({ queryKey: ["pool-groups"] });
      queryClient.invalidateQueries({ queryKey: ["adminStatus"] });
    },
  });

  const s = { ...DEFAULT_SETTINGS, ...settings };

  const update = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    if (key === "locale") {
      i18n.changeLanguage(value as string);
      localStorage.setItem("api-switch-locale", value as string);
    }
    if (key === "active_group") {
      // Persist the remembered default group for the API Management page locally for faster UI restoration.
      localStorage.setItem("api-switch-default-group", value as string);
    }
    updateMutation.mutate({ ...s, [key]: value });
  };

  const toggleProxy = async (enabled: boolean) => {
    try {
      if (enabled) {
        await adapter.proxy.start();
      } else {
        await adapter.proxy.stop();
      }
        queryClient.invalidateQueries({ queryKey: ["proxyStatus"] });
        queryClient.invalidateQueries({ queryKey: ["adminStatus"] });
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
        isWeb={!isTauriRuntime()}
        groups={groups}
        onChange={update}
        onProxyToggle={toggleProxy}
      />
    </div>
  );
}
