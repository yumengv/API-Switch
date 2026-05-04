import { useState, useCallback } from 'react';
import type { Channel } from './types';

export function useChannelSelection() {
  const [selected, setSelected] = useState<Channel | null>(null);
  const select = useCallback((c: Channel) => setSelected(c), []);
  const clear = useCallback(() => setSelected(null), []);
  return { selected, select, clear };
}
