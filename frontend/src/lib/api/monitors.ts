import { api, qs, type RangeArgs } from './core';

export type Monitor = {
  id: string;
  name: string;
  method: string;
  url: string;
  headers: Record<string, string>;
  body: string;
  interval_seconds: number;
  timeout_seconds: number;
  expected_status_min: number;
  expected_status_max: number;
  expected_body_regex: string;
  enabled: number;
  created_at: string;
  updated_at: string;
};

export type MonitorResult = {
  monitor_id: string;
  timestamp: string;
  success: number;
  status_code: number;
  duration_ms: number;
  error_message: string;
  response_size: number;
};

export const fetchMonitors = (r: { project?: string } = {}) =>
  api<Monitor[]>(`/api/v1/monitors${qs(r)}`);
export const fetchMonitorResults = (id: string, r: RangeArgs = {}) =>
  api<MonitorResult[]>(`/api/v1/monitors/${id}/results${qs(r)}`);
export const fetchMonitorUptime = (id: string, r: RangeArgs = {}) =>
  api<{ total: number; success: number; uptime_pct: number; avg_duration_ms: number; p95_duration_ms: number }>(
    `/api/v1/monitors/${id}/uptime${qs(r)}`
  );
export const createMonitor = (body: Partial<Monitor>) =>
  api<Monitor>(`/api/v1/monitors`, { method: 'POST', body: JSON.stringify(body) });
export const updateMonitor = (id: string, body: Partial<Monitor>) =>
  api<Monitor>(`/api/v1/monitors/${id}`, { method: 'PUT', body: JSON.stringify(body) });
export const deleteMonitor = (id: string) =>
  api(`/api/v1/monitors/${id}`, { method: 'DELETE' });
