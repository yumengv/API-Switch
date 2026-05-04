import { useMemo } from 'react';
import type { ChannelOperationHttpError } from './types';

export function getChannelErrorMessage(error: unknown, fallback: string): string {
  if (!error || !(error instanceof Error)) {
    return fallback;
  }

  const operationError = error as ChannelOperationHttpError;
  const message = operationError.error?.message || operationError.message || fallback;

  switch (operationError.kind) {
    case 'auth':
      return `认证失败：${message}`;
    case 'timeout':
      return `请求超时：${message}`;
    case 'network':
      return `网络不可达：${message}`;
    case 'rate_limited':
      return `请求过于频繁：${message}`;
    case 'invalid_url':
      return `地址无效：${message}`;
    case 'unsupported_provider':
      return `接口不兼容：${message}`;
    case 'empty_model_list':
      return `模型列表为空：${message}`;
    case 'endpoint_correction_failed':
      return `端点修正失败：${message}`;
    default:
      return message;
  }
}

export function useChannelModelText(channel: { selected_models?: string[] } | null | undefined) {
  return useMemo(() => channel?.selected_models?.join(', ') ?? '', [channel?.selected_models]);
}
