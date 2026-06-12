import { qs, type RangeArgs, api } from './core';

export type LogRow = {
  timestamp: string;
  observed_timestamp: string;
  service_name: string;
  severity_text: string;
  severity_number: number;
  body: string;
  trace_id: string;
  span_id: string;
  scope_name: string;
  resource_attributes: Record<string, string>;
  attributes: Record<string, string>;
};

export const fetchLogs = (params: RangeArgs & { service?: string; min_severity?: number; query?: string; trace_id?: string } = {}) =>
  api<LogRow[]>(`/api/v1/logs${qs(params)}`);

export const fetchLogStats = (params: RangeArgs & { service?: string; bucket_seconds?: number } = {}) =>
  api<{ ts: string; service: string; severity: string; count: number }[]>(`/api/v1/logs/stats${qs(params)}`);
