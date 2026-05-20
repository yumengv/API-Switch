import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';
import { useQuery, useInfiniteQuery, useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { Edit, Plus, RefreshCw, Trash2, Power, PowerOff, XCircle, FileText } from 'lucide-react';
import { Button } from '@/components/ui/button';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { cn, formatResponseMs } from '@/lib/utils';
import { useApiAdapter } from '../../lib/useApiAdapter';
import { useDirtyPolling } from '../../lib/useDirtyPolling';
import { getChannelErrorMessage } from './channelErrors';
import type { PaginatedResult } from '@/types';
import type { Channel } from './types';
import { ChannelEditorDialog } from './editor';

function sortChannels(items: Channel[]): Channel[] {
  const parseResponseMs = (value?: string) => {
    if (!value || value === 'X') return Number.POSITIVE_INFINITY;
    const num = Number(value);
    return Number.isFinite(num) && num > 0 ? num : Number.POSITIVE_INFINITY;
  };

  return [...items].sort((a, b) => {
    if (a.enabled !== b.enabled) {
      return a.enabled ? -1 : 1;
    }
    const aMs = parseResponseMs(a.response_ms);
    const bMs = parseResponseMs(b.response_ms);
    if (aMs !== bMs) {
      return aMs - bMs;
    }
    return a.name.localeCompare(b.name, 'zh-CN');
  });
}

export const ChannelManager: React.FC = () => {
  const { t } = useTranslation();
  const api = useApiAdapter();
  const queryClient = useQueryClient();
  const {
    data: channelPages,
    fetchNextPage,
    hasNextPage,
    isFetchingNextPage,
    isLoading: loading,
    error: queryError,
  } = useInfiniteQuery({
    queryKey: ["channels", "paginated"],
    queryFn: ({ pageParam = 1 }) =>
      api.channels.listPaginated({ page: pageParam, pageSize: 40 }) as Promise<PaginatedResult<Channel>>,
    getNextPageParam: (lastPage) =>
      lastPage.page * lastPage.page_size < lastPage.total ? lastPage.page + 1 : undefined,
    initialPageParam: 1,
    staleTime: 2000,
  });
  const { data: entries } = useQuery({
    queryKey: ["entries", "all"],
    queryFn: () => api.pool.list(),
    staleTime: 2000,
  });
  const dirtyQueryKeys = useMemo(() => [['channels', 'paginated'], ['channels', 'all'], ['entries']] as const, []);

  useDirtyPolling('channel', dirtyQueryKeys);

  const rawChannels = useMemo(() => channelPages?.pages.flatMap((page) => page.items) ?? [], [channelPages]);
  const channels = useMemo(() => sortChannels(rawChannels), [rawChannels]);
  const entryCountMap = useMemo(() => {
    const map = new Map<string, number>();
    for (const entry of entries ?? []) {
      map.set(entry.channel_id, (map.get(entry.channel_id) ?? 0) + 1);
    }
    return map;
  }, [entries]);
  const [editing, setEditing] = useState<Channel | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [testingChannelId, setTestingChannelId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, string>>({});
  const [deleteTarget, setDeleteTarget] = useState<Channel | null>(null);

  const error = queryError ? getChannelErrorMessage(queryError, t('channel.editor.listLoadFailed', '渠道列表加载失败')) : null;
  const sentinelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const el = sentinelRef.current;
    if (!el || !hasNextPage || isFetchingNextPage) return;
    const observer = new IntersectionObserver(
      ([entry]) => { if (entry.isIntersecting) fetchNextPage(); },
      { rootMargin: '200px' },
    );
    observer.observe(el);
    return () => observer.disconnect();
  }, [hasNextPage, isFetchingNextPage, fetchNextPage]);

  const refreshChannels = async () => {
    queryClient.invalidateQueries({ queryKey: ["channels", "paginated"] });
  };

  const openCreate = () => {
    setEditing(null);
    setDialogOpen(true);
  };

  const openEdit = (channel: Channel) => {
    setEditing(channel);
    setDialogOpen(true);
  };

  const handleDelete = (channel: Channel) => {
    setDeleteTarget(channel);
  };

  const confirmDeleteChannel = async () => {
    if (!deleteTarget) return;
    try {
      await api.channels.delete(deleteTarget.id);
      if (expandedId === deleteTarget.id) {
        setExpandedId(null);
      }
      await queryClient.invalidateQueries({ queryKey: ["channels"] });
      setDeleteTarget(null);
    } catch (err) {
      toast.error(getChannelErrorMessage(err, '删除渠道失败'));
    }
  };

  const testAllChannels = async () => {
    if (!channels) return;
    const toTest = [...channels];
    const results: Record<string, string> = {};
    for (const ch of toTest) {
      setTestingChannelId(ch.id);
      try {
        const result = await api.channels.probeUrl(ch.base_url);
        if (result.reachable) {
          const ms = String(result.latency_ms);
          await api.channels.updateResponseMs(ch.id, ms);
          results[ch.id] = ms;
        } else {
          await api.channels.update({ id: ch.id, enabled: false });
          results[ch.id] = 'X';
        }
      } catch {
        await api.channels.update({ id: ch.id, enabled: false });
        results[ch.id] = 'X';
      }
      setTestResults({ ...results });
      await new Promise((r) => setTimeout(r, 200));
    }
    setTestingChannelId(null);
    await queryClient.invalidateQueries({ queryKey: ["channels"] });
    setTestResults({});
  };

  return (
    <div className="border border-border bg-card p-6 shadow-sm">
      <div className="space-y-6">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <div>
            <h1 className="text-xl font-semibold">{t('channel.editor.newChannel')}</h1>
            <p className="mt-1 text-sm text-muted-foreground">{t('channel.description')}</p>
          </div>
          <Button size="sm" className="gap-1.5" onClick={openCreate}>
            <Plus className="h-4 w-4" />
            {t('channel.editor.tipsAdd')}
          </Button>
        </div>

        {error && <div className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</div>}

        <div className="overflow-hidden rounded-lg border border-border bg-background">
          <table className="w-full table-fixed text-sm">
            <colgroup>
              <col className="w-[20%]" />
              <col className="w-24" />
              <col />
              <col className="w-24" />
              <col className="w-24" />
              <col className="w-24" />
              <col className="w-32" />
            </colgroup>
            <thead className="bg-muted/50">
              <tr className="border-b border-border">
                <th className="px-4 py-3 text-left font-medium">{t('channel.name')}</th>
                <th className="px-4 py-3 text-left font-medium">{t('channel.type')}</th>
                <th className="px-4 py-3 text-left font-medium">{t('channel.baseUrl')}</th>
                <th className="px-4 py-3 text-left font-medium">{t('channel.status')}</th>
                <th className="px-4 py-3 text-left font-medium whitespace-nowrap">
                  <div className="flex items-center gap-1">
                    <span>{t('channel.responseTime')}</span>
                    <button
                      type="button"
                      onClick={testAllChannels}
                      disabled={testingChannelId !== null}
                      className="text-muted-foreground hover:text-foreground transition-colors disabled:opacity-50"
                      title={t('channel.testAllLatency')}
                    >
                      <RefreshCw className={cn('h-3.5 w-3.5', testingChannelId !== null && 'animate-spin')} />
                    </button>
                  </div>
                </th>
                <th className="px-4 py-3 text-center font-medium">{t('channel.modelCount')}</th>
                <th className="px-4 py-3 text-right font-medium">{t('channel.actions')}</th>
              </tr>
            </thead>
            <tbody>
              {loading ? (
                <>
                  <tr>
                    <td className="px-4 py-3">
                      <div className="h-4 w-32 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-5 w-16 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3 w-48 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-5 w-12 animate-pulse bg-muted rounded-full" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3.5 w-3 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3 w-10 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1">
                        <div className="h-7 w-7 animate-pulse bg-muted rounded" />
                        <div className="h-7 w-12 animate-pulse bg-muted rounded" />
                      </div>
                    </td>
                  </tr>
                  <tr>
                    <td className="px-4 py-3">
                      <div className="h-4 w-28 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-5 w-14 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3 w-44 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-5 w-12 animate-pulse bg-muted rounded-full" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3.5 w-3 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3 w-10 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1">
                        <div className="h-7 w-7 animate-pulse bg-muted rounded" />
                        <div className="h-7 w-12 animate-pulse bg-muted rounded" />
                      </div>
                    </td>
                  </tr>
                  <tr>
                    <td className="px-4 py-3">
                      <div className="h-4 w-36 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-5 w-18 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3 w-52 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-5 w-12 animate-pulse bg-muted rounded-full" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3.5 w-3 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="h-3 w-10 animate-pulse bg-muted rounded" />
                    </td>
                    <td className="px-4 py-3">
                      <div className="flex justify-end gap-1">
                        <div className="h-7 w-7 animate-pulse bg-muted rounded" />
                        <div className="h-7 w-12 animate-pulse bg-muted rounded" />
                      </div>
                    </td>
                  </tr>
                </>
              ) : channels.length === 0 ? (
                <tr>
                  <td colSpan={6} className="px-4 py-10 text-center text-muted-foreground">{t('channel.editor.channelListEmpty')}</td>
                </tr>
              ) : (
                channels.map((channel) => (
                  <ChannelRow
                    key={channel.id}
                    channel={channel}
                    expanded={expandedId === channel.id}
                    onToggle={() => setExpandedId((current) => (current === channel.id ? null : channel.id))}
                    onEdit={() => openEdit(channel)}
                    onDelete={() => handleDelete(channel)}
                    onChanged={refreshChannels}
                    testingChannelId={testingChannelId}
                    testResults={testResults}
                    entryCountMap={entryCountMap}
                  />

                ))
              )}
            </tbody>
          </table>
          <div ref={sentinelRef} className="h-4" />
          {isFetchingNextPage && (
            <div className="flex justify-center py-4 text-sm text-muted-foreground">Loading...</div>
          )}
        </div>

        <ChannelEditorDialog
          open={dialogOpen}
          channel={editing}
          onOpenChange={setDialogOpen}
          onSaved={refreshChannels}
        />
        <Dialog open={!!deleteTarget} onOpenChange={(v) => !v && setDeleteTarget(null)}>
          <DialogContent>
            <DialogHeader><DialogTitle>{t("common.deleteTitle")}</DialogTitle></DialogHeader>
            <p className="text-sm text-muted-foreground">{t("common.deleteWarning")}</p>
            <DialogFooter>
              <Button variant="outline" onClick={() => setDeleteTarget(null)}>{t("common.cancel")}</Button>
              <Button variant="destructive" disabled={false} onClick={confirmDeleteChannel}>{t("common.delete")}</Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
      </div>
    </div>
  );
};

