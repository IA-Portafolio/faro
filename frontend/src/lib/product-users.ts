import type { ProductEvent } from './api';

export type ProductUserHrefOptions = {
  project?: string;
  range?: string;
};

export type ProductSessionGroup = {
  session_id: string;
  start_ts: string;
  end_ts: string;
  event_count: number;
  trace_count: number;
  sources: string[];
  events: ProductEvent[];
};

export type TimelineRow =
  | (ProductSessionGroup & { kind: 'session' })
  | { kind: 'event'; event: ProductEvent };

export function buildProductUserHref(
  distinctId: string,
  opts: ProductUserHrefOptions = {}
): string {
  const params = new URLSearchParams();
  if (opts.project) params.set('project', opts.project);
  if (opts.range && opts.range !== '1h') params.set('range', opts.range);
  const query = params.toString();
  return `/users/${encodeURIComponent(distinctId)}${query ? `?${query}` : ''}`;
}

export function shortProductId(value: string | undefined): string {
  if (!value) return '—';
  return value.length > 15 ? `${value.slice(0, 12)}...` : value;
}

export function propertiesPreview(raw: string, maxEntries = 3): string {
  if (!raw) return '';
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (parsed === null || typeof parsed !== 'object' || Array.isArray(parsed)) return '';
    return Object.entries(parsed as Record<string, unknown>)
      .slice(0, maxEntries)
      .map(([key, value]) => {
        const rendered = value === null || typeof value !== 'object' ? String(value) : '{...}';
        return `${key}=${rendered}`;
      })
      .join(' · ');
  } catch {
    return '';
  }
}

function byTimeDesc(a: ProductEvent, b: ProductEvent): number {
  return Date.parse(b.timestamp) - Date.parse(a.timestamp);
}

function byTimeAsc(a: ProductEvent, b: ProductEvent): number {
  return Date.parse(a.timestamp) - Date.parse(b.timestamp);
}

export function groupEventsBySession(events: ProductEvent[]): ProductSessionGroup[] {
  const bySession = new Map<string, ProductEvent[]>();
  for (const ev of events) {
    if (!ev.session_id) continue;
    bySession.set(ev.session_id, [...(bySession.get(ev.session_id) ?? []), ev]);
  }

  const groups: ProductSessionGroup[] = [];
  for (const [session_id, sessionEvents] of bySession.entries()) {
    const ordered = sessionEvents.slice().sort(byTimeAsc);
    const sources = Array.from(new Set(ordered.map((ev) => ev.source).filter(Boolean))).sort();
    const traceIds = new Set(ordered.map((ev) => ev.trace_id).filter(Boolean));
    groups.push({
      session_id,
      start_ts: ordered[0]?.timestamp ?? '',
      end_ts: ordered[ordered.length - 1]?.timestamp ?? '',
      event_count: ordered.length,
      trace_count: traceIds.size,
      sources,
      events: ordered
    });
  }

  return groups.sort((a, b) => Date.parse(b.end_ts) - Date.parse(a.end_ts));
}

export function timelineRows(events: ProductEvent[]): TimelineRow[] {
  const ordered = events.slice().sort(byTimeDesc);
  const sessions = new Map(groupEventsBySession(events).map((session) => [session.session_id, session]));
  const emittedSessions = new Set<string>();
  const rows: TimelineRow[] = [];

  for (const ev of ordered) {
    const session = ev.session_id ? sessions.get(ev.session_id) : undefined;
    if (session && !emittedSessions.has(session.session_id)) {
      rows.push({ kind: 'session', ...session });
      emittedSessions.add(session.session_id);
    }
    rows.push({ kind: 'event', event: ev });
  }

  return rows;
}
