import { qs, type RangeArgs, api } from './core';
import type { TraceSummary } from './traces';

export type ProductSessionSummary = {
  project_id: string;
  session_id: string;
  distinct_id: string;
  started_at: string;
  ended_at: string;
  duration_seconds: number;
  pageview_count: number;
  event_count: number;
  is_bounce: number;
  is_engaged: number;
  converted: number;
  quality_score: number;
  error_count: number;
  has_error: number;
  has_replay: number;
  replay_event_count: number;
  replay_chunk_count: number;
  trace_count: number;
  source: string;
};

export type ProductSessionFilters = RangeArgs & {
  session_id?: string;
  distinct_id?: string;
  has_replay?: boolean | number | string;
  has_error?: boolean | number | string;
};

export const fetchProductSessions = (params: ProductSessionFilters = {}) =>
  api<ProductSessionSummary[]>(`/api/v1/sessions${qs(params)}`);

export const fetchProductSessionTraces = (sessionId: string, project: string) =>
  api<TraceSummary[]>(
    `/api/v1/sessions/${encodeURIComponent(sessionId)}/traces${qs({ project })}`
  );
