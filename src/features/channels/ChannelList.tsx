import React, { useEffect, useState } from 'react';
import { useApiAdapter } from '../../lib/useApiAdapter';
import { getChannelErrorMessage } from './channelErrors';
import type { Channel } from './types';

export const ChannelList: React.FC<{ onSelect: (c: Channel) => void }> = ({ onSelect }) => {
  const api = useApiAdapter();
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    api.channels
      .list()
      .then((items) => {
        if (!cancelled) {
          setChannels(items);
        }
      })
      .catch((err) => {
        if (!cancelled) {
          setError(getChannelErrorMessage(err, 'Failed to load channels'));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [api]);

  if (loading) return <div>Loading channels...</div>;
  if (error) return <div style={{ color: '#b91c1c' }}>{error}</div>;
  if (channels.length === 0) return <div>No channels yet.</div>;

  return (
    <ul>
      {channels.map((c) => (
        <li key={c.id} onClick={() => onSelect(c)} style={{ cursor: 'pointer' }}>
          {c.name} ({c.api_type})
        </li>
      ))}
    </ul>
  );
};
