import { api } from './core';

export type TelegramIntegration = {
  configured: boolean;
  enabled: boolean;
  bot_token_masked: string;
  default_chat_id: string;
  updated_at: string | null;
  updated_by: string;
};

export type TelegramInput = {
  /** Token nuevo. Deja vacío para conservar el actual. */
  bot_token?: string;
  default_chat_id?: string;
  enabled?: boolean;
};

export const fetchTelegramIntegration = () =>
  api<TelegramIntegration>(`/api/v1/integrations/telegram`);
export const saveTelegramIntegration = (body: TelegramInput) =>
  api<TelegramIntegration>(`/api/v1/integrations/telegram`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
export const deleteTelegramIntegration = () =>
  api<TelegramIntegration>(`/api/v1/integrations/telegram`, { method: 'DELETE' });
export const testTelegramIntegration = (chat_id: string, text?: string) =>
  api<{ ok: boolean }>(`/api/v1/integrations/telegram/test`, {
    method: 'POST',
    body: JSON.stringify({ chat_id, text: text ?? '' })
  });
