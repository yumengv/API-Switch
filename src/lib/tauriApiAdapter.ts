import { invoke } from '@tauri-apps/api/core';
import type { ApiAdapter } from './apiAdapter';
import type { Channel, CreateChannelParams, UpdateChannelParams, FetchModelsResult, ProbeResult, ModelInfo, ModelCatalogMetaUpdate } from '../features/channels/types';
import type { DashboardFilter, DashboardStats, ChartDataPoint, ModelRanking, UsageLog, UsageLogFilter, PaginatedResult, ApiEntry, AccessKey, TranslationRelayPayload, TranslationRelayRequest } from '../types';
import {
  listEntries,
  toggleEntry,
  reorderEntries,
  createEntry,
  deleteEntry,
  testEntryLatency,
  backfillEntryCatalogMeta,
  listAccessKeys,
  createAccessKey,
  deleteAccessKey,
  toggleAccessKey,
  getSettings,
  updateSettings,
  getProxyStatus,
  startProxy,
  stopProxy,
  testChat,
} from './api';

export const tauriApiAdapter: ApiAdapter = {
  channels: {
    async list() {
      return await invoke<Channel[]>('list_channels');
    },
    async create(params) {
      return await invoke<Channel>('create_channel', { params });
    },
    async update(params) {
      return await invoke<Channel>('update_channel', { params });
    },
    async delete(id) {
      await invoke('delete_channel', { id });
    },
    async fetchModels(channelId) {
      return await invoke<FetchModelsResult>('fetch_models', { channelId });
    },
    async fetchModelsDirect(apiType, baseUrl, apiKey, verified) {
      return await invoke<FetchModelsResult>('fetch_models_direct', { apiType, baseUrl, apiKey, verified });
    },
    async probeUrl(url) {
      return await invoke<ProbeResult>('probe_url', { url });
    },
    async selectModels(channelId, modelNames, availableModels, catalogMeta = []) {
      await invoke('select_models', { channelId, modelNames, availableModels, catalogMeta });
    },
    async updateResponseMs(channelId, responseMs) {
      await invoke('update_channel_response_ms', { params: { channelId, responseMs } });
    },
  },
  usage: {
    async getLogs(filter) {
      return await invoke<PaginatedResult<UsageLog>>('get_usage_logs', { filter });
    },
    async getDashboardStats(filter) {
      return await invoke<DashboardStats>('get_dashboard_stats', { filter });
    },
    async getModelConsumption(filter) {
      return await invoke<ChartDataPoint[]>('get_model_consumption', { filter });
    },
    async getCallTrend(filter) {
      return await invoke<ChartDataPoint[]>('get_call_trend', { filter });
    },
    async getModelDistribution(filter) {
      return await invoke<ModelRanking[]>('get_model_distribution', { filter });
    },
    async getUserTrend(filter) {
      return await invoke<ChartDataPoint[]>('get_user_trend', { filter });
    },
  },
  pool: {
    list: listEntries,
    toggle: (id, enabled) => toggleEntry(id, enabled),
    reorder: reorderEntries,
    create: (params) => createEntry({ channel_id: params.channelId, model: params.model, display_name: params.displayName, group_name: params.groupName }),
    delete: deleteEntry,
    testLatency: async (id) => {
      const result = await testEntryLatency(id);
      return {
        entry_id: id,
        latency_ms: result.status === 'ok' && result.response_ms !== 'X' ? parseInt(result.response_ms, 10) : null,
      };
    },
    backfillCatalogMeta: (items) =>
      backfillEntryCatalogMeta(
        items.map((item) => ({
          id: item.entryId,
          provider_logo: '',
          release_date: '',
          model_meta_zh: '',
          model_meta_en: '',
        }))
      ),
    getGroups: () => invoke<string[]>('get_all_groups'),
    updateGroup: (id, groupName) => invoke('update_entry_group', { id, groupName }),
  },
  tokens: {
    list: listAccessKeys,
    create: createAccessKey,
    delete: deleteAccessKey,
    toggle: toggleAccessKey,
  },
settings: {
    get: getSettings,
    update: updateSettings,
    patchSettings: async (patch) => {
        // Tauri command 使用完整对象，fallback 到完整 update
        const current = await getSettings();
        await updateSettings({ ...current, ...patch });
        return { ...current, ...patch };
    },
},
  proxy: {
    getStatus: getProxyStatus,
    start: startProxy,
    stop: stopProxy,
  },
  testChat: (entryId, messages) => testChat(entryId, messages),
  translation: {
    getLatest: () => invoke<TranslationRelayPayload | null>('get_translation_relay'),
    translateAndRelay: (request: TranslationRelayRequest) => invoke<TranslationRelayPayload>('translate_and_relay', { request }),
  },
  // NOTE: Uses raw fetch instead of Tauri invoke because there is no `get_version` command.
  // In Combined mode the admin server is merged into the proxy port, so /admin/version works.
  // This path MUST stay as '/admin/version' to match the Rust route in admin/router.rs.
  // Do NOT refactor this to use ADMIN_API_PREFIX — desktop stability depends on this literal.
  async getVersion() {
    const response = await fetch('/admin/version');
    return response.json();
  },
};
