import type { RangePreset } from './stores';

export type SessionHealth = 'error' | 'replay' | 'plain';

export type SessionLikeRow = {
  project_id: string;
  session_id: string;
  distinct_id: string;
  started_at: string;
  ended_at: string;
  duration_seconds: number;
  pageview_count: number;
  event_count: number;
  error_count: number;
  has_error: number;
  has_replay: number;
  replay_event_count: number;
  replay_chunk_count: number;
  source: string;
};

export function formatSessionDuration(seconds: number): string {
  const total = Math.max(0, Math.floor(seconds || 0));
  if (total < 60) return `${total}s`;
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  if (h > 0) return `${h}h ${m}m`;
  return `${m}m ${s}s`;
}

export function sessionReplayHref(row: Pick<SessionLikeRow, 'session_id' | 'has_replay'>): string {
  if (!row.session_id || row.has_replay !== 1) return '';
  return `/replays/${encodeURIComponent(row.session_id)}`;
}

export function sessionEventsHref(
  row: Pick<SessionLikeRow, 'session_id'>,
  project?: string,
  range?: RangePreset
): string {
  const params = new URLSearchParams();
  params.set('query', `session_id:${row.session_id}`);
  if (project) params.set('project', project);
  if (range) params.set('range', range);
  return `/events?${params.toString()}`;
}

export function sessionUserHref(
  row: Pick<SessionLikeRow, 'distinct_id'>,
  project?: string,
  range?: RangePreset
): string {
  if (!row.distinct_id) return '';
  const params = new URLSearchParams();
  if (project) params.set('project', project);
  if (range) params.set('range', range);
  const qs = params.toString();
  return `/users/${encodeURIComponent(row.distinct_id)}${qs ? `?${qs}` : ''}`;
}

export function sessionHealth(row: Pick<SessionLikeRow, 'error_count' | 'has_error' | 'has_replay'>): SessionHealth {
  if (row.has_error === 1 || row.error_count > 0) return 'error';
  if (row.has_replay === 1) return 'replay';
  return 'plain';
}
