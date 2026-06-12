import { qs, type RangeArgs, api } from './core';
import type { ProductEvent } from './productEvents';

export type ProductUserSummary = {
  project_id: string;
  distinct_id: string;
  first_seen: string;
  last_seen: string;
  anonymous_ids: string[];
  sources: string[];
  event_count: number;
  /** JSON serializado con las últimas user properties conocidas. */
  properties: string;
};

export type ProductUserDeviceBreakdown = {
  source: string;
  event_count: number;
  last_seen: string;
  anonymous_id_count: number;
};

export type ProductUserDetail = {
  project_id: string;
  distinct_id: string;
  first_seen: string;
  last_seen: string;
  anonymous_ids: string[];
  sources: string[];
  event_count: number;
  properties: string;
  devices: ProductUserDeviceBreakdown[];
};

export type ProductUserFilters = RangeArgs & {
  query?: string;
  source?: string | string[];
};

function productUsersQs(params: ProductUserFilters): string {
  const u = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (key === 'source') continue;
    if (value === undefined || value === null || value === '') continue;
    u.set(key, String(value));
  }
  const sources = Array.isArray(params.source) ? params.source : [params.source];
  for (const source of sources) {
    if (source) u.append('source', source);
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export const fetchProductUsers = (params: ProductUserFilters = {}) =>
  api<ProductUserSummary[]>(`/api/v1/product-users${productUsersQs(params)}`);

export const fetchProductUser = (distinctId: string, params: RangeArgs = {}) =>
  api<ProductUserDetail>(`/api/v1/product-users/${encodeURIComponent(distinctId)}${qs(params)}`);

export const fetchProductUserEvents = (
  distinctId: string,
  params: RangeArgs & { source?: string } = {}
) => api<ProductEvent[]>(
  `/api/v1/product-users/${encodeURIComponent(distinctId)}/events${qs(params)}`
);
