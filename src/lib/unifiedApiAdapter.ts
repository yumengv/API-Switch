import type { ApiAdapter } from './apiAdapter';
import type {
  Channel,
  CreateChannelParams,
  UpdateChannelParams,
  FetchModelsResult,
  ProbeResult,
  TestChannelResult,
  TestChannelDirectParams,
  ModelInfo,
  ModelCatalogMetaUpdate,
  SaveChannelWithModelsParams,
  SaveChannelWithModelsResult,
} from '../features/channels/types';
import type {
  DashboardFilter,
  DashboardStats,
  ChartDataPoint,
  ModelRanking,
  UsageLog,
  UsageLogFilter,
  PaginatedResult,
  ApiEntry,
  AccessKey,
  AppSettings,
  ProxyStatus,
  AdminStatus,
  TestChatResponse,
  TranslationRelayPayload,
  TranslationRelayRequest,
} from '../types';
import { ADMIN_API_PREFIX } from './adminApiConfig';
import { clearToken, emitAuthExpired, TOKEN_KEY } from './webAuth';

// ============================================================
// Runtime detection
// ============================================================

function isTauriRuntime(): boolean {
  if (typeof window === 'undefined') return false;
  const w = window as Window & { __TAURI__?: unknown; __TAURI_INTERNALS__?: unknown };
  return typeof w.__TAURI__ !== 'undefined' || typeof w.__TAURI_INTERNALS__ !== 'undefined';
}

// ============================================================
// Transport helpers
// ============================================================

/**
 * Call a Tauri command via IPC. Dynamic import ensures the
 * @tauri-apps/api/core module is never bundled in web builds.
 */
async function tauriCmd<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const { invoke } = await import('@tauri-apps/api/core');
  return invoke<T>(cmd, args ?? {});
}

// ------------------------------------------------------------------
// Web-side HTTP helpers (reproduced from webAdminApiAdapter.ts)
// ------------------------------------------------------------------

let lastSettingsVersion = 0;
const lastDirtyStateVersions: Partial<Record<'log' | 'pool' | 'channel' | 'token', number>> = {};

interface ChannelOperationError {
  code: string;
  message: string;
  details?: Record<string, unknown>;
}

type ChannelOperationErrorKind =
  | 'auth'
  | 'timeout'
  | 'network'
  | 'rate_limited'
  | 'invalid_url'
  | 'unsupported_provider'
  | 'empty_model_list'
  | 'endpoint_correction_failed'
  | 'unknown';

interface ChannelOperationHttpError extends Error {
  status: number;
  error?: ChannelOperationError;
  kind: ChannelOperationErrorKind;
  isNetworkError: boolean;
  isAuthError: boolean;
  isTimeoutError: boolean;
}

function classifyChannelOperationError(status: number, error?: ChannelOperationError): ChannelOperationErrorKind {
  switch (error?.code) {
    case 'INVALID_CREDENTIALS':
    case 'UNAUTHORIZED':
    case 'FORBIDDEN':
      return 'auth';
    case 'TIMEOUT':
      return 'timeout';
    case 'ENDPOINT_UNREACHABLE':
      return 'network';
    case 'RATE_LIMITED':
      return 'rate_limited';
    case 'INVALID_URL':
      return 'invalid_url';
    case 'UNSUPPORTED_PROVIDER':
      return 'unsupported_provider';
    case 'EMPTY_MODEL_LIST':
      return 'empty_model_list';
    case 'ENDPOINT_CORRECTION_FAILED':
    case 'ENDPOINT_VALIDATION_FAILED':
      return 'endpoint_correction_failed';
    default:
      if (status === 401 || status === 403) return 'auth';
      return 'unknown';
  }
}

function createHttpError(status: number, fallbackMessage: string, error?: ChannelOperationError): ChannelOperationHttpError {
  const kind = classifyChannelOperationError(status, error);
  const instance = new Error(error?.message || fallbackMessage) as ChannelOperationHttpError;
  instance.name = 'ChannelOperationHttpError';
  instance.status = status;
  instance.error = error;
  instance.kind = kind;
  instance.isNetworkError = kind === 'network';
  instance.isAuthError = kind === 'auth';
  instance.isTimeoutError = kind === 'timeout';
  return instance;
}

