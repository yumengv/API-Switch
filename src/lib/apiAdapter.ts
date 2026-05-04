export interface ApiAdapter {
  channels: {
    list(): Promise<Channel[]>;
    create(params: CreateChannelParams): Promise<Channel>;
    update(params: UpdateChannelParams): Promise<Channel>;
    delete(id: string): Promise<void>;
    fetchModels(channelId: string): Promise<FetchModelsResult>;
    fetchModelsDirect(apiType: string, baseUrl: string, apiKey: string, verified?: boolean): Promise<FetchModelsResult>;
    probeUrl(url: string): Promise<ProbeResult>;
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
    toggle(id: string, enabled: boolean): Promise<void>;
    reorder(orderedIds: string[]): Promise<void>;
    create(params: { channelId: string; model: string; displayName?: string }): Promise<ApiEntry>;
    delete(id: string): Promise<void>;
    testLatency(id: string): Promise<{ entry_id: string; latency_ms: number | null }>;
    backfillCatalogMeta(items: { entryId: string; catalogProvider: string; catalogModelId: string }[]): Promise<void>;
  };
  tokens: {
    list(): Promise<AccessKey[]>;
    create(name: string): Promise<AccessKey>;
    delete(id: string): Promise<void>;
    toggle(id: string, enabled: boolean): Promise<void>;
  };
}

// Types referenced above – import from shared definitions
import type { Channel, CreateChannelParams, UpdateChannelParams, FetchModelsResult, ProbeResult, ModelInfo, ModelCatalogMetaUpdate } from '../features/channels/types';
import type { DashboardFilter, DashboardStats, ChartDataPoint, ModelRanking, UsageLog, UsageLogFilter, PaginatedResult, ApiEntry, AccessKey } from '../types';
