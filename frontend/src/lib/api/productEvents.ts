import { qs, type RangeArgs, api } from './core';

export type ProductEvent = {
  timestamp: string;
  project_id: string;
  event_name: string;
  distinct_id: string;
  anonymous_id: string;
  session_id: string;
  /** JSON serializado. Parsear con JSON.parse cuando se necesita renderizar. */
  properties: string;
  user_properties: string;
  context: string;
  source: string;
  trace_id: string;
  span_id: string;
  event_id: string;
};

export type EventBucket = {
  ts: string;
  event_name: string;
  count: number;
};

export type EventFilters = RangeArgs & {
  event_name?: string;
  distinct_id?: string;
  anonymous_id?: string;
  session_id?: string;
  trace_id?: string;
  source?: string;
  query?: string;
  /** Pares `key:value` para filtrar properties. Cada entrada se envía como un
   *  query param `prop=<key>:<value>` separado. */
  prop?: string[];
};

function eventsQs(params: EventFilters): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (k === 'prop') continue;
    if (v === undefined || v === null || v === '') continue;
    u.set(k, String(v));
  }
  for (const p of params.prop ?? []) {
    if (p && p.includes(':')) u.append('prop', p);
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export const fetchEvents = (params: EventFilters = {}) =>
  api<ProductEvent[]>(`/api/v1/events${eventsQs(params)}`);

export const fetchEventStats = (
  params: RangeArgs & { event_name?: string; bucket_seconds?: number } = {}
) => api<EventBucket[]>(`/api/v1/events/stats${qs(params)}`);
