import { api } from './core';

export type OriginConfig = {
  enabled: boolean;
  origins: string[];
};
export type OriginsView = { config: OriginConfig };

export const fetchOrigins = (slug: string) =>
  api<OriginsView>(`/api/v1/projects/${slug}/origins`);
export const saveOrigins = (slug: string, body: OriginConfig) =>
  api<OriginsView>(`/api/v1/projects/${slug}/origins`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
