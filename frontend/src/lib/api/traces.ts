import { qs, type RangeArgs, api } from './core';

export type TraceSummary = {
  trace_id: string;
  timestamp: string;
  service_name: string;
  root_name: string;
  duration_ns: number;
  status_code: string;
  span_count: number;
};

export type SpanRow = {
  timestamp: string;
  trace_id: string;
  span_id: string;
  parent_span_id: string;
  name: string;
  kind: string;
  service_name: string;
  duration_ns: number;
  status_code: string;
  status_message: string;
  resource_attributes: Record<string, string>;
  span_attributes: Record<string, string>;
  events_timestamps: string[];
  events_names: string[];
  /** Por evento, JSON serializado con sus atributos. */
  events_attributes: string[];
  /** IDs de las trazas referenciadas por links salientes. */
  links_trace_ids?: string[];
  /** IDs de spans correspondientes a `links_trace_ids` (misma longitud). */
  links_span_ids?: string[];
};

export const fetchTraces = (params: RangeArgs & { service?: string; status?: string; min_duration_ms?: number } = {}) =>
  api<TraceSummary[]>(`/api/v1/traces${qs(params)}`);

export const fetchTrace = (id: string) => api<SpanRow[]>(`/api/v1/traces/${encodeURIComponent(id)}`);
