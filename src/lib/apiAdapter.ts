export interface ApiAdapter {
  channels: {
    list(): Promise<Channel[]>;
    listPaginated(params: { page: number; pageSize: number }): Promise<PaginatedResult<Channel>>;
    create(params: CreateChannelParams): Promise<Channel>;
    update(params: UpdateChannelParams): Promise<Channel>;
    delete(id: string): Promise<void>;
    fetchModels(channelId: string): Promise<FetchModelsResult>;
    fetchModelsDirect(apiType: string, baseUrl: string, apiKey: string, verified?: boolean): Promise<FetchModelsResult>;
    probeUrl(url: string, apiType?: string, apiKey?: string): Promise<ProbeResult>;
    testChannel(channelId: string): Promise<TestChannelResult>;
    testChannelDirect(params: TestChannelDirectParams): Promise<TestChannelResult>;
    selectModels(channelId: string, modelNames: string[], availableModels: ModelInfo[], catalogMeta?: ModelCatalogMetaUpdate[]): Promise<void>;
        updateResponseMs(channelId: string, responseMs: string): Promise<void>;
    saveChannelWithModels(params: SaveChannelWithModelsParams): Promise<SaveChannelWithModelsResult>;
  };
  usage: {
    getLogs(filter: UsageLogFilter): Promise<PaginatedResult<UsageLog>>;
    getDashboardStats(filter?: DashboardFilter): Promise<DashboardStats>;
    getModelConsumption(filter?: DashboardFilter): Promise<ChartDataPoint[]>;
    getCallTrend(filter?: DashboardFilter): Promise<ChartDataPoint[]>;
    getModelDistribution(filter?: DashboardFilter): Promise<ModelRanking[]>;
    getUserTrend(filter?: DashboardFilter): Promise<ChartDataPoint[]>;
    clearLogDetails(): Promise<number>;
  };
  pool: {
    list(): Promise<ApiEntry[]>;
    listPaginated(params: { page: number; pageSize: number; groupName?: string; search?: string; channelId?: string }): Promise<PaginatedResult<ApiEntry>>;
    toggle(id: string, enabled: boolean, options?: { pinToTop?: boolean }): Promise<void>;
    batchToggle(ids: string[], enabled: boolean): Promise<void>;
    reorder(orderedIds: string[]): Promise<void>;
    updateSortIndex(id: string, sortIndex: number): Promise<void>;
    updateSortIndexes(items: { id: string; sortIndex: number }[]): Promise<void>;
    create(params: { channelId: string; model: string; displayName?: string; groupName?: string }): Promise<ApiEntry>;
    delete(id: string): Promise<void>;
    testLatency(id: string, modelScore?: number): Promise<{ entry_id: string; latency_ms: number | null; score: number; error_detail?: string }>;
    backfillCatalogMeta(items: { entryId: string; catalogProvider: string; catalogModelId: string }[]): Promise<void>;
    getGroups(): Promise<string[]>;
    listModelGroups(): Promise<ModelGroupConfig[]>;
    listModelGroupEntryIds(name: string): Promise<string[]>;
    upsertModelGroup(params: { name: string; description?: string; enabled?: boolean; priority?: number }): Promise<ModelGroupConfig>;
    updateModelGroupEnabled(name: string, enabled: boolean): Promise<void>;
    deleteModelGroup(name: string): Promise<void>;
    replaceModelGroupEntries(name: string, entryIds: string[]): Promise<void>;
    updateDisplayName(id: string, displayName: string): Promise<void>;
    updateGroup(id: string, groupName: string): Promise<void>;
  };
  tokens: {
    list(): Promise<AccessKey[]>;
    listPaginated(params: { page: number; pageSize: number }): Promise<PaginatedResult<AccessKey>>;
    create(name: string): Promise<AccessKey>;
    delete(id: string): Promise<void>;
    toggle(id: string, enabled: boolean): Promise<void>;
  };
  connectionApps: {
    list(): Promise<ConnectionAppItem[]>;
    execute(id: string): Promise<AppConfigResult>;
  };
  importExport: {
    exportChannelModel(): Promise<string>;
    previewChannelModel(payload: string): Promise<ChannelModelImportPreview>;
    importChannelModel(payload: string): Promise<ChannelModelImportResult>;
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
  getPlatformCapabilities(): Promise<PlatformCapabilities>;
  getStateVersion(): Promise<{ log: number; pool: number; channel: number; token: number }>;
  dirty: {
    /**
     * 杞鑴忔爣璁帮紝妯″潡鍙栧€? 'log' | 'pool' | 'channel' | 'token'
     * 杩斿洖 true 琛ㄧず瀵瑰簲妯″潡鏈夊彉鍔紝闇€瑕佸埛鏂版煡璇?
     */
    take(module: 'log' | 'pool' | 'channel' | 'token'): Promise<boolean>;
  };
}



import type { Channel, CreateChannelParams, UpdateChannelParams, FetchModelsResult, ProbeResult, TestChannelResult, TestChannelDirectParams, ModelInfo, ModelCatalogMetaUpdate, SaveChannelWithModelsParams, SaveChannelWithModelsResult } from '../features/channels/types';
import type { DashboardFilter, DashboardStats, ChartDataPoint, ModelRanking, UsageLog, UsageLogFilter, PaginatedResult, ApiEntry, AccessKey, AppSettings, ProxyStatus, AdminStatus, PlatformCapabilities, TestChatResponse, TranslationRelayPayload, TranslationRelayRequest, ConnectionAppItem, AppConfigResult, ChannelModelImportPreview, ChannelModelImportResult, ModelGroupConfig } from '../types';



