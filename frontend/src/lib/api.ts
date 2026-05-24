import { env as publicEnv } from '$env/dynamic/public';

function base(): string {
  const fromEnv = publicEnv.PUBLIC_API_BASE;
  if (fromEnv) return fromEnv.replace(/\/$/, '');
  if (typeof window !== 'undefined') {
    return `${window.location.protocol}//${window.location.hostname}:8080`;
  }
  return 'http://localhost:8080';
}

export type RangeArgs = {
  from?: string;
  to?: string;
  last_minutes?: number;
  limit?: number;
  offset?: number;
  project?: string;
};

function qs(params: Record<string, unknown>): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === '') continue;
    u.set(k, String(v));
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export class UnauthorizedError extends Error {
  constructor() {
    super('unauthorized');
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${base()}${path}`, {
    credentials: 'include',
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers || {}) }
  });
  if (res.status === 401) {
    if (typeof window !== 'undefined' && !window.location.pathname.startsWith('/login')) {
      window.location.assign('/login?next=' + encodeURIComponent(window.location.pathname));
    }
    throw new UnauthorizedError();
  }
  if (!res.ok) {
    const txt = await res.text();
    throw new Error(`HTTP ${res.status}: ${txt}`);
  }
  return res.json() as Promise<T>;
}

export const apiBase = base;

// ---------- Types ----------

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

export type Service = {
  service_name: string;
  log_count: number;
  error_count: number;
  last_seen: string;
};

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
  events_attributes: string[];
};

export type MetricName = {
  metric_name: string;
  metric_type: string;
  metric_unit: string;
  service_name: string;
};

export type Point = { ts: string; value: number };

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

export type Project = {
  id: string;
  slug: string;
  name: string;
  description: string;
  ingest_token: string;
  dsn: string;
  created_at: string;
  updated_at: string;
};

// ---------- Endpoints ----------

export const fetchDashboard = (r: RangeArgs = {}) => api<Dashboard>(`/api/v1/dashboard${qs(r)}`);
export const fetchServices = (r: RangeArgs = {}) => api<Service[]>(`/api/v1/services${qs(r)}`);

export const fetchLogs = (params: RangeArgs & { service?: string; min_severity?: number; query?: string; trace_id?: string } = {}) =>
  api<LogRow[]>(`/api/v1/logs${qs(params)}`);

export const fetchLogStats = (params: RangeArgs & { service?: string; bucket_seconds?: number } = {}) =>
  api<{ ts: string; service: string; severity: string; count: number }[]>(`/api/v1/logs/stats${qs(params)}`);

export const fetchTraces = (params: RangeArgs & { service?: string; status?: string; min_duration_ms?: number } = {}) =>
  api<TraceSummary[]>(`/api/v1/traces${qs(params)}`);

export const fetchTrace = (id: string) => api<SpanRow[]>(`/api/v1/traces/${encodeURIComponent(id)}`);

export const fetchMetricNames = (r: RangeArgs = {}) => api<MetricName[]>(`/api/v1/metrics/names${qs(r)}`);
export const fetchMetricSeries = (params: RangeArgs & { name: string; service?: string; bucket_seconds?: number; agg?: string }) =>
  api<Point[]>(`/api/v1/metrics/series${qs(params)}`);

export const fetchIssues = (r: RangeArgs & { service?: string; status?: string } = {}) =>
  api<Issue[]>(`/api/v1/errors${qs(r)}`);
export const fetchIssue = (fp: string) =>
  api<{ issue: Issue; events: ErrorEvent[] }>(`/api/v1/errors/${encodeURIComponent(fp)}`);
export const updateIssueStatus = (fp: string, body: { status: string; service_name: string; assignee?: string; note?: string }) =>
  api(`/api/v1/errors/${encodeURIComponent(fp)}/status`, { method: 'POST', body: JSON.stringify(body) });

export const fetchMonitors = () => api<Monitor[]>(`/api/v1/monitors`);
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

export const fetchAlertRules = () => api<AlertRule[]>(`/api/v1/alerts/rules`);
export const fetchAlertIncidents = (r: RangeArgs = {}) => api<AlertIncident[]>(`/api/v1/alerts/incidents${qs(r)}`);
export const createAlertRule = (body: Partial<AlertRule>) =>
  api<AlertRule>(`/api/v1/alerts/rules`, { method: 'POST', body: JSON.stringify(body) });
export const updateAlertRule = (id: string, body: Partial<AlertRule>) =>
  api<AlertRule>(`/api/v1/alerts/rules/${id}`, { method: 'PUT', body: JSON.stringify(body) });
export const deleteAlertRule = (id: string) =>
  api(`/api/v1/alerts/rules/${id}`, { method: 'DELETE' });

// ---------- Projects ----------
export const fetchProjects = () => api<Project[]>(`/api/v1/projects`);
export const fetchProject = (slug: string) => api<Project>(`/api/v1/projects/${slug}`);
export const createProject = (body: { name: string; slug?: string; description?: string }) =>
  api<Project>(`/api/v1/projects`, { method: 'POST', body: JSON.stringify(body) });
export const updateProject = (slug: string, body: { name: string; description?: string }) =>
  api<Project>(`/api/v1/projects/${slug}`, { method: 'PUT', body: JSON.stringify(body) });
export const deleteProject = (slug: string) =>
  api(`/api/v1/projects/${slug}`, { method: 'DELETE' });
export const rotateProjectToken = (slug: string) =>
  api<Project>(`/api/v1/projects/${slug}/rotate`, { method: 'POST' });

// ---------- Integrations ----------
export type TelegramIntegration = {
  configured: boolean;
  enabled: boolean;
  bot_token_masked: string;
  default_chat_id: string;
  updated_at: string | null;
  updated_by: string;
};

export type TelegramInput = {
  /** Token nuevo. Deja vacío para conservar el actual. */
  bot_token?: string;
  default_chat_id?: string;
  enabled?: boolean;
};

export const fetchTelegramIntegration = () =>
  api<TelegramIntegration>(`/api/v1/integrations/telegram`);
export const saveTelegramIntegration = (body: TelegramInput) =>
  api<TelegramIntegration>(`/api/v1/integrations/telegram`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
export const deleteTelegramIntegration = () =>
  api<TelegramIntegration>(`/api/v1/integrations/telegram`, { method: 'DELETE' });
export const testTelegramIntegration = (chat_id: string, text?: string) =>
  api<{ ok: boolean }>(`/api/v1/integrations/telegram/test`, {
    method: 'POST',
    body: JSON.stringify({ chat_id, text: text ?? '' })
  });

// ---------- Auth ----------
export type AuthUser = { id: string; email: string; name: string; role: string };

export const login = (body: { email: string; password: string }) =>
  api<AuthUser>(`/api/v1/auth/login`, { method: 'POST', body: JSON.stringify(body) });
export const logout = () =>
  api<{ ok: boolean }>(`/api/v1/auth/logout`, { method: 'POST' });
export const me = () => api<AuthUser>(`/api/v1/auth/me`);

// ---------- Users ----------
export type User = { id: string; email: string; name: string; role: string; created_at: string };

export const fetchUsers = () => api<User[]>(`/api/v1/users`);
export const createUser = (body: { email: string; password: string; name?: string; role?: string }) =>
  api<User>(`/api/v1/users`, { method: 'POST', body: JSON.stringify(body) });
export const updateUser = (id: string, body: { name: string; role: string }) =>
  api<User>(`/api/v1/users/${id}`, { method: 'PUT', body: JSON.stringify(body) });
export const deleteUser = (id: string) =>
  api(`/api/v1/users/${id}`, { method: 'DELETE' });
export const changeUserPassword = (id: string, password: string) =>
  api(`/api/v1/users/${id}/password`, { method: 'PUT', body: JSON.stringify({ password }) });
