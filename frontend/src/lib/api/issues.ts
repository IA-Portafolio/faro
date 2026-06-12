import { api, qs, type RangeArgs } from './core';

export type Issue = {
  fingerprint: string;
  service_name: string;
  exception_type: string;
  message: string;
  event_count: number;
  first_seen: string;
  last_seen: string;
  status: string;
};

export type ErrorEvent = {
  timestamp: string;
  fingerprint: string;
  service_name: string;
  severity_text: string;
  message: string;
  exception_type: string;
  exception_message: string;
  stack_trace: string;
  trace_id: string;
  span_id: string;
  attributes: Record<string, string>;
};

export type IssueSession = {
  session_id: string;
  timestamp: string;
  service_name: string;
  has_replay: number;
};

export const fetchIssues = (r: RangeArgs & { service?: string; status?: string } = {}) =>
  api<Issue[]>(`/api/v1/errors${qs(r)}`);
export const fetchIssue = (fp: string) =>
  api<{ issue: Issue; events: ErrorEvent[] }>(`/api/v1/errors/${encodeURIComponent(fp)}`);
export const updateIssueStatus = (fp: string, body: { status: string; service_name: string; assignee?: string; note?: string }) =>
  api(`/api/v1/errors/${encodeURIComponent(fp)}/status`, { method: 'POST', body: JSON.stringify(body) });
export const fetchIssueSessions = (fp: string) =>
  api<IssueSession[]>(`/api/v1/errors/${encodeURIComponent(fp)}/sessions`);
