export interface Channel {
  id: string;
  name: string;
  api_type: string;
  base_url: string;
  api_key: string;
  available_models?: ModelInfo[];
  selected_models?: string[];
  enabled: boolean;
  notes?: string;
  response_ms?: string;
}
export interface CreateChannelParams {
  name: string;
  api_type: string;
  base_url: string;
  api_key: string;
  notes?: string;
}
export interface UpdateChannelParams {
  id: string;
  name?: string;
  api_type?: string;
  base_url?: string;
  api_key?: string;
  enabled?: boolean;
  notes?: string;
}
export interface ModelInfo {
  id: string;
  name: string;
  owned_by?: string;
}

export interface ModelCatalogMetaUpdate {
  model: string;
  provider_logo: string;
  release_date: string;
  model_meta_zh: string;
  model_meta_en: string;
}

export interface ChannelOperationError {
  code: string;
  message: string;
  details?: Record<string, unknown>;
}

export type ChannelOperationErrorKind =
  | 'network'
  | 'auth'
  | 'timeout'
  | 'rate_limited'
  | 'invalid_url'
  | 'unsupported_provider'
  | 'empty_model_list'
  | 'endpoint_correction_failed'
  | 'unknown';

export interface ChannelOperationHttpError extends Error {
  status?: number;
  error?: ChannelOperationError;
  kind: ChannelOperationErrorKind;
  isNetworkError: boolean;
  isAuthError: boolean;
  isTimeoutError: boolean;
}

export interface FetchModelsResult {
  detected_type: string;
  corrected_base_url: string;
  models: Array<Record<string, unknown>>;
  message: string;
  warning?: string;
  error?: ChannelOperationError;
  endpoint_corrected: boolean;
  auto_saved: boolean;
}
export interface ProbeResult {
  reachable: boolean;
  status_code?: number;
  latency_ms: number;
  detected_type?: string;
  message: string;
  warning?: string;
  error?: ChannelOperationError;
}

export interface TestChannelResult {
  success: boolean;
  latency_ms: number;
  status_code?: number;
  message: string;
}
