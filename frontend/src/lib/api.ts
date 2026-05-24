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
  /** Cursor keyset: timestamp del último item de la página anterior. El backend
   *  filtra `WHERE <column> < cursor` antes del LIMIT, sin escanear las páginas
   *  saltadas como hacía el viejo `offset`. */
  cursor?: string;
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

export type ServiceMapNode = {
  service: string;
  calls: number;
  errors: number;
  p95_ms: number;
  is_root: number;
};

export type ServiceMapEdge = {
  source: string;
  target: string;
  calls: number;
  errors: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
};

export type ServiceMap = {
  nodes: ServiceMapNode[];
  edges: ServiceMapEdge[];
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
  /** Por evento, JSON serializado con sus atributos. */
  events_attributes: string[];
  /** IDs de las trazas referenciadas por links salientes. */
  links_trace_ids?: string[];
  /** IDs de spans correspondientes a `links_trace_ids` (misma longitud). */
  links_span_ids?: string[];
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
export const fetchServiceMap = (r: RangeArgs = {}) => api<ServiceMap>(`/api/v1/services/map${qs(r)}`);

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

// ---------- Session replays ----------
export type ReplaySummary = {
  session_id: string;
  service_name: string;
  start_ts: string;
  end_ts: string;
  event_count: number;
  chunk_count: number;
  user_id: string;
  page_url: string;
};

export type ReplayPayload = {
  session_id: string;
  service_name: string;
  start_ts: string;
  end_ts: string;
  event_count: number;
  page_url: string;
  user_id: string;
  user_agent: string;
  /** Array de eventos rrweb concatenados de todos los chunks, en orden. */
  events: unknown[];
};

export type IssueSession = {
  session_id: string;
  timestamp: string;
  service_name: string;
  has_replay: number;
};

export const fetchReplays = (r: RangeArgs & { service?: string; session_id?: string } = {}) =>
  api<ReplaySummary[]>(`/api/v1/replays${qs(r)}`);
export const fetchReplay = (sessionId: string) =>
  api<ReplayPayload>(`/api/v1/replays/${encodeURIComponent(sessionId)}`);
export const fetchIssueSessions = (fp: string) =>
  api<IssueSession[]>(`/api/v1/errors/${encodeURIComponent(fp)}/sessions`);

// ---------- Product events (6º pilar) ----------
export type ProductEvent = {
  timestamp: string;
  project_id: string;
  event_name: string;
  distinct_id: string;
  anonymous_id: string;
  session_id: string;
  /** JSON serializado. Parsear con JSON.parse cuando se necesita renderizar. */
  properties: string;
  user_properties: string;
  context: string;
  source: string;
  trace_id: string;
  span_id: string;
  event_id: string;
};

export type EventBucket = {
  ts: string;
  event_name: string;
  count: number;
};

export type EventFilters = RangeArgs & {
  event_name?: string;
  distinct_id?: string;
  anonymous_id?: string;
  session_id?: string;
  trace_id?: string;
  source?: string;
  query?: string;
  /** Pares `key:value` para filtrar properties. Cada entrada se envía como un
   *  query param `prop=<key>:<value>` separado. */
  prop?: string[];
};

function eventsQs(params: EventFilters): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (k === 'prop') continue;
    if (v === undefined || v === null || v === '') continue;
    u.set(k, String(v));
  }
  for (const p of params.prop ?? []) {
    if (p && p.includes(':')) u.append('prop', p);
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export const fetchEvents = (params: EventFilters = {}) =>
  api<ProductEvent[]>(`/api/v1/events${eventsQs(params)}`);

export const fetchEventStats = (
  params: RangeArgs & { event_name?: string; bucket_seconds?: number } = {}
) => api<EventBucket[]>(`/api/v1/events/stats${qs(params)}`);

// ---------- Retention (product analytics) ----------
export type RetentionCohort = {
  cohort_date: string;
  cohort_size: number;
  d1_users: number;
  d7_users: number;
  d30_users: number;
};

export type RetentionResult = {
  from: string;
  to: string;
  event_name: string;
  interval: 'day';
  columns: Array<1 | 7 | 30>;
  cohorts: RetentionCohort[];
  took_ms: number;
};

export type RetentionFilters = RangeArgs & {
  event_name?: string;
  interval?: 'day';
};

export const fetchRetention = (params: RetentionFilters = {}) =>
  api<RetentionResult>(`/api/v1/retention${qs(params)}`);

// ---------- Product users ----------
export type ProductUserSummary = {
  project_id: string;
  distinct_id: string;
  first_seen: string;
  last_seen: string;
  anonymous_ids: string[];
  sources: string[];
  event_count: number;
  /** JSON serializado con las últimas user properties conocidas. */
  properties: string;
};

export type ProductUserDeviceBreakdown = {
  source: string;
  event_count: number;
  last_seen: string;
  anonymous_id_count: number;
};

export type ProductUserDetail = {
  project_id: string;
  distinct_id: string;
  first_seen: string;
  last_seen: string;
  anonymous_ids: string[];
  sources: string[];
  event_count: number;
  properties: string;
  devices: ProductUserDeviceBreakdown[];
};

export type ProductUserFilters = RangeArgs & {
  query?: string;
  source?: string | string[];
};

function productUsersQs(params: ProductUserFilters): string {
  const u = new URLSearchParams();
  for (const [key, value] of Object.entries(params)) {
    if (key === 'source') continue;
    if (value === undefined || value === null || value === '') continue;
    u.set(key, String(value));
  }
  const sources = Array.isArray(params.source) ? params.source : [params.source];
  for (const source of sources) {
    if (source) u.append('source', source);
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export const fetchProductUsers = (params: ProductUserFilters = {}) =>
  api<ProductUserSummary[]>(`/api/v1/product_users${productUsersQs(params)}`);

export const fetchProductUser = (distinctId: string, params: RangeArgs = {}) =>
  api<ProductUserDetail>(`/api/v1/product_users/${encodeURIComponent(distinctId)}${qs(params)}`);

export const fetchProductUserEvents = (
  distinctId: string,
  params: RangeArgs & { source?: string } = {}
) => api<ProductEvent[]>(
  `/api/v1/product_users/${encodeURIComponent(distinctId)}/events${qs(params)}`
);

// ---------- Funnels (product analytics) ----------
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
  events: string[];
  window_seconds?: number;
  from?: string;
  to?: string;
  last_minutes?: number;
  project?: string;
};

export const fetchFunnelEvents = (r: RangeArgs = {}) =>
  api<EventCandidate[]>(`/api/v1/funnels/events${qs(r)}`);
export const previewFunnel = (body: FunnelRequest) =>
  api<FunnelResult>(`/api/v1/funnels/preview`, {
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

// ---------- Experiments (feature flag A/B stats) ----------
export type ExperimentVariantResult = {
  variant: 'A' | 'B' | string;
  sample: number;
  conversions: number;
  conversion_rate: number;
};

export type ExperimentAnalyzeRequest = {
  flag_key: string;
  conversion_event: string;
  project?: string;
  from?: string;
  to?: string;
  last_minutes?: number;
};

export type ExperimentAnalyzeResult = {
  flag_key: string;
  conversion_event: string;
  project: string;
  from: string;
  to: string;
  variants: ExperimentVariantResult[];
  sample: number;
  winner: string;
  absolute_delta: number;
  relative_lift: number;
  p_value: number;
  ci95_low: number;
  ci95_high: number;
  summary: string;
};

export const analyzeExperiment = (body: ExperimentAnalyzeRequest) =>
  api<ExperimentAnalyzeResult>(`/api/v1/experiments/analyze`, {
    method: 'POST',
    body: JSON.stringify(body)
  });

// ---------- Cohorts (segmentación de usuarios) ----------

export type CohortFilter = { key: string; value: string };
export type CohortDefinition = {
  event: string;
  op: '==' | '>=' | '>' | '<=' | '<';
  count: number;
  /** Ventana hacia atrás desde ahora, en días. */
  last_days: number;
  filters?: CohortFilter[];
};

export type Cohort = {
  id: string;
  project_id: string;
  name: string;
  description: string;
  /** JSON serializado de [`CohortDefinition`]. Usar `parseCohortDefinition`. */
  definition: string;
  created_at: string;
  updated_at: string;
  created_by: string;
  deleted: number;
  version: number;
};

export type CohortInput = {
  name: string;
  description?: string;
  project?: string;
  definition: CohortDefinition;
};

export type CohortPreview = {
  size: number;
  sample: string[];
  took_ms: number;
};

export type CohortRetentionPoint = {
  /** 0 = hoy, 1 = ayer, … */
  day_back: number;
  active_users: number;
};

export type CohortRetention = {
  cohort_size: number;
  horizon_days: number;
  points: CohortRetentionPoint[];
  took_ms: number;
};

export type CohortOverlap = {
  size_a: number;
  size_b: number;
  intersection: number;
  /** Jaccard ∈ [0, 1]. */
  jaccard: number;
  took_ms: number;
};

/** Parser tolerante: una definition malformada se reporta a UI sin reventar la pantalla. */
export function parseCohortDefinition(raw: string): CohortDefinition | null {
  try {
    const v = JSON.parse(raw) as CohortDefinition;
    if (!v.event || typeof v.event !== 'string') return null;
    if (typeof v.count !== 'number') return null;
    if (typeof v.last_days !== 'number') return null;
    if (!v.op) return null;
    return v;
  } catch {
    return null;
  }
}

export const listCohorts = (r: { project?: string } = {}) =>
  api<Cohort[]>(`/api/v1/cohorts${qs(r)}`);
export const getCohort = (id: string) =>
  api<Cohort>(`/api/v1/cohorts/${encodeURIComponent(id)}`);
export const createCohort = (body: CohortInput) =>
  api<Cohort>(`/api/v1/cohorts`, { method: 'POST', body: JSON.stringify(body) });
export const updateCohort = (id: string, body: CohortInput) =>
  api<Cohort>(`/api/v1/cohorts/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
export const deleteCohort = (id: string) =>
  api<{ ok: boolean }>(`/api/v1/cohorts/${encodeURIComponent(id)}`, { method: 'DELETE' });

export const previewCohort = (body: {
  project?: string;
  definition: CohortDefinition;
  sample_limit?: number;
}) => api<CohortPreview>(`/api/v1/cohorts/preview`, { method: 'POST', body: JSON.stringify(body) });

export const fetchCohortUsers = (id: string, params: { limit?: number } = {}) =>
  api<CohortPreview>(`/api/v1/cohorts/${encodeURIComponent(id)}/users${qs(params)}`);

export const fetchCohortRetention = (id: string, params: { horizon_days?: number } = {}) =>
  api<CohortRetention>(`/api/v1/cohorts/${encodeURIComponent(id)}/retention${qs(params)}`);

export const fetchCohortOverlap = (id: string, other: string) =>
  api<CohortOverlap>(`/api/v1/cohorts/${encodeURIComponent(id)}/overlap${qs({ other })}`);

export const fetchAlertRules = (r: { project?: string } = {}) =>
  api<AlertRule[]>(`/api/v1/alerts/rules${qs(r)}`);
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

// ---------- Redaction config por proyecto ----------
export type RedactionCustomRule = {
  name: string;
  pattern: string;
  replacement: string;
};
export type RedactionConfig = {
  enabled: boolean;
  /** Slugs de built-ins activados (e.g. 'email', 'jwt'). */
  builtins: string[];
  custom: RedactionCustomRule[];
};
export type RedactionBuiltinInfo = {
  slug: string;
  label: string;
  description: string;
};
export type RedactionView = {
  config: RedactionConfig;
  available_builtins: RedactionBuiltinInfo[];
};

export const fetchRedaction = (slug: string) =>
  api<RedactionView>(`/api/v1/projects/${slug}/redaction`);
export const saveRedaction = (slug: string, body: RedactionConfig) =>
  api<RedactionView>(`/api/v1/projects/${slug}/redaction`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });

// ---------- Allowed origins (RUM SDK origin verification) ----------
export type OriginConfig = {
  enabled: boolean;
  origins: string[];
};
export type OriginsView = { config: OriginConfig };

export const fetchOrigins = (slug: string) =>
  api<OriginsView>(`/api/v1/projects/${slug}/origins`);
export const saveOrigins = (slug: string, body: OriginConfig) =>
  api<OriginsView>(`/api/v1/projects/${slug}/origins`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });

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

// ---------- Notification channels (multi-instancia) ----------
export type ChannelKind =
  | 'webhook'
  | 'slack'
  | 'discord'
  | 'pagerduty'
  | 'opsgenie'
  | 'email_resend'
  | 'telegram';

/** El backend devuelve `config` con los secretos enmascarados. Para editar,
 *  el frontend manda los campos secretos VACÍOS para conservar el actual. */
export type NotificationChannel = {
  id: string;
  name: string;
  kind: ChannelKind;
  enabled: boolean;
  config: Record<string, unknown>;
  created_at: string;
  updated_at: string;
  updated_by: string;
};

export type ChannelInput = {
  /** Sólo en POST. Si vacío, el backend genera uno a partir del `name`. */
  id?: string;
  name: string;
  kind: ChannelKind;
  enabled: boolean;
  config: Record<string, unknown>;
};

export const fetchChannelKinds = () =>
  api<{ kinds: ChannelKind[] }>(`/api/v1/integrations/channels/kinds`);
export const listChannels = () =>
  api<NotificationChannel[]>(`/api/v1/integrations/channels`);
export const getChannel = (id: string) =>
  api<NotificationChannel>(`/api/v1/integrations/channels/${encodeURIComponent(id)}`);
export const createChannel = (body: ChannelInput) =>
  api<NotificationChannel>(`/api/v1/integrations/channels`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
export const updateChannel = (id: string, body: ChannelInput) =>
  api<NotificationChannel>(`/api/v1/integrations/channels/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
export const deleteChannel = (id: string) =>
  api<{ deleted: string }>(`/api/v1/integrations/channels/${encodeURIComponent(id)}`, {
    method: 'DELETE'
  });
export const testChannel = (id: string, note?: string) =>
  api<{ ok: boolean; kind: string }>(
    `/api/v1/integrations/channels/${encodeURIComponent(id)}/test`,
    {
      method: 'POST',
      body: JSON.stringify({ note: note ?? '' })
    }
  );

// ---------- Auth ----------
export type AuthUser = { id: string; email: string; name: string; role: string };

/** Respuesta del POST /auth/login. Si `needs_totp` está, el cliente debe llamar a
 *  /auth/login/2fa con el `challenge_token` y un código TOTP/recovery. */
export type LoginResponse =
  | (AuthUser & { needs_totp?: false })
  | { needs_totp: true; challenge_token: string; expires_in_secs: number };

export const login = (body: { email: string; password: string }) =>
  api<LoginResponse>(`/api/v1/auth/login`, { method: 'POST', body: JSON.stringify(body) });
export const loginTotp = (body: { challenge_token: string; code: string; recovery?: boolean }) =>
  api<AuthUser>(`/api/v1/auth/login/2fa`, { method: 'POST', body: JSON.stringify(body) });
export const logout = () =>
  api<{ ok: boolean }>(`/api/v1/auth/logout`, { method: 'POST' });
export const me = () => api<AuthUser>(`/api/v1/auth/me`);

// ---------- Sessions + 2FA ----------
export type SessionInfo = {
  token_hash: string;
  created_at: string;
  expires_at: string;
  is_current: boolean;
};

export const fetchSessions = () => api<SessionInfo[]>(`/api/v1/me/sessions`);
export const revokeOtherSessions = () =>
  api<{ revoked: number }>(`/api/v1/me/sessions/revoke-others`, { method: 'POST' });

export type TwoFaStatus = { enabled: boolean; recovery_codes_remaining: number };
export type TwoFaSetup = {
  secret_base32: string;
  otpauth_url: string;
  /** SVG inline del QR, listo para inyectar con {@html}. */
  qr_svg: string;
};
export type TwoFaEnableResult = { enabled: boolean; recovery_codes: string[] };

export const fetchTwoFaStatus = () => api<TwoFaStatus>(`/api/v1/me/security/2fa`);
export const twoFaSetup = () => api<TwoFaSetup>(`/api/v1/me/security/2fa/setup`, { method: 'POST' });
export const twoFaEnable = (code: string) =>
  api<TwoFaEnableResult>(`/api/v1/me/security/2fa/enable`, {
    method: 'POST',
    body: JSON.stringify({ code })
  });
export const twoFaDisable = (body: { password: string; code?: string; recovery_code?: string }) =>
  api<{ enabled: false }>(`/api/v1/me/security/2fa/disable`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
export const twoFaRegenRecovery = (body: { password: string; code: string }) =>
  api<TwoFaEnableResult>(`/api/v1/me/security/2fa/recovery-codes`, {
    method: 'POST',
    body: JSON.stringify(body)
  });

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

// ---------- Preferences ----------
export type ThemePref = 'light' | 'dark' | 'system';
export type TimeRangePref = '5m' | '15m' | '1h' | '6h' | '24h' | '7d';

export type Preferences = {
  theme: ThemePref;
  /** Slug del proyecto por defecto. `''` significa "ver todos". */
  default_project: string;
  default_time_range: TimeRangePref;
  updated_at: string;
};

/** Payload de actualización parcial (PATCH-style): omitir un campo lo deja intacto. */
export type PreferencesPatch = Partial<Pick<Preferences, 'theme' | 'default_project' | 'default_time_range'>>;

export const fetchPreferences = () => api<Preferences>(`/api/v1/me/preferences`);
export const savePreferences = (body: PreferencesPatch) =>
  api<Preferences>(`/api/v1/me/preferences`, { method: 'PUT', body: JSON.stringify(body) });
