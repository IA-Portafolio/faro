import { describe, expect, it } from 'vitest';

import {
  errorIssueHref,
  eventsHref,
  formatInsightCount,
  formatInsightLatency,
  formatInsightPercent,
  insightSeverity,
  summarizeCombinedInsight,
  type CombinedInsightLike
} from './insights';

const row: CombinedInsightLike = {
  service_name: 'checkout',
  span_name: '/api/checkout',
  funnel_from: 'checkout_started',
  funnel_to: 'checkout_completed',
  started_events: 12_453,
  completed_events: 8_901,
  conversion_rate: 0.7148,
  failed_sessions: 3_552,
  linked_error_count: 23,
  linked_error_sessions: 18,
  p95_latency_ms: 230
};

describe('insights helpers', () => {
  it('formats counts, percents and latency for dashboard cards', () => {
    expect(formatInsightCount(12_453)).toBe('12.5k');
    expect(formatInsightPercent(0.7148)).toBe('71.5%');
    expect(formatInsightLatency(230)).toBe('230ms');
    expect(formatInsightLatency(1_250)).toBe('1.25s');
  });

  it('summarizes the combined product/observability insight', () => {
    expect(summarizeCombinedInsight(row)).toBe(
      'checkout: 8,901/12,453 checkout_completed (71.5%). 23 errores linkeados a 18 de 3,552 sesiones fallidas. p95 /api/checkout: 230ms.'
    );
  });

  it('classifies severity from linked failed sessions', () => {
    expect(insightSeverity({ ...row, linked_error_sessions: 0 })).toBe('ok');
    expect(insightSeverity({ ...row, linked_error_sessions: 3 })).toBe('warn');
    expect(insightSeverity(row)).toBe('danger');
  });

  it('builds pillar drill-down links', () => {
    expect(errorIssueHref('abc 123')).toBe('/errors/abc%20123');
    expect(eventsHref('checkout_started', 'proj', '24h')).toBe(
      '/events?event_name=checkout_started&project=proj&range=24h'
    );
  });
});
