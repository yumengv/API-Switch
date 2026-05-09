import type { ApiAdapter } from './apiAdapter';
import type {
  Channel,
  ChannelOperationError,
  ChannelOperationErrorKind,
  ChannelOperationHttpError,
  FetchModelsResult,
  ProbeResult,
  ModelInfo,
  ModelCatalogMetaUpdate,
} from '../features/channels/types';
import type { DashboardFilter, DashboardStats, ChartDataPoint, ModelRanking, UsageLog, UsageLogFilter, PaginatedResult, ApiEntry, AccessKey, AppSettings, VersionedAppSettings, ProxyStatus, TestChatResponse, TranslationRelayPayload, TranslationRelayRequest, TranslationRelayResponse } from '../types';

import { ADMIN_API_PREFIX } from './adminApiConfig';
import { clearToken, emitAuthExpired, TOKEN_KEY } from './webAuth';

const apiBase = ADMIN_API_PREFIX;

// Track settings version for optimistic concurrency control
let lastSettingsVersion = 0;

interface ErrorEnvelope {
  error?: ChannelOperationError;
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
      if (status === 401 || status === 403) {
        return 'auth';
      }
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

async function request<T>(
    method: 'GET' | 'POST' | 'PUT' | 'DELETE' | 'PATCH',
    path: string,
    data?: unknown,
    queryParams?: Record<string, unknown> | null
): Promise<T> {
  const token = localStorage.getItem(TOKEN_KEY);

  // Build URL with query params for GET requests
  let url = `${apiBase}${path}`;
  if (queryParams) {
    const searchParams = new URLSearchParams();
    Object.entries(queryParams as Record<string, unknown>).forEach(([key, value]) => {
      if (value !== undefined && value !== null) {
        searchParams.append(key, String(value));
      }
    });
    const queryString = searchParams.toString();
    if (queryString) {
      url += `?${queryString}`;
    }
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
      const body = (await response.json()) as ErrorEnvelope;
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

  if (response.status === 204) {
    return undefined as T;
  }

  return response.json() as Promise<T>;
}

export const webAdminApiAdapter: ApiAdapter = {
  channels: {
    list: () => request<Channel[]>('GET', '/channels'),
    create: (params) => request<Channel>('POST', '/channels', params),
    update: (params) => request<Channel>('PUT', `/channels/${params.id}`, params),
    delete: (id) => request<void>('DELETE', `/channels/${id}`),
    fetchModels: (channelId) => request<FetchModelsResult>('POST', `/channels/${channelId}/fetch-models`),
    fetchModelsDirect: (apiType, baseUrl, apiKey, verified) =>
      request<FetchModelsResult>('POST', '/channels/fetch-models-direct', { apiType, baseUrl, apiKey, verified }),
    probeUrl: (url) => request<ProbeResult>('POST', '/channels/probe-url', { url }),
    selectModels: (channelId, modelNames, availableModels, catalogMeta = []) => request<void>('POST', `/channels/${channelId}/select-models`, {
      modelNames,
      availableModels,
      catalogMeta,
    }),
    updateResponseMs: (channelId, responseMs) => request<void>('PUT', `/channels/${channelId}/response-ms`, { channelId, responseMs }),
  },
  usage: {
    getLogs: (filter) => request<PaginatedResult<UsageLog>>('GET', '/logs', undefined, filter as Record<string, unknown>),
    getDashboardStats: (filter) => request<DashboardStats>('GET', '/dashboard/stats', undefined, filter as Record<string, unknown>),
    getModelConsumption: (filter) => request<ChartDataPoint[]>('GET', '/dashboard/model-consumption', undefined, filter as Record<string, unknown>),
    getCallTrend: (filter) => request<ChartDataPoint[]>('GET', '/dashboard/call-trend', undefined, filter as Record<string, unknown>),
    getModelDistribution: (filter) => request<ModelRanking[]>('GET', '/dashboard/model-distribution', undefined, filter as Record<string, unknown>),
    getUserTrend: (filter) => request<ChartDataPoint[]>('GET', '/dashboard/user-trend', undefined, filter as Record<string, unknown>),
  },
  pool: {
    list: () => request<ApiEntry[]>('GET', '/pool'),
    toggle: (id, enabled) => request<void>('PUT', `/pool/${id}/toggle`, enabled),
    reorder: (orderedIds) => request<void>('POST', '/pool/reorder', { ordered_ids: orderedIds }),
    create: (params) => request<ApiEntry>('POST', '/pool', {
      channel_id: params.channelId,
      model: params.model,
      display_name: params.displayName,
      group_name: params.groupName,
    }),
    delete: (id) => request<void>('DELETE', `/pool/${id}`),
    testLatency: (id) => request<{ entry_id: string; latency_ms: number | null }>('POST', `/pool/${id}/test-latency`),
    backfillCatalogMeta: (items) => request<void>('POST', '/pool/backfill-catalog-meta', {
      items: items.map(item => ({
        id: item.entryId,
        catalog_provider: item.catalogProvider,
        catalog_model_id: item.catalogModelId,
      })),
    }),
    getGroups: () => request<string[]>('GET', '/pool/groups'),
    updateGroup: (id, groupName) => request<void>('PUT', `/pool/${id}/group`, groupName),
  },
  tokens: {
    list: () => request<AccessKey[]>('GET', '/tokens'),
    create: (name) => request<AccessKey>('POST', '/tokens', { name }),
    delete: (id) => request<void>('DELETE', `/tokens/${id}`),
    toggle: (id, enabled) => request<void>('PUT', `/tokens/${id}/toggle`, enabled),
  },
settings: {
    get: () => request<{ data: AppSettings; _version: number }>('GET', '/settings').then(r => {
        // Track version for subsequent updates
        lastSettingsVersion = r._version;
        return r.data;
    }),
    update: (settings) => {
        return request<void>('PUT', '/settings', { data: settings, _version: lastSettingsVersion });
    },
    patchSettings: (patch) => {
        return request<{ data: AppSettings; _version: number }>('PATCH', '/settings', patch).then(r => {
            lastSettingsVersion = r._version;
            return r.data;
        });
    },
},
  proxy: {
    getStatus: () => request<ProxyStatus>('GET', '/proxy/status'),
    start: () => request<ProxyStatus>('POST', '/proxy/start'),
    stop: () => request<void>('POST', '/proxy/stop'),
  },
  // BUG FIX: was '/admin/version' which double-prefixed to '/admin/admin/version'
  // because request() already prepends apiBase. Use relative path like all other endpoints.
  getVersion: () => request<{ version: string }>('GET', '/version'),
  testChat: (entryId, messages) => request<TestChatResponse>('POST', '/test-chat', { entry_id: entryId, messages }),
  translation: {
    getLatest: async () => {
      const response = await request<TranslationRelayResponse>('GET', '/translation-relay');
      return response.latest;
    },
    translateAndRelay: () => {
      throw new Error('Web Admin does not support triggering translation in v1');
    },
  },
};
