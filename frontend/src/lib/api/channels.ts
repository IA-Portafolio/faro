import { api } from './core';

export type ChannelKind =
  | 'webhook'
  | 'slack'
  | 'discord'
  | 'pagerduty'
  | 'opsgenie'
  | 'email_resend'
  | 'telegram';

/** El backend devuelve `config` con los secretos enmascarados. Para editar,
 *  el frontend manda los campos secretos VACÍOS para conservar el actual. */
export type NotificationChannel = {
  id: string;
  name: string;
  kind: ChannelKind;
  enabled: boolean;
  config: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  updated_by: string;
};

export type ChannelInput = {
  /** Sólo en POST. Si vacío, el backend genera uno a partir del `name`. */
  id?: string;
  name: string;
  kind: ChannelKind;
  enabled: boolean;
  config: Record<string, unknown>;
};

export const fetchChannelKinds = () =>
  api<{ kinds: ChannelKind[] }>(`/api/v1/integrations/channels/kinds`);
export const listChannels = () =>
  api<NotificationChannel[]>(`/api/v1/integrations/channels`);
export const getChannel = (id: string) =>
  api<NotificationChannel>(`/api/v1/integrations/channels/${encodeURIComponent(id)}`);
export const createChannel = (body: ChannelInput) =>
  api<NotificationChannel>(`/api/v1/integrations/channels`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
export const updateChannel = (id: string, body: ChannelInput) =>
  api<NotificationChannel>(`/api/v1/integrations/channels/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
export const deleteChannel = (id: string) =>
  api<{ deleted: string }>(`/api/v1/integrations/channels/${encodeURIComponent(id)}`, {
    method: 'DELETE'
  });
export const testChannel = (id: string, note?: string) =>
  api<{ ok: boolean; kind: string }>(
    `/api/v1/integrations/channels/${encodeURIComponent(id)}/test`,
    {
      method: 'POST',
      body: JSON.stringify({ note: note ?? '' })
    }
  );
