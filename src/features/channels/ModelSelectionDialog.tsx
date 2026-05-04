import React, { useState } from 'react';
import { useApiAdapter } from '../../lib/useApiAdapter';
import { getChannelErrorMessage, useChannelModelText } from './channelErrors';
import type { Channel, ModelInfo } from './types';

interface Props {
  channel: Channel;
  onClose: () => void;
  onSaved: () => void;
}

export const ModelSelectionDialog: React.FC<Props> = ({ channel, onClose, onSaved }) => {
  const api = useApiAdapter();
  const initialNames = useChannelModelText(channel);
  const [modelNames, setModelNames] = useState<string>(initialNames);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      const names = modelNames.split(',').map((s) => s.trim()).filter(Boolean);
      const availableModels: ModelInfo[] = Array.isArray(channel.available_models) ? channel.available_models : [];
      await api.channels.selectModels(channel.id, names, availableModels, []);
      onSaved();
    } catch (err) {
      setError(getChannelErrorMessage(err, 'Failed to select models'));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="dialog">
      <h3>Select Models for {channel.name}</h3>
      {error && <div style={{ color: '#b91c1c', marginBottom: 8 }}>{error}</div>}
      <textarea value={modelNames} onChange={(e) => setModelNames(e.target.value)} placeholder="model1, model2" />
      <button onClick={handleSave} disabled={saving}>{saving ? 'Saving...' : 'Save'}</button>
      <button onClick={onClose} disabled={saving}>Cancel</button>
    </div>
  );
};
