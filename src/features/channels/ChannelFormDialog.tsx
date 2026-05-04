import React, { useState } from 'react';
import { useApiAdapter } from '../../lib/useApiAdapter';
import { getChannelErrorMessage } from './channelErrors';
import type { CreateChannelParams, UpdateChannelParams, Channel } from './types';

interface Props {
  channel?: Channel;
  onClose: () => void;
  onSaved: () => void;
}

export const ChannelFormDialog: React.FC<Props> = ({ channel, onClose, onSaved }) => {
  const api = useApiAdapter();
  const [form, setForm] = useState<CreateChannelParams | UpdateChannelParams>(
    channel
      ? { id: channel.id, name: channel.name, api_type: channel.api_type, base_url: channel.base_url, api_key: channel.api_key, enabled: channel.enabled, notes: channel.notes }
      : { name: '', api_type: '', base_url: '', api_key: '' }
  );
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const formValues = form as Partial<CreateChannelParams & UpdateChannelParams>;

  const handleChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target;
    setForm((current) => ({ ...current, [name]: value }));
  };
  const handleSubmit = async () => {
    setSaving(true);
    setError(null);
    try {
      if (channel) {
        await api.channels.update(form as UpdateChannelParams);
      } else {
        await api.channels.create(form as CreateChannelParams);
      }
      onSaved();
    } catch (err) {
      setError(getChannelErrorMessage(err, 'Failed to save channel'));
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="dialog">
      <h3>{channel ? 'Edit' : 'Create'} Channel</h3>
      {error && <div style={{ color: '#b91c1c', marginBottom: 8 }}>{error}</div>}
      <input name="name" placeholder="Name" value={formValues.name || ''} onChange={handleChange} />
      <input name="api_type" placeholder="API Type" value={formValues.api_type || ''} onChange={handleChange} />
      <input name="base_url" placeholder="Base URL" value={formValues.base_url || ''} onChange={handleChange} />
      <input name="api_key" placeholder="API Key" value={formValues.api_key || ''} onChange={handleChange} />
      <button onClick={handleSubmit} disabled={saving}>{saving ? 'Saving...' : 'Save'}</button>
      <button onClick={onClose} disabled={saving}>Cancel</button>
    </div>
  );
};
