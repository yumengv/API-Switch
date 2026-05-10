// ============================================================
// API Switch - Type Definitions
// ============================================================

// --- Sort Mode ---

export type ModelSortMode = "latest" | "fastest" | "custom";

// --- Channel ---

export interface Channel {
  id: string;
  name: string;
  api_type: ApiType;
  base_url: string;
  api_key: string;
  available_models: ModelInfo[];
  selected_models: string[];
  enabled: boolean;
  last_fetch_at: number;
  notes: string;
  response_ms: string;
  created_at: number;
  updated_at: number;
}

export type ApiType = "openai" | "claude" | "gemini" | "azure" | "custom" | "responses";

export const API_TYPE_OPTIONS: { value: ApiType; label: string }[] = [
  { value: "custom", label: "Custom (OpenAI-compatible)" },
  { value: "openai", label: "OpenAI" },
  { value: "claude", label: "Anthropic" },
  { value: "gemini", label: "Google Gemini" },
  { value: "azure", label: "Azure OpenAI" },
  { value: "responses", label: "OpenAI Responses (Beta)" },
];

export const API_TYPE_DEFAULT_URLS: Record<ApiType, string> = {
  openai: "https://api.openai.com",
  claude: "https://api.anthropic.com",
  gemini: "https://generativelanguage.googleapis.com",
  azure: "",
  custom: "",
  responses: "https://api.openai.com",
};

export interface ModelInfo {
  id: string;
  name: string;
  owned_by?: string;
}

export interface CreateChannelParams {
  name: string;
  api_type: ApiType;
  base_url: string;
  api_key: string;
  notes?: string;
}

export interface UpdateChannelParams {
  id: string;
  name?: string;
  api_type?: ApiType;
  base_url?: string;
  api_key?: string;
  enabled?: boolean;
  notes?: string;
}

// --- API Entry ---

export interface ApiEntry {
  id: string;
  channel_id: string;
  model: string;
  display_name: string;
  group_name: string;
  sort_index: number;
  enabled: boolean;
  cooldown_until?: number | null;
  circuit_state: CircuitState;
  created_at: number;
  updated_at: number;
  // Joined from channel
  channel_name?: string;
  channel_api_type?: ApiType;
  // Model provider (e.g. "openai", "anthropic", "google")
  owned_by?: string;
  // Response time from speed test (milliseconds string, or "X")
  response_ms?: string | null;
  provider_logo?: string | null;
  release_date?: string | null;
  model_meta_zh?: string | null;
  model_meta_en?: string | null;
}

export interface CreateEntryParams {
  channel_id: string;
  model: string;
  display_name?: string;
  provider_logo?: string;
  release_date?: string;
  model_meta_zh?: string;
  model_meta_en?: string;
  group_name?: string;
}

export type CircuitState = "closed" | "open" | "half_open";

// --- Access Key ---

export interface AccessKey {
  id: string;
  name: string;
  key: string;
  enabled: boolean;
  created_at: number;
}

// --- Usage Log ---

export interface UsageLog {
  id: number;
  type: number;
  content: string;
  access_key_id: string | null;
  access_key_name: string;
  token_name: string;
  api_entry_id: string;
  channel_id: string;
  channel_name: string;
  model: string;
  requested_model: string;
  quota: number;
  is_stream: boolean;
  prompt_tokens: number;
  completion_tokens: number;
  latency_ms: number;
  first_token_ms: number;
  use_time: number;
  status_code: number;
  success: boolean;
  request_id: string;
  log_group: string;
  other: string;
  error_message: string | null;
  ip: string | null;
  created_at: number;
}

export interface UsageLogFilter {
  start_time?: number;
  end_time?: number;
  model?: string;
  request_id?: string;
  channel_id?: string;
  access_key_id?: string;
  success?: boolean;
  page?: number;
  page_size?: number;
}

export interface PaginatedResult<T> {
  items: T[];
  total: number;
  page: number;
  page_size: number;
}

// --- Dashboard Stats ---

export interface DashboardStats {
  total_requests: number;
  today_requests: number;
  total_prompt_tokens: number;
  total_completion_tokens: number;
  today_prompt_tokens: number;
  today_completion_tokens: number;
  rpm: number;
  tpm: number;
  success_rate: number;
  avg_latency_ms: number;
}

