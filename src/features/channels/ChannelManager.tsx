import React, { useEffect, useMemo, useState } from 'react';
import { Edit, Plus, RefreshCw, Save, Trash2 } from 'lucide-react';
import { Button } from '@/components/ui/button';
import { Checkbox } from '@/components/ui/checkbox';
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog';
import { Input } from '@/components/ui/input';
import { Label } from '@/components/ui/label';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select';
import { cn } from '@/lib/utils';
import { useApiAdapter } from '../../lib/useApiAdapter';
import { getChannelErrorMessage } from './channelErrors';
import type { Channel, CreateChannelParams, ModelInfo, UpdateChannelParams } from './types';

type ChannelFormState = {
  id?: string;
  name: string;
  api_type: string;
  base_url: string;
  api_key: string;
  notes: string;
  enabled: boolean;
};

const API_TYPES = [
  { value: 'custom', label: 'Custom' },
  { value: 'openai', label: 'OpenAI' },
  { value: 'claude', label: 'Claude' },
  { value: 'gemini', label: 'Gemini' },
  { value: 'azure', label: 'Azure' },
];

const DEFAULT_FORM: ChannelFormState = {
  name: '',
  api_type: 'custom',
  base_url: '',
  api_key: '',
  notes: '',
  enabled: true,
};

function channelToForm(channel: Channel): ChannelFormState {
  return {
    id: channel.id,
    name: channel.name,
    api_type: channel.api_type,
    base_url: channel.base_url,
    api_key: channel.api_key,
    notes: channel.notes ?? '',
    enabled: channel.enabled,
  };
}

