import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import { toast } from 'sonner';
import { useQueryClient } from '@tanstack/react-query';
import { useTranslation } from 'react-i18next';
import { RefreshCw, Save, X, Zap } from 'lucide-react';
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
import { getCatalogModel, formatTokenCount } from '@/lib/modelsCatalog';
import { useApiAdapter } from '@/lib/useApiAdapter';
import { getChannelErrorMessage } from '../channelErrors';
import type { ModelInfo, UpdateChannelParams, CreateChannelParams, FetchModelsResult, SaveChannelWithModelsParams } from '../types';
import type { ChannelFormState, UrlProbeResult } from './types';
import { DEFAULT_FORM, API_TYPES, channelToForm } from './types';
import { buildEntryCatalogMeta, formatReleaseDate } from './utils';

type EditorModelInfo = ModelInfo & {
  sourceProtocol: string;
  temporary?: boolean;
};

/** 渠道编辑器对话框组件 */
export const ChannelEditorDialog: React.FC<{
  open: boolean;
  channel: import('../types').Channel | null;
  onOpenChange: (value: boolean) => void;
  onSaved: () => Promise<void>;
}> = ({ open, channel, onOpenChange, onSaved }) => {
  const { t } = useTranslation();
  const api = useApiAdapter();
  const queryClient = useQueryClient();
  const [form, setForm] = useState<ChannelFormState>(DEFAULT_FORM);
  const [showApiKey, setShowApiKey] = useState(false);
  const [fetchingModels, setFetchingModels] = useState(false);
  const [modelsValidated, setModelsValidated] = useState(false);
  const [modelSearch, setModelSearch] = useState('');
  const [availableModels, setAvailableModels] = useState<EditorModelInfo[]>([]);
  const [selectedModels, setSelectedModels] = useState<string[]>([]);
  const [urlProbe, setUrlProbe] = useState<UrlProbeResult | null>(null);
  const [probingUrl, setProbingUrl] = useState(false);
  const [availableProtocols, setAvailableProtocols] = useState<string[]>([]);
  const [showModels, setShowModels] = useState(false);
  // 时间范围选择：3个月/6个月/12个月
  const [timeRange, setTimeRange] = useState<3 | 6 | 12>(3);
  // 模型测速状态
  const [testingModels, setTestingModels] = useState(false);
  const [modelTestResults, setModelTestResults] = useState<Record<string, { success: boolean; latency?: number; reason?: string }>>({});

  const probeSeqRef = useRef(0);
  const fetchSeqRef = useRef(0);
  const testSeqRef = useRef(0);
  const existingModelsRef = useRef<Set<string>>(new Set());
  const [saving, setSaving] = useState(false);

  // 初始化表单数据
  useEffect(() => {
    if (!open) return;
    setSaving(false);
    setModelSearch('');
    setTimeRange(3);
    setShowApiKey(false);
    setAvailableProtocols([]);
    existingModelsRef.current = new Set();
    // 模型区默认隐藏逻辑：仅当现有渠道已有模型时才默认展开，否则收起
    const hasModels = !!channel && ((channel.available_models?.length || 0) > 0 || (channel.selected_models?.length || 0) > 0);
    setModelsValidated(!!channel && ((channel.available_models?.length || 0) > 0));
    setShowModels(hasModels);
    if (channel) {
      setForm(channelToForm(channel));
      setAvailableModels((channel.available_models || []).map((model) => ({
        ...model,
        sourceProtocol: channel.api_type,
      })));
      setSelectedModels(channel.selected_models || []);
    } else {
      setForm(DEFAULT_FORM);
      setAvailableModels([]);
      setSelectedModels([]);
    }
   }, [channel, open]);

  const apiKeys = useMemo(() => form.api_key.split('\n').map((key) => key.trim()).filter(Boolean), [form.api_key]);
  const primaryApiKey = apiKeys[0] ?? '';

  // URL 检测
  useEffect(() => {
    const seq = ++probeSeqRef.current;
    if (!form.base_url.trim()) {
      setUrlProbe(null);
      setProbingUrl(false);
      return;
    }
    setUrlProbe(null);
    setProbingUrl(true);
    const timer = setTimeout(async () => {
      try {
        const result = await api.channels.probeUrl(form.base_url.trim(), form.api_type, primaryApiKey);
        if (probeSeqRef.current === seq) {
          setUrlProbe(result as UrlProbeResult);
          if (result.reachable) {
            const types = result.available_types?.length
              ? result.available_types
              : result.detected_type
                ? [result.detected_type]
                : [];
            setAvailableProtocols(types);
            if (result.detected_type || result.corrected_base_url) {
              setForm(prev => ({
                ...prev,
                api_type: result.detected_type || prev.api_type,
                base_url: result.corrected_base_url || prev.base_url,
              }));
            }
          }
        }
      } catch {
        if (probeSeqRef.current === seq) {
          setUrlProbe({ reachable: false, status_code: undefined, latency_ms: 0, detected_type: undefined, corrected_base_url: undefined, message: t('channel.editor.probeFailedGeneric', 'Probe failed') });
        }
      } finally {
        if (probeSeqRef.current === seq) {
          setProbingUrl(false);
        }
      }
    }, 800);
    return () => clearTimeout(timer);
  }, [api, form.api_type, form.base_url, primaryApiKey, t]);

  // canSave 必须检查 4 项：name, api_type, base_url, api_key 都有内容
  const canSave = !!(form.name.trim() && form.api_type.trim() && form.base_url.trim() && primaryApiKey);
  
  // canFetchModels: 基础输入满足即可尝试，URL 探测失败不阻塞获取模型
  const canFetchModels = !!(form.name.trim() && form.api_type.trim() && form.base_url.trim() && primaryApiKey && !fetchingModels);

  const setValue = <K extends keyof ChannelFormState>(key: K, value: ChannelFormState[K]) => {
    if (key === 'api_type' || key === 'base_url' || key === 'api_key') {
      setModelsValidated(false);
      setUrlProbe(null);
      setAvailableModels([]);
      setSelectedModels([]);
      if (key !== 'api_type') setAvailableProtocols([]);
    }
    setForm((current) => ({ ...current, [key]: value }));
  };

  const handleApiTypeChange = (type: string) => {
    setForm((prev) => ({
      ...prev,
      api_type: type,
      base_url: prev.base_url,
    }));
    setAvailableModels([]);
    setSelectedModels([]);
    setModelsValidated(false);
  };

  useEffect(() => {
    if (!open || !channel?.id) {
      existingModelsRef.current = new Set();
      return;
    }
    let cancelled = false;
    api.pool.list()
      .then((entries) => {
        if (cancelled) return;
        existingModelsRef.current = new Set(
          entries
            .filter((entry) => entry.channel_id === channel.id)
            .map((entry) => entry.model.toLowerCase()),
        );
      })
      .catch(() => {
        if (!cancelled) {
          existingModelsRef.current = new Set();
        }
      });
    return () => {
      cancelled = true;
    };
  }, [api, channel?.id, open]);

  // 自动选择模型：所选时间范围内发布的模型 + 已存在模型 + 当前临时创建模型
  const autoSelectModels = useCallback((models: EditorModelInfo[]): string[] => {
    const rangeStart = new Date();
    rangeStart.setMonth(rangeStart.getMonth() - timeRange);
    const rangeStartStr = rangeStart.toISOString().slice(0, 10);

    const selected = new Set<string>();

    for (const model of models) {
      const catalog = getCatalogModel(model.name);
      if (model.temporary || (catalog?.release_date && formatReleaseDate(catalog.release_date) >= rangeStartStr)) {
        selected.add(model.name);
      }
    }

    for (const model of models) {
      if (existingModelsRef.current.has(model.name.toLowerCase())) {
        selected.add(model.name);
      }
    }

    return Array.from(selected);
  }, [timeRange]);

  useEffect(() => {
    if (!showModels || availableModels.length === 0) return;
    setSelectedModels(autoSelectModels(availableModels));
  }, [showModels, availableModels, autoSelectModels]);

  const createEditorModels = (models: Array<Record<string, unknown>>, sourceProtocol: string): EditorModelInfo[] =>
    models.map((item, index) => ({
      id: String(item.id ?? item.name ?? index),
      name: String(item.name ?? ''),
      owned_by: typeof item.owned_by === 'string' ? item.owned_by : undefined,
      sourceProtocol,
    })).filter((item) => item.name);

  const mergeModelsByName = (groups: EditorModelInfo[][]): EditorModelInfo[] => {
    const map = new Map<string, EditorModelInfo>();
    for (const group of groups) {
      for (const model of group) {
        const key = model.name.toLowerCase();
        if (!map.has(key)) map.set(key, model);
      }
    }
    return Array.from(map.values());
  };

  const withTimeout = async <T,>(promise: Promise<T>, ms: number, message: string): Promise<T> => {
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(() => reject(new Error(message)), ms);
    });
    try {
      return await Promise.race([promise, timeout]);
    } finally {
      if (timer) clearTimeout(timer);
    }
  };

  const fetchModelsByProtocol = async (apiType: string): Promise<{ result: FetchModelsResult; models: EditorModelInfo[] }> => {
    const result = await api.channels.fetchModelsDirect(apiType, form.base_url, primaryApiKey, false);
    if (result.models.length > 0) {
      setAvailableProtocols(prev => Array.from(new Set([...prev, apiType])));
    }
    const sourceProtocol = result.detected_type || apiType;
    return {
      result,
      models: createEditorModels(result.models || [], sourceProtocol),
    };
  };

  // 获取模型列表
  const handleFetchModels = async () => {
    const seq = ++fetchSeqRef.current;

    setFetchingModels(true);
    setModelsValidated(false);
    try {
      const fetched = await withTimeout(
        fetchModelsByProtocol(form.api_type),
        10_000,
        t('channel.editor.fetchModelsTimeout', '获取模型超时'),
      );
      if (fetchSeqRef.current !== seq) return;

      const finalModels = mergeModelsByName([fetched.models]);
      setShowModels(true);
      setSelectedModels([]);
      setAvailableModels(finalModels);
      setModelTestResults({});
      setModelsValidated(true);
      const nextSelected = autoSelectModels(finalModels);
      if (fetchSeqRef.current !== seq) return;
      setSelectedModels(nextSelected);

      if (finalModels.length === 0) {
        toast.warning(t('channel.editor.noModelsFetched', '未获取到模型'));
      }
    } catch (err) {
      if (fetchSeqRef.current === seq) {
        toast.error(getChannelErrorMessage(err, t('channel.editor.fetchModelsFailed', '获取模型列表失败')));
      }
    } finally {
      if (fetchSeqRef.current === seq) {
        setFetchingModels(false);
      }
    }
  };

  const toggleModel = (modelName: string) => {
    setSelectedModels((prev) =>
      prev.includes(modelName)
        ? prev.filter((m) => m !== modelName)
        : [...prev, modelName],
    );
  };

  const selectAllFiltered = () => {
    const names = filteredModels.map((m) => m.name);
    setSelectedModels((prev) => Array.from(new Set([...prev, ...names])));
  };

  const clearAllSelected = () => {
    if (modelSearch.trim()) {
      const namesToClear = new Set(filteredModels.map((model) => model.name));
      setSelectedModels((prev) => prev.filter((name) => !namesToClear.has(name)));
      return;
    }
    setSelectedModels([]);
  };

  const createTemporaryModel = () => {
    const name = modelSearch.trim();
    if (!name) return;
    const exists = availableModels.some((model) => model.name.toLowerCase() === name.toLowerCase());
    if (exists) return;
    const model: EditorModelInfo = {
      id: `temp-${name}`,
      name,
      sourceProtocol: form.api_type,
      temporary: true,
    };
    setAvailableModels((prev) => [...prev, model]);
    setSelectedModels((prev) => Array.from(new Set([...prev, name])));
  };

  const handleTestModels = async () => {
    if (filteredModels.length > 30) {
      toast.error(t('channel.editor.tooManyModelsToTest', '模型数量过多（{{count}}个），测速耗时较长，请减少筛选范围', { count: filteredModels.length }));
      return;
    }

    const seq = ++testSeqRef.current;
    setTestingModels(true);
    setModelTestResults({});
    const results: Record<string, { success: boolean; latency?: number; reason?: string }> = {};
    try {
      await withTimeout(
        (async () => {
          for (const model of filteredModels) {
            if (testSeqRef.current !== seq) return;
            try {
              const result = await api.channels.testChannelDirect({
                api_type: form.api_type,
                base_url: form.base_url,
                api_key: primaryApiKey,
                model: model.name,
              });
              results[model.name] = result.success
                ? { success: true, latency: result.latency_ms }
                : { success: false, reason: result.message };
            } catch (err) {
              results[model.name] = { success: false, reason: err instanceof Error ? err.message : String(err) };
            }
            if (testSeqRef.current === seq) setModelTestResults({ ...results });
          }
        })(),
        10_000,
        t('channel.editor.testModelsTimeout', '模型测速超时'),
      );
    } catch (err) {
      if (testSeqRef.current === seq) {
        toast.error(err instanceof Error ? err.message : String(err));
      }
    } finally {
      if (testSeqRef.current === seq) {
        setTestingModels(false);
      }
    }
  };



  const handleSave = async () => {
    if (saving || fetchingModels) return;
    if (!canSave) {
      toast.error(t('channel.editor.requiredFieldsHint', '请确保填写有效的 Base URL 与 API Key 后再保存'));
      return;
    }
    setSaving(true);
    try {
      const keys = form.id ? [primaryApiKey] : apiKeys;
      const successNames: string[] = [];
      const failedNames: string[] = [];

      for (let i = 0; i < keys.length; i++) {
        const key = keys[i];
        const name = keys.length > 1 ? `${form.name}-${i + 1}` : form.name;
        
        const saveParams: SaveChannelWithModelsParams = {
          id: form.id || undefined,
          name: name,
          api_type: form.api_type,
          base_url: form.base_url,
          api_key: key,
          notes: form.notes,
          enabled: form.enabled,
          selected_models: selectedModels,
          available_models: availableModels,
          catalog_meta: selectedModels.map(buildEntryCatalogMeta),
          response_ms: urlProbe?.reachable && urlProbe.latency_ms > 0 ? String(urlProbe.latency_ms) : undefined,
        };

        try {
          const result = await api.channels.saveChannelWithModels(saveParams);
          successNames.push(name);
          if (result.warnings && result.warnings.length > 0) {
            result.warnings.forEach((w: string) => { toast.warning(w); });
          }
        } catch (err) {
          failedNames.push(name);
          if (keys.length === 1) throw err;
          toast.error(`${name}: ${getChannelErrorMessage(err, t('channel.editor.saveFailed', '保存渠道失败'))}`);
        }
      }

      if (successNames.length > 0 && keys.length > 1) {
        toast.success(t('channel.editor.batchSaveSummary', '批量创建完成：成功 {{success}} 个，失败 {{failed}} 个', { success: successNames.length, failed: failedNames.length }));
      }
      if (successNames.length === 0 && failedNames.length > 0) {
        throw new Error(t('channel.editor.batchSaveAllFailed', '批量创建全部失败'));
      }

      await onSaved();
      queryClient.invalidateQueries({ queryKey: ['entries'] });
      onOpenChange(false);
    } catch (err) {
      toast.error(getChannelErrorMessage(err, t('channel.editor.saveFailed', '保存渠道失败')));
    } finally {
      setSaving(false);
    }
  };

  const handleClose = () => {
    queryClient.invalidateQueries({ queryKey: ['channels'] });
    queryClient.invalidateQueries({ queryKey: ['entries'] });
    onOpenChange(false);
  };

  const filteredModels = useMemo(() => {
    if (!modelSearch) return availableModels;
    return availableModels.filter((m) => m.name.toLowerCase().includes(modelSearch.toLowerCase()));
  }, [availableModels, modelSearch]);


