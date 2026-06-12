import { qs, type RangeArgs, api } from './core';

export type ServiceDashboardIssue = {
  fingerprint: string;
  service_name: string;
  exception_type: string;
  message: string;
  error_count: number;
  affected_failed_sessions: number;
  first_seen: string;
  last_seen: string;
};

export type ServiceDashboardInsight = {
  project: string;
  service_name: string;
  span_name: string;
  funnel_from: string;
  funnel_to: string;
  started_events: number;
  completed_events: number;
  conversion_rate: number;
  started_sessions: number;
  completed_sessions: number;
  failed_sessions: number;
  linked_error_count: number;
  linked_error_sessions: number;
  p95_latency_ms: number;
  span_count: number;
  summary: string;
  top_errors: ServiceDashboardIssue[];
};

export type ServiceDashboardFilters = RangeArgs & {
  service?: string;
  span_name?: string;
  funnel_from?: string;
  funnel_to?: string;
};

export const fetchServiceDashboardInsight = (params: ServiceDashboardFilters = {}) =>
  api<ServiceDashboardInsight>(`/api/v1/insights/service-dashboard${qs(params)}`);
