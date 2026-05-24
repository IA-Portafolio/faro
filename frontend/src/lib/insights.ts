export type InsightSeverity = 'ok' | 'warn' | 'danger';

export type CombinedInsightLike = {
  service_name: string;
  span_name: string;
  funnel_from: string;
  funnel_to: string;
  started_events: number;
  completed_events: number;
  conversion_rate: number;
  failed_sessions: number;
  linked_error_count: number;
  linked_error_sessions: number;
  p95_latency_ms: number;
};

function fmtInt(n: number): string {
  return Math.round(Math.max(0, Number.isFinite(n) ? n : 0)).toLocaleString('en-US');
}

export function formatInsightCount(n: number): string {
  if (!Number.isFinite(n)) return '0';
  const value = Math.max(0, n);
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}k`;
  return fmtInt(value);
}

export function formatInsightPercent(rate: number): string {
  if (!Number.isFinite(rate)) return '0.0%';
  return `${(rate * 100).toFixed(1)}%`;
}

export function formatInsightLatency(ms: number): string {
  if (!Number.isFinite(ms) || ms <= 0) return '0ms';
  if (ms >= 1_000) return `${(ms / 1_000).toFixed(ms >= 10_000 ? 1 : 2)}s`;
  return `${Math.round(ms).toLocaleString()}ms`;
}

export function summarizeCombinedInsight(row: CombinedInsightLike): string {
  return `${row.service_name}: ${fmtInt(row.completed_events)}/${fmtInt(row.started_events)} ${row.funnel_to} (${formatInsightPercent(row.conversion_rate)}). ${fmtInt(row.linked_error_count)} errores linkeados a ${fmtInt(row.linked_error_sessions)} de ${fmtInt(row.failed_sessions)} sesiones fallidas. p95 ${row.span_name}: ${formatInsightLatency(row.p95_latency_ms)}.`;
}

export function insightSeverity(row: Pick<CombinedInsightLike, 'linked_error_sessions'>): InsightSeverity {
  if (row.linked_error_sessions <= 0) return 'ok';
  if (row.linked_error_sessions < 10) return 'warn';
  return 'danger';
}

export function errorIssueHref(fingerprint: string): string {
  return `/errors/${encodeURIComponent(fingerprint)}`;
}

export function eventsHref(eventName: string, project?: string, range?: string): string {
  const params = new URLSearchParams();
  params.set('event_name', eventName);
  if (project) params.set('project', project);
  if (range) params.set('range', range);
  return `/events?${params.toString()}`;
}

export function tracesHref(service: string, range?: string): string {
  const params = new URLSearchParams();
  if (service) params.set('service', service);
  if (range) params.set('range', range);
  const query = params.toString();
  return `/traces${query ? `?${query}` : ''}`;
}