function ChannelRow({
  channel,
  expanded,
  onToggle,
  onEdit,
  onDelete,
  onChanged,
  testingChannelId,
  testResults,
  entryCountMap,
}: {
  channel: Channel;
  expanded: boolean;
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onChanged: () => Promise<void>;
  testingChannelId?: string | null;
  testResults?: Record<string, string>;
  entryCountMap?: Map<string, number>;
}) {
  const { t } = useTranslation();
  const api = useApiAdapter();
  const [saving, setSaving] = useState(false);
  const [fetching, setFetching] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probeResult, setProbeResult] = useState<string | null>(null);
  const [rowError, setRowError] = useState<string | null>(null);

  const availableModels = channel.available_models ?? [];
  const selectedModels = channel.selected_models ?? [];

  const toggleEnabled = async () => {
    setSaving(true);
    setRowError(null);
    try {
      await api.channels.update({ id: channel.id, enabled: !channel.enabled });
      await onChanged();
    } catch (err) {
      const msg = (err && typeof err === 'object' && 'message' in err) ? String((err as {message:unknown}).message) : String(err);
      console.error('[toggleEnabled]', err);
      toast.error(msg || t('channel.editor.saveStatusFailed', '保存渠道状态失败'));
    } finally {
      setSaving(false);
    }
  };

  const probeUrl = async () => {
    setProbing(true);
    setProbeResult(null);
    setRowError(null);
    try {
      const result = await api.channels.probeUrl(channel.base_url);
      setProbeResult(`${result.latency_ms}ms`);
    } catch (err) {
      setRowError(getChannelErrorMessage(err, t('channel.editor.probeFailed', '测速失败')));
    } finally {
      setProbing(false);
    }
  };

  const fetchModels = async () => {
    setFetching(true);
    setRowError(null);
    try {
      await api.channels.fetchModels(channel.id);
      await onChanged();
    } catch (err) {
      setRowError(getChannelErrorMessage(err, t('channel.editor.fetchModelsFailed', '获取模型列表失败')));
    } finally {
      setFetching(false);
    }
  };

  const syncSelection = async (modelNames: string[]) => {
    setSaving(true);
    setRowError(null);
    try {
      await api.channels.selectModels(channel.id, modelNames, availableModels, []);
      await onChanged();
    } catch (err) {
      setRowError(getChannelErrorMessage(err, t('channel.editor.syncFailed', '同步 API 池失败')));
    } finally {
      setSaving(false);
    }
  };

  const toggleModel = (modelName: string) => {
    const next = selectedModels.includes(modelName)
      ? selectedModels.filter((item) => item !== modelName)
      : [...selectedModels, modelName];
    syncSelection(next);
  };

  return (
    <>
      <tr className="border-b border-border hover:bg-muted/30 cursor-pointer" onClick={onToggle}>
        <td className="min-w-0 px-4 py-3">
          <div className="max-w-full text-left">
            <div className="truncate font-medium flex items-center gap-1">
              {channel.name}
              {channel.notes && (
                <span title={channel.notes}>
                  <FileText className="h-3 w-3 text-muted-foreground shrink-0" />
                </span>
              )}
            </div>
          </div>
        </td>
        <td className="px-4 py-3">
          <span className="rounded bg-secondary px-2 py-0.5 text-xs text-muted-foreground">{channel.api_type}</span>
        </td>
        <td className="min-w-0 px-4 py-3 font-mono text-xs" title={channel.base_url}>
          <div className="truncate">{channel.base_url}</div>
        </td>
        <td className="px-4 py-3">
          <span className={cn('rounded-full px-2.5 py-1 text-xs font-medium', channel.enabled ? 'bg-green-100 text-green-700' : 'bg-muted text-muted-foreground')}>
            {channel.enabled ? t('channel.enabled') : t('channel.disabled')}
          </span>
        </td>
        <td className="px-4 py-3 text-xs text-muted-foreground whitespace-nowrap">
          {testingChannelId === channel.id ? (
            <RefreshCw className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
          ) : (() => {
            const testValue = testResults?.[channel.id];
            const persistedValue = channel.response_ms ? String(channel.response_ms) : '';
            const displayValue = testValue && testValue !== 'X' && persistedValue !== testValue
              ? testValue
              : persistedValue;

            if (testValue === 'X' && !persistedValue) {
              return <span className="text-red-500" title={t('common.failed')}><XCircle className="h-3.5 w-3.5" /></span>;
            }

            if (displayValue) {
              return <span className="text-green-600">{formatResponseMs(displayValue)}</span>;
            }

            return <span className="text-red-500" title={t('channel.testAllLatency')}><XCircle className="h-3.5 w-3.5" /></span>;
          })()}
        </td>
        <td className="px-4 py-3 whitespace-nowrap text-center">{entryCountMap?.get(channel.id) ?? 0} / {availableModels.length}</td>
        <td className="px-4 py-3">
          <div className="flex justify-end gap-1">
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={(event) => { event.stopPropagation(); onEdit(); }} title={t('common.edit')}>
              <Edit className="h-4 w-4" />
            </Button>
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={(event) => { event.stopPropagation(); toggleEnabled(); }} disabled={saving} title={channel.enabled ? t('channel.disabled') : t('channel.enabled')}>
              {channel.enabled ? <PowerOff className="h-4 w-4" /> : <Power className="h-4 w-4" />}
            </Button>
            <Button variant="ghost" size="icon" className="h-8 w-8 text-destructive" onClick={(event) => { event.stopPropagation(); onDelete(); }} title={t('common.delete')}>
              <Trash2 className="h-4 w-4" />
            </Button>
          </div>
        </td>
      </tr>

      {expanded && channel.notes ? (
        <tr className="border-b border-border bg-muted/20">
          <td colSpan={7} className="px-4 py-3">
            <div className="space-y-1 text-sm max-w-3xl">
              <div className="font-medium text-muted-foreground">{t('channel.notes')}</div>
              <pre className="whitespace-pre-wrap break-all">{channel.notes}</pre>
            </div>
          </td>
        </tr>
      ) : null}
    </>
  );
}
