import { qs, type RangeArgs, api } from './core';

export type Dashboard = {
  log_count: number;
  error_count: number;
  service_count: number;
  trace_count: number;
  open_issue_count: number;
  firing_incident_count: number;
  monitors_total: number;
  monitors_down: number;
};

export const fetchDashboard = (r: RangeArgs = {}) => api<Dashboard>(`/api/v1/dashboard${qs(r)}`);
