import { qs, type RangeArgs, api } from './core';

export type EventCandidate = { name: string; count: number };

export type FunnelStep = {
  event: string;
  users: number;
  /** [0, 1]. Step 0 siempre vale 1.0. */
  conversion_from_start: number;
  /** [0, 1]. Step 0 siempre vale 1.0. */
  conversion_from_prev: number;
};

export type FunnelResult = {
  steps: FunnelStep[];
  total_entered: number;
  window_seconds: number;
  from: string;
  to: string;
  took_ms: number;
};

export type FunnelRequest = {
  steps: string[];
  window_seconds?: number;
  from?: string;
  to?: string;
  last_minutes?: number;
  project?: string;
};

export const fetchFunnelEvents = (r: RangeArgs = {}) =>
  api<EventCandidate[]>(`/api/v1/funnels/events${qs(r)}`);
export const computeFunnel = (body: FunnelRequest) =>
  api<FunnelResult>(`/api/v1/funnels/compute`, {
    method: 'POST',
    body: JSON.stringify(body)
  });

// Drop-off: "para los que llegaron al paso N pero no a N+1, ¿qué hicieron en los
// siguientes lookahead_seconds?"
export type DropOffEvent = {
  event_name: string;
  users: number;
  occurrences: number;
  /** users / dropped_users ∈ [0, 1]. */
  share: number;
};

export type DropOffResult = {
  step_index: number;
  step_event: string;
  next_event: string;
  dropped_users: number;
  lookahead_seconds: number;
  window_seconds: number;
  from: string;
  to: string;
  top_events: DropOffEvent[];
  took_ms: number;
};

export type DropOffRequest = FunnelRequest & {
  step_index: number;
  lookahead_seconds?: number;
  limit?: number;
};

export const previewDropOff = (body: DropOffRequest) =>
  api<DropOffResult>(`/api/v1/funnels/drop-off`, {
    method: 'POST',
    body: JSON.stringify(body)
  });

// Time-to-convert: histograma del delta entre dos eventos por usuario.
export type TimeBin = {
  lower_seconds: number;
  /** null = catch-all del último bucket sin tope. */
  upper_seconds: number | null;
  users: number;
};

export type TimeToConvertResult = {
  event_from: string;
  event_to: string;
  total_with_from: number;
  total_converted: number;
  p50_seconds: number;
  p90_seconds: number;
  p99_seconds: number;
  min_seconds: number;
  max_seconds_observed: number;
  bins: TimeBin[];
  max_seconds: number;
  from: string;
  to: string;
  took_ms: number;
};

export type TimeToConvertRequest = {
  event_from: string;
  event_to: string;
  max_seconds?: number;
  from?: string;
  to?: string;
  last_minutes?: number;
  project?: string;
};

export const previewTimeToConvert = (body: TimeToConvertRequest) =>
  api<TimeToConvertResult>(`/api/v1/funnels/time-to-convert`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