export const ChannelManager: React.FC = () => {
  const api = useApiAdapter();
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<Channel | null>(null);
  const [dialogOpen, setDialogOpen] = useState(false);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  async function loadChannels() {
    setLoading(true);
    setError(null);
    try {
      const items = await api.channels.list();
      setChannels(items);
    } catch (err) {
      setError(getChannelErrorMessage(err, '渠道列表加载失败'));
    } finally {
      setLoading(false);
    }
  }

  useEffect(() => {
    loadChannels();
  }, []);

  const openCreate = () => {
    setEditing(null);
    setDialogOpen(true);
  };

  const openEdit = (channel: Channel) => {
    setEditing(channel);
    setDialogOpen(true);
  };

  const handleDelete = async (channel: Channel) => {
    if (!window.confirm(`确定删除渠道 ${channel.name}？关联 API 池条目也会被删除。`)) {
      return;
    }
    setError(null);
    try {
      await api.channels.delete(channel.id);
      if (expandedId === channel.id) {
        setExpandedId(null);
      }
      await loadChannels();
    } catch (err) {
      setError(getChannelErrorMessage(err, '删除渠道失败'));
    }
  };

  return (
    <div className="rounded-xl border border-border bg-card p-6 shadow-sm">
      <div className="space-y-6">
        <div className="flex flex-wrap items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold">渠道管理</h1>
          <p className="mt-1 text-sm text-muted-foreground">统一管理上游渠道、模型同步与基础配置。</p>
        </div>
        <Button size="sm" className="gap-1.5" onClick={openCreate}>
          <Plus className="h-4 w-4" />
          添加渠道
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
            <col className="w-32" />
          </colgroup>
          <thead className="bg-muted/50">
            <tr className="border-b border-border">
              <th className="px-4 py-3 text-left font-medium">渠道名称</th>
              <th className="px-4 py-3 text-left font-medium">类型</th>
              <th className="px-4 py-3 text-left font-medium">Base URL</th>
              <th className="px-4 py-3 text-left font-medium">状态</th>
              <th className="px-4 py-3 text-left font-medium">模型数</th>
              <th className="px-4 py-3 text-right font-medium">操作</th>
            </tr>
          </thead>
          <tbody>
            {loading ? (
              <tr>
                <td colSpan={6} className="px-4 py-10 text-center text-muted-foreground">加载中...</td>
              </tr>
            ) : channels.length === 0 ? (
              <tr>
                <td colSpan={6} className="px-4 py-10 text-center text-muted-foreground">暂无渠道，请先添加渠道。</td>
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
                  onChanged={loadChannels}
                />
              ))
            )}
          </tbody>
        </table>
      </div>

      <ChannelEditorDialog
        open={dialogOpen}
        channel={editing}
        onOpenChange={setDialogOpen}
        onSaved={loadChannels}
      />
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
}: {
  channel: Channel;
  expanded: boolean;
  onToggle: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onChanged: () => Promise<void>;
}) {
  const api = useApiAdapter();
  const [saving, setSaving] = useState(false);
  const [fetching, setFetching] = useState(false);
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
      setRowError(getChannelErrorMessage(err, '保存渠道状态失败'));
    } finally {
      setSaving(false);
    }
  };

  const fetchModels = async () => {
    setFetching(true);
    setRowError(null);
    try {
      await api.channels.fetchModels(channel.id);
      await onChanged();
    } catch (err) {
      setRowError(getChannelErrorMessage(err, '获取模型列表失败'));
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
      setRowError(getChannelErrorMessage(err, '同步 API 池失败'));
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
      <tr className="border-b border-border hover:bg-muted/30">
        <td className="min-w-0 px-4 py-3">
          <button type="button" className="max-w-full text-left" onClick={onToggle}>
            <div className="truncate font-medium">{channel.name}</div>
            {channel.notes ? <div className="mt-1 truncate text-xs text-muted-foreground">{channel.notes}</div> : null}
          </button>
        </td>
        <td className="px-4 py-3">
          <span className="rounded bg-secondary px-2 py-0.5 text-xs text-muted-foreground">{channel.api_type}</span>
        </td>
        <td className="min-w-0 px-4 py-3 font-mono text-xs" title={channel.base_url}>
          <div className="truncate">{channel.base_url}</div>
        </td>
        <td className="px-4 py-3">
          <span className={cn('rounded-full px-2.5 py-1 text-xs font-medium', channel.enabled ? 'bg-green-100 text-green-700' : 'bg-muted text-muted-foreground')}>
            {channel.enabled ? '启用' : '禁用'}
          </span>
        </td>
        <td className="px-4 py-3 whitespace-nowrap">{selectedModels.length} / {availableModels.length}</td>
        <td className="px-4 py-3">
          <div className="flex justify-end gap-1">
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={onEdit} title="编辑">
              <Edit className="h-4 w-4" />
            </Button>
            <Button variant="ghost" size="sm" className="h-8" onClick={toggleEnabled} disabled={saving}>
              {channel.enabled ? '禁用' : '启用'}
            </Button>
            <Button variant="ghost" size="icon" className="h-8 w-8" onClick={onDelete} title="删除">
              <Trash2 className="h-4 w-4 text-destructive" />
            </Button>
          </div>
        </td>
      </tr>

      {expanded && (
        <tr className="border-b border-border bg-muted/10">
          <td colSpan={6} className="px-4 py-4">
            <div className="space-y-4">
              {rowError && <div className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">{rowError}</div>}
              <div className="flex flex-wrap items-center gap-2">
                <Button size="sm" variant="outline" className="gap-1.5" onClick={fetchModels} disabled={fetching}>
                  <RefreshCw className={cn('h-3.5 w-3.5', fetching && 'animate-spin')} />
                  {fetching ? '获取中...' : '获取模型列表'}
                </Button>
                <Button size="sm" variant="outline" onClick={() => syncSelection(availableModels.map((model) => model.name))} disabled={saving || availableModels.length === 0}>
                  全选模型
                </Button>
                <Button size="sm" variant="outline" onClick={() => syncSelection([])} disabled={saving || selectedModels.length === 0}>
                  清空选择
                </Button>
              </div>

              <div className="grid gap-3 md:grid-cols-2 text-sm">
                <InfoBlock label="Base URL" value={channel.base_url} mono />
                <InfoBlock label="API Key" value={channel.api_key ? '••••••••' : ''} mono />
              </div>

              {availableModels.length > 0 ? (
                <div className="max-h-72 overflow-y-auto rounded-md border border-border bg-background">
                  {availableModels.map((model) => (
                    <label key={model.id || model.name} className="flex cursor-pointer items-center gap-2 border-b border-border px-3 py-2 text-sm last:border-b-0 hover:bg-accent">
                      <Checkbox checked={selectedModels.includes(model.name)} onCheckedChange={() => toggleModel(model.name)} disabled={saving} />
                      <span className="truncate">{model.name}</span>
                      {model.owned_by ? <span className="ml-auto text-xs text-muted-foreground">{model.owned_by}</span> : null}
                    </label>
                  ))}
                </div>
              ) : (
                <div className="rounded-md border border-dashed border-border p-6 text-center text-sm text-muted-foreground">
                  暂无模型。请先点击“获取模型列表”。
                </div>
              )}
            </div>
          </td>
        </tr>
      )}
    </>
  );
}

function ChannelEditorDialog({
  open,
  channel,
  onOpenChange,
  onSaved,
}: {
  open: boolean;
  channel: Channel | null;
  onOpenChange: (value: boolean) => void;
  onSaved: () => Promise<void>;
}) {
  const api = useApiAdapter();
  const [form, setForm] = useState<ChannelFormState>(DEFAULT_FORM);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setError(null);
    setForm(channel ? channelToForm(channel) : DEFAULT_FORM);
  }, [channel, open]);

  const canSave = form.name.trim() && form.base_url.trim() && form.api_key.trim();

  const setValue = <K extends keyof ChannelFormState>(key: K, value: ChannelFormState[K]) => {
    setForm((current) => ({ ...current, [key]: value }));
  };

  const handleSave = async () => {
    if (!canSave) return;
    setSaving(true);
    setError(null);
    try {
      if (form.id) {
        const params: UpdateChannelParams = {
          id: form.id,
          name: form.name,
          api_type: form.api_type,
          base_url: form.base_url,
          api_key: form.api_key,
          enabled: form.enabled,
          notes: form.notes,
        };
        await api.channels.update(params);
      } else {
        const params: CreateChannelParams = {
          name: form.name,
          api_type: form.api_type,
          base_url: form.base_url,
          api_key: form.api_key,
          notes: form.notes,
        };
        await api.channels.create(params);
      }
      await onSaved();
      onOpenChange(false);
    } catch (err) {
      setError(getChannelErrorMessage(err, '保存渠道失败'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle>{channel ? '编辑渠道' : '添加渠道'}</DialogTitle>
        </DialogHeader>

        <div className="space-y-4">
          {error && <div className="rounded-md bg-destructive/10 px-3 py-2 text-sm text-destructive">{error}</div>}
          <div className="space-y-2">
            <Label>渠道名称</Label>
            <Input value={form.name} onChange={(event) => setValue('name', event.target.value)} placeholder="例如 OpenAI" />
          </div>
          <div className="space-y-2">
            <Label>API 类型</Label>
            <Select value={form.api_type} onValueChange={(value) => setValue('api_type', value)}>
              <SelectTrigger>
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {API_TYPES.map((item) => (
                  <SelectItem key={item.value} value={item.value}>{item.label}</SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="space-y-2">
            <Label>Base URL</Label>
            <Input value={form.base_url} onChange={(event) => setValue('base_url', event.target.value)} placeholder="https://api.example.com" />
          </div>
          <div className="space-y-2">
            <Label>API Key</Label>
            <Input type="password" value={form.api_key} onChange={(event) => setValue('api_key', event.target.value)} />
          </div>
          <div className="space-y-2">
            <Label>备注</Label>
            <Input value={form.notes} onChange={(event) => setValue('notes', event.target.value)} />
          </div>
          {form.id && (
            <label className="flex items-center gap-2 text-sm">
              <Checkbox checked={form.enabled} onCheckedChange={(value) => setValue('enabled', Boolean(value))} />
              启用该渠道
            </label>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)} disabled={saving}>取消</Button>
          <Button onClick={handleSave} disabled={!canSave || saving} className="gap-1.5">
            <Save className="h-4 w-4" />
            {saving ? '保存中...' : '保存'}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function InfoBlock({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="rounded-md border border-border bg-background p-3">
      <div className="mb-1 text-xs text-muted-foreground">{label}</div>
      <div className={cn('break-all text-sm', mono && 'font-mono text-xs')}>{value}</div>
    </div>
  );
}