return (
    <Dialog open={open} onOpenChange={(value) => {
      if (!value) setSaving(false);
      if (!value) {
        queryClient.invalidateQueries({ queryKey: ['channels'] });
        queryClient.invalidateQueries({ queryKey: ['entries'] });
      }
      onOpenChange(value);
    }}>
      <DialogContent className={cn(
        "flex flex-col",
        showModels ? "sm:max-w-3xl max-w-3xl" : "max-w-[500px]"
      )}>
        <DialogHeader>
          <DialogTitle>{channel ? t('channel.editor.editTitle') : t('channel.editor.title')}</DialogTitle>
        </DialogHeader>

        <div className={cn(
          "flex-1 min-h-0 overflow-auto",
          showModels && "grid grid-cols-2 gap-3"
        )}>
          {/* 渠道信息区 */}
          <div className={cn(
            "space-y-4 pb-4",
            showModels && "min-w-0 border-r pr-3"
          )}>

            <div className="space-y-2">
              <Label htmlFor="channel-name">{t('channel.editor.channelName')}</Label>
              <Input id="channel-name" value={form.name} onChange={(event) => setValue('name', event.target.value)} placeholder={t('channel.form.placeholderName')} />
            </div>

            <div className="space-y-2">
              <Label htmlFor="channel-apitype">{t('channel.editor.apiType')}</Label>
              <Select value={form.api_type} onValueChange={handleApiTypeChange}>
                <SelectTrigger id="channel-apitype">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {API_TYPES.map((item) => {
                    const isAvailable = availableProtocols.includes(item.value);
                    return (
                      <SelectItem
                        key={item.value}
                        value={item.value}
                        className={cn(isAvailable && "bg-green-600 text-white data-[highlighted]:bg-green-700 data-[highlighted]:text-white")}
                      >
                        {item.label}
                      </SelectItem>
                    );
                  })}
                </SelectContent>
              </Select>
            </div>

            <div className="space-y-2">
              <Label htmlFor="channel-baseurl">{t('channel.editor.baseUrl')}</Label>
              <div className="relative">
                <Input
                  id="channel-baseurl"
                  value={form.base_url}
                  onChange={(event) => setValue('base_url', event.target.value)}
                  placeholder={t('channel.form.placeholderBaseUrl')}
                  className={urlProbe ? (urlProbe.reachable ? 'pr-24 border-green-500/50 focus-visible:ring-green-500/30' : 'pr-24 border-red-500/50 focus-visible:ring-red-500/30') : 'pr-8'}
                />
                <div className="absolute right-1.5 top-1/2 -translate-y-1/2 flex items-center gap-1 pointer-events-none">
                  {probingUrl ? (
                    <RefreshCw className="h-3.5 w-3.5 animate-spin text-muted-foreground" />
                  ) : urlProbe?.reachable ? (
                    <span className="text-[10px] text-green-600 font-medium whitespace-nowrap">✓ {urlProbe.latency_ms}ms</span>
                  ) : urlProbe ? (
                    <span className="text-[10px] text-red-500" title={urlProbe.message}>✗</span>
                  ) : null}
                </div>
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="channel-apikey">{t('channel.editor.apiKey')}</Label>
              <div className="relative">
                <textarea
                  id="channel-apikey"
                  rows={1}
                  value={form.api_key}
                  onChange={(event) => setValue('api_key', event.target.value)}
                  placeholder={channel ? undefined : t('channel.editor.multiKeyPlaceholder', '每行一个 API Key；获取模型使用第一行，保存时按行批量创建')}
                  className="flex min-h-9 w-full resize-y rounded-md border border-input bg-transparent px-3 py-2 pr-10 text-sm shadow-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
                  style={{ WebkitTextSecurity: showApiKey ? 'none' : 'disc' } as React.CSSProperties}
                />
                <Button type="button" variant="ghost" size="icon" className="absolute right-0 top-0 h-9 px-3 hover:bg-transparent" onClick={() => setShowApiKey(!showApiKey)}>
                  {showApiKey ? t('channel.editor.hidePassword') : t('channel.editor.showPassword')}
                </Button>
              </div>
            </div>

            <div className="space-y-2">
              <Label htmlFor="channel-notes">{t('channel.editor.notes')}</Label>
              <textarea
                id="channel-notes"
                value={form.notes}
                onChange={(event) => setValue('notes', event.target.value)}
                rows={2}
                className="flex min-h-14 w-full resize-y rounded-md border border-input bg-transparent px-3 py-1.5 text-sm shadow-sm outline-none transition-colors placeholder:text-muted-foreground focus-visible:ring-1 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
              />
            </div>

            {/* 获取模型按钮 - fill 宽度长条按钮，位于渠道区底部 */}
            <Button 
              className="w-full gap-1.5" 
              onClick={handleFetchModels} 
              disabled={!canFetchModels || fetchingModels}
            >
              <RefreshCw className={cn('h-4 w-4', fetchingModels && 'animate-spin')} />
              {fetchingModels ? t('channel.editor.fetching') : t('channel.editor.fetchModels')}
            </Button>
          </div>

          {/* 模型信息区 - 仅在展开时显示 */}
          {showModels && (
            <div className="min-w-0 space-y-3 pl-3 pr-4 pt-4">
              {/* 时间范围选择：3个月/6个月/12个月 */}
              <div className="flex overflow-hidden rounded-md border border-input">
                {([3, 6, 12] as const).map((months) => (
                  <Button
                    key={months}
                    size="sm"
                    variant={timeRange === months ? "default" : "ghost"}
                    onClick={() => setTimeRange(months)}
                    className="flex-1 rounded-none border-0 border-r last:border-r-0"
                  >
                    {t('channel.editor.months', { count: months, defaultValue: `${months}个月` })}
                  </Button>
                ))}
              </div>

              {/* 搜索和操作按钮 */}
              <div className="flex flex-wrap gap-2 items-center">
                <Input 
                  id="model-search" 
                  placeholder={t('channel.editor.searchPlaceholder', '搜索/创建模型')} 
                  value={modelSearch} 
                  onChange={(e) => setModelSearch(e.target.value)} 
                  className="h-8 text-sm flex-1 min-w-48" 
                />
                <Button size="sm" variant="outline" onClick={selectAllFiltered}>{t('common.selectAll', '全选')}</Button>
                <Button size="sm" variant="outline" onClick={clearAllSelected}>{t('common.clear', '清除')}</Button>
              </div>

              {/* 模型列表 */}
              <div className="h-[262px] overflow-y-auto rounded-md border border-border bg-background pr-2">
                {filteredModels.map((model) => {
                  const testResult = modelTestResults[model.name];
                  return (
                    <label key={model.id || model.name} htmlFor={`model-${model.id || model.name}`} className="flex cursor-pointer items-center gap-2 border-b border-border py-2 pl-3 pr-1 text-sm last:border-b-0 hover:bg-accent">
                      <Checkbox id={`model-${model.id || model.name}`} checked={selectedModels.includes(model.name)} onCheckedChange={() => toggleModel(model.name)} />
                      <span className={cn(
                        "truncate",
                        testResult?.success === true && "text-green-600",
                        testResult?.success === false && "text-red-500"
                      )} title={testResult?.reason}>
                        {model.name}
                        {testResult?.success === true && testResult.latency && (
                          <span className="text-xs ml-1">({(testResult.latency / 1000).toFixed(2)}s)</span>
                        )}
                        {testResult?.success === false && (
                          <span className="text-xs ml-1">(失败)</span>
                        )}
                      </span>
                      <span className="ml-auto shrink-0 text-xs text-muted-foreground bg-muted px-1.5 py-0.5 rounded">
                        {model.sourceProtocol}
                      </span>
                    </label>
                  );
                })}
                {filteredModels.length === 0 && modelSearch.trim() ? (
                  <button
                    type="button"
                    className="flex w-full items-center justify-between px-3 py-2 text-left text-sm hover:bg-accent"
                    onClick={createTemporaryModel}
                  >
                    <span>{t('channel.editor.createModel', { name: modelSearch.trim(), defaultValue: `创建模型「${modelSearch.trim()}」` })}</span>
                    <span className="text-xs text-muted-foreground bg-muted px-1.5 py-0.5 rounded">{form.api_type}</span>
                  </button>
                ) : null}
                {filteredModels.length === 0 && !modelSearch.trim() ? (
                  <div className="px-3 py-6 text-center text-sm text-muted-foreground">
                    {modelsValidated ? t('channel.editor.emptyPlaceholder2') : t('channel.editor.emptyPlaceholder')}
                  </div>
                ) : null}
              </div>

              <div className="text-right text-[11px] leading-none text-muted-foreground">
                {selectedModels.length}/{availableModels.length}
              </div>

              {/* 模型测速按钮 - fill 宽度 */}
              <Button 
                className="w-full gap-1.5" 
                variant="outline"
                onClick={handleTestModels} 
                disabled={testingModels || filteredModels.length === 0}
              >
                <Zap className={cn('h-4 w-4', testingModels && 'animate-pulse')} />
                {testingModels 
                  ? t('channel.editor.testingModels', '测速中...') 
                  : t('channel.editor.testModels', '模型测速')
                }
                {filteredModels.length > 0 && !testingModels && (
                  <span className="text-xs text-muted-foreground ml-1">
                    ({filteredModels.length})
                  </span>
                )}
              </Button>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={handleClose} disabled={saving || fetchingModels} className="gap-1.5"><X className="h-4 w-4" />{t('common.cancel')}</Button>
          <Button onClick={handleSave} disabled={saving || fetchingModels} className="gap-1.5">
            <Save className="h-4 w-4" />
            {saving ? t('channel.editor.saving') : t('common.save')}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
};