async function webRequest<T>(
  method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH',
  path: string,
  data?: unknown,
  queryParams?: Record<string, unknown> | null,
): Promise<T> {
  const token = localStorage.getItem(TOKEN_KEY);

  let url = `${ADMIN_API_PREFIX}${path}`;
  if (queryParams) {
    const searchParams = new URLSearchParams();
    Object.entries(queryParams).forEach(([key, value]) => {
      if (value !== undefined && value !== null) searchParams.append(key, String(value));
    });
    const qs = searchParams.toString();
    if (qs) url += `?${qs}`;
  }

  let response: Response;
  try {
    response = await fetch(url, {
      method,
      headers: {
        'Content-Type': 'application/json',
        ...(token ? { Authorization: `Bearer ${token}` } : {}),
      },
      body: data === undefined ? undefined : JSON.stringify(data),
    });
  } catch (cause) {
    const rawMessage = cause instanceof Error ? cause.message : String(cause);
    const error: ChannelOperationError = {
      code: 'ENDPOINT_UNREACHABLE',
      message: rawMessage,
      details: { path, method },
    };
    throw createHttpError(0, rawMessage, error);
  }

  if (!response.ok) {
    let error: ChannelOperationError | undefined;
    let message = `HTTP ${response.status}`;
    try {
      const body = (await response.json()) as { error?: ChannelOperationError };
      error = body.error;
      message = error?.message || message;
    } catch {
      // ignore non-json response
    }
    if (response.status === 401 || response.status === 403) {
      clearToken();
      emitAuthExpired({ status: response.status, message });
    }
    throw createHttpError(response.status, message, error);
  }

  if (response.status === 204) return undefined as T;

  return response.json() as Promise<T>;
}

// ============================================================
// Runtime dispatcher
// ============================================================

function useTauri(): boolean {
  return isTauriRuntime();
}

// ============================================================
// Unified adapter 鈥?single source of truth for every endpoint
// ============================================================

