import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { useState, useRef, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { DEFAULT_SETTINGS, type AppSettings, type ProxyStatus } from "@/types";

export interface SettingsEditorProps {
  settings?: AppSettings | null;
  proxyStatus?: ProxyStatus | null;
  appVersion?: string;
  isWeb?: boolean;
  groups?: string[];
  onChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  onProxyToggle?: (enabled: boolean) => void | Promise<void>;
}

export function SettingsEditor({
  settings,
  proxyStatus,
  appVersion,
  isWeb = false,
  groups = ["auto"],
  onChange,
  onProxyToggle,
}: SettingsEditorProps) {
  const { t } = useTranslation();
  const s = { ...DEFAULT_SETTINGS, ...settings };

  // Local state for text inputs that should only save on blur
  const [editUsername, setEditUsername] = useState(s.web_admin_username);
  const [editPassword, setEditPassword] = useState(s.web_admin_password);
  const usernameEditing = useRef(false);
  const passwordEditing = useRef(false);

  // Sync from props when not actively editing
  useEffect(() => {
    if (!usernameEditing.current) setEditUsername(s.web_admin_username);
  }, [s.web_admin_username]);
  useEffect(() => {
    // Don't sync empty password (backend getter may clear it for security)
    if (!passwordEditing.current && s.web_admin_password) {
      setEditPassword(s.web_admin_password);
    }
  }, [s.web_admin_password]);

  return (
    <div className="space-y-6">
      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.proxy.title")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <Label>{t("settings.proxy.port")}</Label>
            <Input
              type="number"
              className="w-32"
              value={s.listen_port}
              onChange={(event) => onChange("listen_port", parseInt(event.target.value) || 9090)}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label>{t("settings.proxy.enabled")}</Label>
            <Switch
              checked={proxyStatus?.running ?? s.proxy_enabled}
              onCheckedChange={(value) => {
                if (onProxyToggle) {
                  onProxyToggle(value);
                } else {
                  onChange("proxy_enabled", value);
                }
              }}
            />
          </div>
          {(proxyStatus?.running ?? s.proxy_enabled) && (
            <div className="text-sm text-muted-foreground">
              {t("settings.proxy.address")}: http://127.0.0.1:{proxyStatus?.port ?? s.listen_port}
            </div>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.security.title")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <div>
              <Label>{t("settings.security.forceKey")}</Label>
              <p className="text-xs text-muted-foreground">{t("settings.security.forceKeyDesc")}</p>
            </div>
            <Switch checked={s.access_key_required} onCheckedChange={(value) => onChange("access_key_required", value)} />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.circuit.title")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <Label>{t("settings.circuit.threshold")}</Label>
            <Input
              type="number"
              className="w-32"
              value={s.circuit_failure_threshold}
              onChange={(event) => onChange("circuit_failure_threshold", parseInt(event.target.value) || 1)}
            />
          </div>
          <div className="flex items-center justify-between">
            <div>
              <Label>{t("settings.circuit.connectTimeout")}</Label>
              <p className="text-xs text-muted-foreground">{t("settings.circuit.connectTimeoutDesc")}</p>
            </div>
            <Input
              type="number"
              min={1}
              max={300}
              className="w-32"
              value={s.proxy_connect_timeout_secs}
              onChange={(event) => onChange("proxy_connect_timeout_secs", Math.min(300, Math.max(1, parseInt(event.target.value) || 30)))}
            />
          </div>
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <Label>{t("settings.circuit.recovery")}</Label>
              <span className="text-sm text-muted-foreground w-16 text-right">{s.circuit_recovery_secs}s</span>
            </div>
            <Slider
              min={300}
              max={1800}
              step={30}
              value={s.circuit_recovery_secs}
              onValueChange={(value) => onChange("circuit_recovery_secs", value)}
            />
            <p className="text-xs text-muted-foreground">300s – 1800s</p>
          </div>
          <div className="space-y-2">
            <Label>{t("settings.circuit.disableCodes")}</Label>
            <Input value={s.circuit_disable_codes} onChange={(event) => onChange("circuit_disable_codes", event.target.value)} />
            <p className="text-xs text-muted-foreground">{t("settings.circuit.disableDesc")}</p>
          </div>
        </CardContent>
      </Card>

      <Card>
          <CardHeader>
            <CardTitle className="text-base">{t("settings.webAdmin.title")}</CardTitle>
          </CardHeader>
          <CardContent className="space-y-4">
            <div className="flex items-center justify-between">
              <div>
                <Label>{t("settings.webAdmin.enabled")}</Label>
                <p className="text-xs text-muted-foreground">{t("settings.webAdmin.enabledDesc")}</p>
              </div>
              <Switch checked={s.web_admin_enabled} onCheckedChange={(value) => onChange("web_admin_enabled", value)} />
            </div>
            <div className="flex items-center justify-between">
              <Label>{t("settings.webAdmin.port")}</Label>
              <Input
                type="number"
                min={1}
                max={65535}
                className="w-32"
                value={s.web_admin_port}
                onChange={(event) => onChange("web_admin_port", Math.min(65535, Math.max(1, parseInt(event.target.value) || 9099)))}
              />
            </div>
            <div className="space-y-2">
              <Label>{t("settings.webAdmin.username")}</Label>
              <Input
                value={editUsername}
                onFocus={() => { usernameEditing.current = true; }}
                onChange={(event) => setEditUsername(event.target.value)}
                onBlur={() => {
                  usernameEditing.current = false;
                  onChange("web_admin_username", editUsername);
                }}
              />
            </div>
            <div className="space-y-2">
              <Label>{t("settings.webAdmin.password")}</Label>
              <Input
                type="password"
                value={editPassword}
                placeholder={settings?.web_admin_password ? t("settings.webAdmin.configured") : ""}
                onFocus={() => { passwordEditing.current = true; }}
                onChange={(event) => setEditPassword(event.target.value)}
                onBlur={() => {
                  passwordEditing.current = false;
                  if (editPassword) {
                    onChange("web_admin_password", editPassword);
                  }
                }}
              />
              <p className="text-xs text-muted-foreground">
                {t("settings.webAdmin.singlePortDesc")}
              </p>
            </div>
            {s.web_admin_enabled && s.web_admin_username && s.web_admin_password && (
              <div className="text-sm text-muted-foreground">{t("settings.webAdmin.address")}: http://127.0.0.1:{s.web_admin_port}/admin</div>
            )}
          </CardContent>
        </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-base">{t("settings.general.title")}</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="flex items-center justify-between">
            <Label>{t("settings.general.language")}</Label>
            <Select value={s.locale} onValueChange={(value) => onChange("locale", value)}>
              <SelectTrigger className="w-32">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem value="zh">中文</SelectItem>
                <SelectItem value="en">English</SelectItem>
              </SelectContent>
            </Select>
          </div>
              <div className="flex items-center justify-between">
                <div>
                  <Label>{t("settings.general.showConversationModel")}</Label>
                  <p className="text-xs text-muted-foreground">{t("settings.general.showConversationModelDesc")}</p>
                </div>
                <Switch checked={s.show_conversation_model} onCheckedChange={(value) => onChange("show_conversation_model", value)} />
              </div>
              {!isWeb && (
            <>
              <div className="flex items-center justify-between">
                <Label>{t("settings.tray.autostart")}</Label>
                <Switch checked={s.autostart} onCheckedChange={(value) => onChange("autostart", value)} />
              </div>
              <div className="flex items-center justify-between">
                <Label>{t("settings.tray.startMinimized")}</Label>
                <Switch checked={s.start_minimized} onCheckedChange={(value) => onChange("start_minimized", value)} />
              </div>
            </>
          )}
          {appVersion && (
            <div className="flex items-center justify-between pt-2 border-t">
              <Label className="text-muted-foreground">{t("settings.general.currentVersion")}</Label>
              <span className="text-sm font-mono text-muted-foreground">{appVersion}</span>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
