import { api, qs, type RangeArgs } from './core';

export type ReplaySummary = {
  session_id: string;
  service_name: string;
  start_ts: string;
  end_ts: string;
  event_count: number;
  chunk_count: number;
  user_id: string;
  page_url: string;
};

export type ReplayPayload = {
  session_id: string;
  service_name: string;
  start_ts: string;
  end_ts: string;
  event_count: number;
  page_url: string;
  user_id: string;
  user_agent: string;
  /** Array de eventos rrweb concatenados de todos los chunks, en orden. */
  events: unknown[];
};

export const fetchReplays = (r: RangeArgs & { service?: string; session_id?: string } = {}) =>
  api<ReplaySummary[]>(`/api/v1/replays${qs(r)}`);
export const fetchReplay = (sessionId: string) =>
  api<ReplayPayload>(`/api/v1/replays/${encodeURIComponent(sessionId)}`);
