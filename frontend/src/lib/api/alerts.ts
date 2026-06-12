import { api, qs, type RangeArgs } from './core';

export type AlertRule = {
  id: string;
  name: string;
  description: string;
  source: string;
  query: string;
  condition: string;
  threshold: number;
  window_seconds: number;
  interval_seconds: number;
  severity: string;
  notification_targets: string[];
  enabled: number;
  created_at: string;
  updated_at: string;
};

export type AlertIncident = {
  id: string;
  rule_id: string;
  rule_name: string;
  started_at: string;
  resolved_at: string | null;
  value: number;
  threshold: number;
  severity: string;
  status: string;
  note: string;
};

export const fetchAlertRules = (r: { project?: string } = {}) =>
  api<AlertRule[]>(`/api/v1/alerts/rules${qs(r)}`);
export const fetchAlertIncidents = (r: RangeArgs = {}) => api<AlertIncident[]>(`/api/v1/alerts/incidents${qs(r)}`);
export const createAlertRule = (body: Partial<AlertRule>) =>
  api<AlertRule>(`/api/v1/alerts/rules`, { method: 'POST', body: JSON.stringify(body) });
export const updateAlertRule = (id: string, body: Partial<AlertRule>) =>
  api<AlertRule>(`/api/v1/alerts/rules/${id}`, { method: 'PUT', body: JSON.stringify(body) });
export const deleteAlertRule = (id: string) =>
  api(`/api/v1/alerts/rules/${id}`, { method: 'DELETE' });