export interface ChartDataPoint {
  time: string;
  model: string;
  value: number;
}

export interface ModelRanking {
  model: string;
  count: number;
  prompt_tokens: number;
  completion_tokens: number;
}

export interface UserRanking {
  access_key_name: string;
  count: number;
  prompt_tokens: number;
  completion_tokens: number;
}

export interface DashboardFilter {
  start_time?: number;
  end_time?: number;
  granularity?: "hour" | "day" | "week";
}

// --- Config ---

export interface AppSettings {
  proxy_enabled: boolean;
  listen_port: number;
  access_key_required: boolean;
  circuit_failure_threshold: number;
  proxy_connect_timeout_secs: number;
  circuit_recovery_secs: number;
  circuit_disable_codes: string;
  circuit_retry_codes: string;
  disable_keywords: string;
  locale: string;
  theme: "light" | "dark" | "system";
  autostart: boolean;
  start_minimized: boolean;
  show_guide: boolean;
  default_sort_mode: ModelSortMode;
  // Remembers the default/restored group for the API Management UI only.
  active_group: string;
  show_conversation_model: boolean;
  web_admin_enabled: boolean;
  web_admin_username: string;
  web_admin_password: string;
  web_admin_port: number;
}

export interface VersionedAppSettings extends AppSettings {
  _version: number;
}

export const DEFAULT_SETTINGS: VersionedAppSettings = {
  proxy_enabled: false,
  listen_port: 9090,
  access_key_required: false,
  circuit_failure_threshold: 3,
  proxy_connect_timeout_secs: 30,
  circuit_recovery_secs: 600,
  circuit_disable_codes: "401,403,410",
  circuit_retry_codes: "100-199,300-399,401-407,409-499,500-503,505-523,525-599",
  disable_keywords: "Your credit balance is too low\nThis organization has been disabled.\nYou exceeded your current quota\nPermission denied\nThe security token included in the request is invalid\nOperation not allowed\nYour account is not authorized",
  locale: "zh",
  theme: "light",
  autostart: false,
  start_minimized: false,
  show_guide: true,
  default_sort_mode: "custom",
  active_group: "auto",
  show_conversation_model: true,
  web_admin_enabled: false,
  web_admin_username: "admin",
  web_admin_password: "admin",
  web_admin_port: 9099,
  _version: 0,
};

// --- Proxy ---

export interface ProxyStatus {
  running: boolean;
  address: string;
  port: number;
}

export interface AdminStatus {
  running: boolean;
  address: string;
  port: number;
}

// --- Limit Query ---

export type LimitCredentialStatus = "valid" | "expired" | "not_found" | "parse_error";

export interface LimitTier {
  name: string;
  utilization: number;
  resetsAt: string | null;
}

export interface LimitQueryResult {
  provider: string;
  credentialStatus: LimitCredentialStatus;
  credentialMessage: string | null;
  success: boolean;
  tiers: LimitTier[];
  error: string | null;
  queriedAt: number | null;
  raw: unknown | null;
}

// --- Test Chat ---

export interface TestChatResponse {
  content: string;
  latency_ms: number;
  usage?: {
    prompt_tokens: number;
    completion_tokens: number;
    total_tokens: number;
  };
}

// --- Translation Relay ---

/**
 * Request payload for translation relay.
 */
export interface TranslationRelayRequest {
  /** Text to be translated */
  text: string;
  /** Optional source language code */
  sourceLang?: string;
  /** Optional target language code */
  targetLang?: string;
  /** Optional identifier of the model/entry used for translation */
  entryId?: string;
}

/**
 * Payload representing the latest translation result.
 */
export interface TranslationRelayPayload {
  /** Original source text */
  sourceText: string;
  /** Translated text */
  translatedText: string;
  /** Source language, if known */
  sourceLang?: string;
  /** Target language, if known */
  targetLang?: string;
  /** Whether translation succeeded */
  success: boolean;
  /** Error message if failed */
  error?: string | null;
  /** Timestamp of the update (ms since epoch) */
  updatedAt: number;
}

/**
 * HTTP response shape for translation relay endpoint.
 */
export interface TranslationRelayResponse {
  /** The latest payload, or null if none */
  latest: TranslationRelayPayload | null;
}
