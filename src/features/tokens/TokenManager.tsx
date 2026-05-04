import { useState, useEffect, useCallback } from "react";
import { useTranslation } from "react-i18next";
import { Plus, Trash2, Copy, Check } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogFooter,
  DialogDescription,
} from "@/components/ui/dialog";
import { useApiAdapter } from "@/lib/useApiAdapter";
import type { AccessKey } from "@/types";

const POLL_INTERVAL_MS = 10_000;

export function TokenManager() {
  const { t } = useTranslation();
  const adapter = useApiAdapter();

  const [keys, setKeys] = useState<AccessKey[]>([]);
  const [loading, setLoading] = useState(true);
  const [showCreate, setShowCreate] = useState(false);
  const [newKeyName, setNewKeyName] = useState("");
  const [createdKey, setCreatedKey] = useState<AccessKey | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const fetchKeys = useCallback(async () => {
    try {
      const data = await adapter.tokens.list();
      setKeys(data);
    } catch {
      // keep stale data on transient failure
    } finally {
      setLoading(false);
    }
  }, [adapter.tokens]);

  useEffect(() => {
    fetchKeys();
    const id = setInterval(fetchKeys, POLL_INTERVAL_MS);
    return () => clearInterval(id);
  }, [fetchKeys]);

  const handleCreate = async () => {
    if (!newKeyName.trim()) return;
    try {
      const key = await adapter.tokens.create(newKeyName.trim());
      setCreatedKey(key);
      setNewKeyName("");
      setShowCreate(false);
      await fetchKeys();
    } catch {
      // error surfaced via future toast integration
    }
  };

  const handleDelete = async (id: string) => {
    try {
      await adapter.tokens.delete(id);
      await fetchKeys();
    } catch {
      // handle error
    }
  };

  const handleToggle = async (id: string, enabled: boolean) => {
    try {
      await adapter.tokens.toggle(id, enabled);
      await fetchKeys();
    } catch {
      // handle error
    }
  };

  const copyKey = async (key: string, id: string) => {
    await navigator.clipboard.writeText(key);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 3000);
  };

  const formatDate = (ts: number) => new Date(ts * 1000).toLocaleString();

  if (loading) {
    return <div className="p-6 text-muted-foreground">{t("common.loading")}</div>;
  }

  return (
    <div className="p-6">
      <div className="flex items-center justify-between mb-6">
        <h1 className="text-xl font-semibold">{t("token.title")}</h1>
        <Button size="sm" className="gap-1.5" onClick={() => setShowCreate(true)}>
          <Plus className="h-4 w-4" />
          {t("token.add")}
        </Button>
      </div>

      {keys.length ? (
        <div className="border rounded-lg overflow-hidden">
          <table className="w-full text-sm">
            <thead>
              <tr className="border-b bg-muted/50 text-left text-muted-foreground">
                <th className="px-4 py-2 font-medium w-16">{t("token.enabled")}</th>
                <th className="px-4 py-2 font-medium">{t("token.name")}</th>
                <th className="px-4 py-2 font-medium">{t("token.key")}</th>
                <th className="px-4 py-2 font-medium">{t("token.created")}</th>
                <th className="px-4 py-2 font-medium w-16">{t("common.action")}</th>
              </tr>
            </thead>
            <tbody>
              {keys.map((k) => (
                <tr key={k.id} className="border-b last:border-b-0 hover:bg-muted/30">
                  <td className="px-4 py-3">
                    <Switch
                      checked={k.enabled}
                      onCheckedChange={(checked) => handleToggle(k.id, checked)}
                    />
                  </td>
                  <td className="px-4 py-3 font-medium">{k.name}</td>
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-1 min-w-0">
                      <code className="text-xs bg-muted px-1.5 py-0.5 rounded font-mono break-all flex-1 min-w-0">
                        {k.key}
                      </code>
                      <Button
                        variant="ghost"
                        size="icon"
                        className="h-5 w-5 shrink-0 text-muted-foreground"
                        onClick={() => copyKey(k.key, k.id)}
                      >
                        {copiedId === k.id ? (
                          <Check className="h-3 w-3 text-green-600" />
                        ) : (
                          <Copy className="h-3 w-3" />
                        )}
                      </Button>
                    </div>
                  </td>
                  <td className="px-4 py-3 text-muted-foreground text-xs">
                    {formatDate(k.created_at)}
                  </td>
                  <td className="px-4 py-3">
                    <Button
                      variant="ghost"
                      size="icon"
                      className="h-7 w-7"
                      onClick={() => handleDelete(k.id)}
                    >
                      <Trash2 className="h-3.5 w-3.5 text-destructive" />
                    </Button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      ) : (
        <div className="flex h-64 items-center justify-center text-muted-foreground">
          {t("common.noData")}
        </div>
      )}

      {/* Create Dialog */}
      <Dialog open={showCreate} onOpenChange={setShowCreate}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("token.add")}</DialogTitle>
          </DialogHeader>
          <div className="space-y-4">
            <div className="space-y-2">
              <Label>{t("token.name")}</Label>
              <Input
                value={newKeyName}
                onChange={(e) => setNewKeyName(e.target.value)}
                placeholder="My Laptop"
              />
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setShowCreate(false)}>
              {t("common.cancel")}
            </Button>
            <Button onClick={handleCreate} disabled={!newKeyName.trim()}>
              {t("common.add")}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* Created Key Dialog */}
      <Dialog open={!!createdKey} onOpenChange={(v) => !v && setCreatedKey(null)}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>{t("token.add")}</DialogTitle>
            <DialogDescription>{t("token.keyWarning")}</DialogDescription>
          </DialogHeader>
          {createdKey && (
            <div className="space-y-3">
              <div className="flex items-center gap-2">
                <code className="flex-1 text-sm bg-muted p-3 rounded font-mono break-all">
                  {createdKey.key}
                </code>
                <Button
                  variant="outline"
                  size="icon"
                  onClick={() => copyKey(createdKey.key, createdKey.id)}
                >
                  {copiedId === createdKey.id ? (
                    <Check className="h-4 w-4 text-green-600" />
                  ) : (
                    <Copy className="h-4 w-4" />
                  )}
                </Button>
              </div>
            </div>
          )}
          <DialogFooter>
            <Button onClick={() => setCreatedKey(null)}>{t("common.close")}</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
