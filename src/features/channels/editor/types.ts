// Channel Editor 缁勪欢鍐呴儴绫诲瀷鍜屽父閲忓畾涔?
import type { Channel, ModelCatalogMetaUpdate } from '../types';

/** 琛ㄥ崟鐘舵€?*/
export interface ChannelFormState {
  id?: string;
  name: string;
  api_type: string;
  base_url: string;
  api_key: string;
  notes: string;
  enabled: boolean;
}

/** URL Probe 鎺㈡祴缁撴灉 */
export interface UrlProbeResult {
  reachable: boolean;
  latency_ms: number;
  status_code?: number;
  detected_type?: string;
  corrected_base_url?: string;
  message: string;
}

/** 榛樿琛ㄥ崟鍊?*/
export const DEFAULT_FORM: ChannelFormState = {
  name: '',
    api_type: 'openai',
  base_url: '',
  api_key: '',
  notes: '',
  enabled: true,
};

/** API 绫诲瀷鍒楄〃 */
export const API_TYPES = [
  { value: 'openai', label: 'OpenAI-compatible' },
  { value: 'responses', label: 'OpenAI-Responses' },
  { value: 'anthropic', label: 'Anthropic/Claude' },
  { value: 'gemini', label: 'Google-Gemini' },
  { value: 'azure', label: 'Microsoft-Azure' },
  
] as const;

export function channelToForm(channel: Channel): ChannelFormState {
  return {
    id: channel.id,
    name: channel.name,
    
    base_url: channel.base_url,
    api_key: channel.api_key,
    notes: channel.notes ?? '',
    api_type: channel.api_type === 'custom' ? 'openai' : (channel.api_type === 'claude' ? 'anthropic' : channel.api_type),
    enabled: channel.enabled,
  };
}


