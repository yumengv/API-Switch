export interface ApiAdapter {
  channels: {
    list(): Promise<Channel[]>;
    listPaginated(params: { page: number; pageSize: number }): Promise<PaginatedResult<Channel>>;
    create(params: CreateChannelParams): Promise<Channel>;
    update(params: UpdateChannelParams): Promise<Channel>;
    delete(id: string): Promise<void>;
    fetchModels(channelId: string): Promise<FetchModelsResult>;
    fetchModelsDirect(apiType: string, baseUrl: string, apiKey: string, verified?: boolean): Promise<FetchModelsResult>;
    probeUrl(url: string): Promise<ProbeResult>;
    testChannel(channelId: string): Promise<TestChannelResult>;
    selectModels(channelId: string, modelNames: string[], availableModels: ModelInfo[], catalogMeta?: ModelCatalogMetaUpdate[]): Promise<void>;
    updateResponseMs(channelId: string, responseMs: string): Promise<void>;
  };
  usage: {
    getLogs(filter: UsageLogFilter): Promise<PaginatedResult<UsageLog>>;
    getDashboardStats(filter?: DashboardFilter): Promise<DashboardStats>;
    getModelConsumption(filter?: DashboardFilter): Promise<ChartDataPoint[]>;
    getCallTrend(filter?: DashboardFilter): Promise<ChartDataPoint[]>;
    getModelDistribution(filter?: DashboardFilter): Promise<ModelRanking[]>;
    getUserTrend(filter?: DashboardFilter): Promise<ChartDataPoint[]>;
  };
  pool: {
    list(): Promise<ApiEntry[]>;
    listPaginated(params: { page: number; pageSize: number; groupName?: string; search?: string; channelId?: string }): Promise<PaginatedResult<ApiEntry>>;
    toggle(id: string, enabled: boolean): Promise<void>;
    batchToggle(ids: string[], enabled: boolean): Promise<void>;
    reorder(orderedIds: string[]): Promise<void>;
    create(params: { channelId: string; model: string; displayName?: string; groupName?: string }): Promise<ApiEntry>;
    delete(id: string): Promise<void>;
    testLatency(id: string): Promise<{ entry_id: string; latency_ms: number | null; error_detail?: string }>;
    backfillCatalogMeta(items: { entryId: string; catalogProvider: string; catalogModelId: string }[]): Promise<void>;
    getGroups(): Promise<string[]>;
    updateGroup(id: string, groupName: string): Promise<void>;
  };
  tokens: {
    list(): Promise<AccessKey[]>;
    listPaginated(params: { page: number; pageSize: number }): Promise<PaginatedResult<AccessKey>>;
    create(name: string): Promise<AccessKey>;
    delete(id: string): Promise<void>;
    toggle(id: string, enabled: boolean): Promise<void>;
  };
settings: {
    get(): Promise<AppSettings>;
    update(settings: AppSettings): Promise<void>;
    patchSettings(patch: Partial<AppSettings>): Promise<AppSettings>;
};
  proxy: {
    getStatus(): Promise<ProxyStatus>;
    start(): Promise<ProxyStatus>;
    stop(): Promise<void>;
  };
  testChat(entryId: string, messages: { role: string; content: string }[]): Promise<TestChatResponse>;
  translation: {
    getLatest(): Promise<TranslationRelayPayload | null>;
    translateAndRelay(request: TranslationRelayRequest): Promise<TranslationRelayPayload>;
  };
  getVersion(): Promise<{ version: string }>;
  getAdminStatus(): Promise<AdminStatus>;
  getStateVersion(): Promise<{ version: number }>;
  dirty: {
    /**
     * 轮询脏标记，模块取值: 'log' | 'pool' | 'channel' | 'token'
     * 返回 true 表示对应模块有变动，需要刷新查询
     */
    take(module: 'log' | 'pool' | 'channel' | 'token'): Promise<boolean>;
  };
}



import type { Channel, CreateChannelParams, UpdateChannelParams, FetchModelsResult, ProbeResult, TestChannelResult, ModelInfo, ModelCatalogMetaUpdate } from '../features/channels/types';
import type { DashboardFilter, DashboardStats, ChartDataPoint, ModelRanking, UsageLog, UsageLogFilter, PaginatedResult, ApiEntry, AccessKey, AppSettings, ProxyStatus, AdminStatus, TestChatResponse, TranslationRelayPayload, TranslationRelayRequest } from '../types';