export const apiAdapter: ApiAdapter = {
  channels: {
    list: () =>
      useTauri()
        ? tauriCmd<Channel[]>('list_channels')
        : webRequest<Channel[]>('GET', '/channels'),

    listPaginated: ({ page, pageSize }) =>
      useTauri()
        ? tauriCmd<PaginatedResult<Channel>>('list_channels_paginated', { page, pageSize })
        : webRequest<PaginatedResult<Channel>>('GET', '/channels/paginated', undefined, {
            page,
            page_size: pageSize,
          }),

    create: (params) =>
      useTauri()
        ? tauriCmd<Channel>('create_channel', { params })
        : webRequest<Channel>('POST', '/channels', params),

    update: (params) =>
      useTauri()
        ? tauriCmd<Channel>('update_channel', { params })
        : webRequest<Channel>('PUT', `/channels/${params.id}`, params),

    delete: (id) =>
      useTauri()
        ? tauriCmd<void>('delete_channel', { id })
        : webRequest<void>('DELETE', `/channels/${id}`),

    fetchModels: (channelId) =>
      useTauri()
        ? tauriCmd<FetchModelsResult>('fetch_models', { channelId })
        : webRequest<FetchModelsResult>('POST', `/channels/${channelId}/fetch-models`),

    fetchModelsDirect: (apiType, baseUrl, apiKey, verified) =>
      useTauri()
        ? tauriCmd<FetchModelsResult>('fetch_models_direct', { apiType, baseUrl, apiKey, verified })
        : webRequest<FetchModelsResult>('POST', '/channels/fetch-models-direct', { apiType, baseUrl, apiKey, verified }),

    probeUrl: (url, apiType, apiKey) =>
      useTauri()
        ? tauriCmd<ProbeResult>('probe_url', { url, apiType, apiKey })
        : webRequest<ProbeResult>('POST', '/channels/probe-url', { url, apiType, apiKey }),

    selectModels: (channelId, modelNames, availableModels, catalogMeta = []) =>
      useTauri()
        ? tauriCmd<void>('select_models', { channelId, modelNames, availableModels, catalogMeta })
        : webRequest<void>('POST', `/channels/${channelId}/select-models`, { modelNames, availableModels, catalogMeta }),

    updateResponseMs: (channelId, responseMs) =>
      useTauri()
        ? tauriCmd<void>('update_channel_response_ms', { params: { channelId, responseMs } })
        : webRequest<void>('PUT', `/channels/${channelId}/response-ms`, { channelId, responseMs }),

    saveChannelWithModels: (params) =>
      useTauri()
        ? tauriCmd<SaveChannelWithModelsResult>('save_channel_with_models', { params })
        : webRequest<SaveChannelWithModelsResult>('POST', '/channels/save-with-models', params),

    testChannel: (channelId) =>
      useTauri()
        ? tauriCmd<TestChannelResult>('test_channel', { channelId })
        : webRequest<TestChannelResult>('POST', `/channels/${channelId}/test`),

    testChannelDirect: (params: TestChannelDirectParams) =>
      useTauri()
        ? tauriCmd<TestChannelResult>('test_channel_direct', { params })
        : webRequest<TestChannelResult>('POST', '/channels/test-direct', params),
  },

  usage: {
    getLogs: (filter) =>
      useTauri()
        ? tauriCmd<PaginatedResult<UsageLog>>('get_usage_logs', { filter })
        : webRequest<PaginatedResult<UsageLog>>('GET', '/logs', undefined, filter as Record<string, unknown>),

    getDashboardStats: (filter) =>
      useTauri()
        ? tauriCmd<DashboardStats>('get_dashboard_stats', { filter })
        : webRequest<DashboardStats>('GET', '/dashboard/stats', undefined, filter as Record<string, unknown>),

    getModelConsumption: (filter) =>
      useTauri()
        ? tauriCmd<ChartDataPoint[]>('get_model_consumption', { filter })
        : webRequest<ChartDataPoint[]>('GET', '/dashboard/model-consumption', undefined, filter as Record<string, unknown>),

    getCallTrend: (filter) =>
      useTauri()
        ? tauriCmd<ChartDataPoint[]>('get_call_trend', { filter })
        : webRequest<ChartDataPoint[]>('GET', '/dashboard/call-trend', undefined, filter as Record<string, unknown>),

    getModelDistribution: (filter) =>
      useTauri()
        ? tauriCmd<ModelRanking[]>('get_model_distribution', { filter })
        : webRequest<ModelRanking[]>('GET', '/dashboard/model-distribution', undefined, filter as Record<string, unknown>),

    getUserTrend: (filter) =>
      useTauri()
        ? tauriCmd<ChartDataPoint[]>('get_user_trend', { filter })
        : webRequest<ChartDataPoint[]>('GET', '/dashboard/user-trend', undefined, filter as Record<string, unknown>),
  },

  pool: {
    list: () =>
      useTauri()
        ? tauriCmd<ApiEntry[]>('list_entries')
        : webRequest<ApiEntry[]>('GET', '/pool'),

    listPaginated: ({ page, pageSize, groupName, search, channelId }) =>
      useTauri()
        ? tauriCmd<PaginatedResult<ApiEntry>>('list_entries_paginated', { page, pageSize, groupName, search, channelId })
        : webRequest<PaginatedResult<ApiEntry>>('GET', '/pool/paginated', undefined, {
            page,
            page_size: pageSize,
            group_name: groupName,
            search,
            channel_id: channelId,
          }),

    toggle: (id, enabled) =>
      useTauri()
        ? tauriCmd<void>('toggle_entry', { id, enabled })
        : webRequest<void>('PUT', `/pool/${id}/toggle`, enabled),

    batchToggle: (ids, enabled) =>
      useTauri()
        ? tauriCmd<void>('batch_toggle_entries', { ids, enabled })
        : Promise.all(ids.map((id) =>
            webRequest<void>('PUT', `/pool/${id}/toggle`, enabled)
          )).then(() => {}),

    reorder: (orderedIds) =>
      useTauri()
        ? tauriCmd<void>('reorder_entries', { orderedIds })
        : webRequest<void>('POST', '/pool/reorder', { ordered_ids: orderedIds }),

    create: (params) =>
      useTauri()
        ? tauriCmd<ApiEntry>('create_entry', { params: { channel_id: params.channelId, model: params.model, display_name: params.displayName, group_name: params.groupName } })
        : webRequest<ApiEntry>('POST', '/pool', {
            channel_id: params.channelId,
            model: params.model,
            display_name: params.displayName,
            group_name: params.groupName,
          }),

    delete: (id) =>
      useTauri()
        ? tauriCmd<void>('delete_entry', { id })
        : webRequest<void>('DELETE', `/pool/${id}`),
      updateDisplayName: (id: string, display_name: string) =>
        useTauri()
          ? tauriCmd<void>("update_entry_display_name", { id, displayName: display_name })
          : webRequest<void>("PUT", `/pool/${id}/display-name`, { display_name }),
    testLatency: async (id) => {
      if (useTauri()) {
        const result = await tauriCmd<{ status: string; response_ms: string; error_detail?: string }>('test_entry_latency', { entryId: id });
        return {
          entry_id: id,
          latency_ms: result.status === 'ok' && result.response_ms !== 'X' ? parseInt(result.response_ms, 10) : null,
          error_detail: result.error_detail,
        };
      }
      return webRequest<{ entry_id: string; latency_ms: number | null; error_detail?: string }>('POST', `/pool/${id}/test-latency`);
    },

    backfillCatalogMeta: (items) =>
      useTauri()
        ? tauriCmd<void>('backfill_entry_catalog_meta', {
            items: items.map((item) => ({
              id: item.entryId,
              provider_logo: '',
              release_date: '',
              model_meta_zh: '',
              model_meta_en: '',
            })),
          })
        : webRequest<void>('POST', '/pool/backfill-catalog-meta',
            items.map((item) => ({
              id: item.entryId,
              providerLogo: '',
              releaseDate: '',
              modelMetaZh: '',
              modelMetaEn: '',
            })),
          ),

    getGroups: () =>
      useTauri()
        ? tauriCmd<string[]>('get_all_groups')
        : webRequest<string[]>('GET', '/pool/groups'),

    updateGroup: (id, groupName) =>
      useTauri()
        ? tauriCmd<void>('update_entry_group', { id, groupName })
        : webRequest<void>('PUT', `/pool/${id}/group`, groupName),
  },

  tokens: {
    list: () =>
      useTauri()
        ? tauriCmd<AccessKey[]>('list_access_keys')
        : webRequest<AccessKey[]>('GET', '/tokens'),

    listPaginated: ({ page, pageSize }) =>
      useTauri()
        ? tauriCmd<PaginatedResult<AccessKey>>('list_access_keys_paginated', { page, pageSize })
        : webRequest<PaginatedResult<AccessKey>>('GET', '/tokens/paginated', undefined, {
            page,
            page_size: pageSize,
          }),

    create: (name) =>
      useTauri()
        ? tauriCmd<AccessKey>('create_access_key', { name })
        : webRequest<AccessKey>('POST', '/tokens', { name }),

    delete: (id) =>
      useTauri()
        ? tauriCmd<void>('delete_access_key', { id })
        : webRequest<void>('DELETE', `/tokens/${id}`),

    toggle: (id, enabled) =>
      useTauri()
        ? tauriCmd<void>('toggle_access_key', { id, enabled })
        : webRequest<void>('PUT', `/tokens/${id}/toggle`, enabled),
  },

  settings: {
    get: () =>
      useTauri()
        ? tauriCmd<AppSettings>('get_settings')
        : webRequest<{ data: AppSettings; _version: number }>('GET', '/settings').then((r) => {
            lastSettingsVersion = r._version;
            return r.data;
          }),

    update: async (settings) => {
      if (useTauri()) {
        await tauriCmd<void>('update_settings', { settings });
      } else {
        // Web锛歅UT 鍓嶅厛鑾峰彇鏈€鏂扮増鏈彿锛涚増鏈啿绐佹椂鑷姩閲嶈瘯锛堟渶澶?3 娆★級
        for (let attempt = 0; attempt < 3; attempt++) {
          const latest = await webRequest<{ data: AppSettings; _version: number }>('GET', '/settings');
          try {
            // PUT 杩斿洖 RestartResponse { _version, ... }
            const result = await webRequest<{ _version: number }>('PUT', '/settings', {
              data: settings,
              _version: latest._version,
            });
            lastSettingsVersion = result._version; // PUT 鎴愬姛鍚庢洿鏂扮増鏈彿
            return;
          } catch (err: unknown) {
            const httpErr = err as ChannelOperationHttpError;
            // 鐗堟湰鍐茬獊锛圚TTP 409锛夛細閲嶈瘯鑾峰彇鏈€鏂扮増鏈?
            if (httpErr?.status === 409 && attempt < 2) continue;
            throw err; // 鍏朵粬閿欒鎴栭噸璇曡€楀敖鍒欐姏鍑?
          }
        }
      }
    },

    patchSettings: async (patch) => {
      if (useTauri()) {
        const current = await tauriCmd<AppSettings>('get_settings');
        await tauriCmd<void>('update_settings', { settings: { ...current, ...patch } });
        return { ...current, ...patch };
      }
      const r = await webRequest<{ data: AppSettings; _version: number }>('PATCH', '/settings', patch);
      lastSettingsVersion = r._version;
      return r.data;
    },
  },

  proxy: {
    getStatus: () =>
      useTauri()
        ? tauriCmd<ProxyStatus>('get_proxy_status')
        : webRequest<ProxyStatus>('GET', '/proxy/status'),

    start: () =>
      useTauri()
        ? tauriCmd<ProxyStatus>('start_proxy')
        : webRequest<ProxyStatus>('POST', '/proxy/start'),

    stop: () =>
      useTauri()
        ? tauriCmd<void>('stop_proxy')
        : webRequest<void>('POST', '/proxy/stop'),
  },

  testChat: (entryId, messages) =>
    useTauri()
      ? tauriCmd<TestChatResponse>('test_chat', { entryId, messages })
      : webRequest<TestChatResponse>('POST', '/test-chat', { entry_id: entryId, messages }),

  dirty: {
    /** 脏标记轮询，模块取值 'log' | 'pool' | 'channel' | 'token' */
    take: async (module: 'log' | 'pool' | 'channel' | 'token') => {
      if (useTauri()) {
        return tauriCmd<boolean>('take_dirty', { params: { module } });
      }
      const { version } = await webRequest<{ version: number }>('GET', '/state-version');
      const lastVersion = lastDirtyStateVersions[module];
      if (lastVersion === undefined) {
        lastDirtyStateVersions[module] = version;
        return false;
      }
      if (version === lastVersion) return false;
      lastDirtyStateVersions[module] = version;
      return true;
    },
  },

  translation: {
    getLatest: () => {
      if (useTauri()) {
        return tauriCmd<TranslationRelayPayload | null>('get_translation_relay');
      }
      return webRequest<TranslationRelayResponse>('GET', '/translation-relay').then((r) => r.latest);
    },

    translateAndRelay: (request) =>
      useTauri()
        ? tauriCmd<TranslationRelayPayload>('translate_and_relay', { request })
        : (() => { throw new Error('Web Admin does not support triggering translation in v1'); })(),
  },

  // In Combined mode the admin server is merged into the proxy port,
  // so /admin/version works for both desktop and web.
  getVersion: () => webRequest<{ version: string }>('GET', '/version'),

  getAdminStatus: () => {
    if (useTauri()) {
      return tauriCmd<AdminStatus>('get_admin_status');
    }
    return webRequest<AdminStatus>('GET', '/status').catch(() => ({
      running: false,
      address: '',
      port: 0,
    }));
  },
  getStateVersion: () =>
    webRequest<{ version: number }>('GET', '/state-version'),
};

// Type import for translation relay response shape used only in the web path
import type { TranslationRelayResponse } from '../types';

