import { api } from './core';

export type RedactionCustomRule = {
  name: string;
  pattern: string;
  replacement: string;
};
export type RedactionConfig = {
  enabled: boolean;
  /** Slugs de built-ins activados (e.g. 'email', 'jwt'). */
  builtins: string[];
  custom: RedactionCustomRule[];
};
export type RedactionBuiltinInfo = {
  slug: string;
  label: string;
  description: string;
};
export type RedactionView = {
  config: RedactionConfig;
  available_builtins: RedactionBuiltinInfo[];
};

export const fetchRedaction = (slug: string) =>
  api<RedactionView>(`/api/v1/projects/${slug}/redaction`);
export const saveRedaction = (slug: string, body: RedactionConfig) =>
  api<RedactionView>(`/api/v1/projects/${slug}/redaction`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
