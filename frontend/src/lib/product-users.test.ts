import { describe, expect, it } from 'vitest';
import type { ProductEvent } from './api';
import {
  buildProductUserHref,
  groupEventsBySession,
  propertiesPreview,
  shortProductId,
  timelineRows
} from './product-users';

function event(overrides: Partial<ProductEvent>): ProductEvent {
  return {
    timestamp: '2026-05-24T10:00:00.000Z',
    project_id: 'default',
    event_name: 'page_view',
    distinct_id: 'user_42',
    anonymous_id: '',
    session_id: '',
    properties: '',
    user_properties: '',
    context: '',
    source: 'web',
    trace_id: '',
    span_id: '',
    event_id: 'evt-default',
    ...overrides
  };
}

describe('buildProductUserHref', () => {
  it('encodes the distinct id and preserves project/range when present', () => {
    expect(buildProductUserHref('email+demo@example.com', { project: 'shop', range: '24h' }))
      .toBe('/users/email%2Bdemo%40example.com?project=shop&range=24h');
  });

  it('omits empty project and default range', () => {
    expect(buildProductUserHref('user_42', { project: '', range: '1h' }))
      .toBe('/users/user_42');
  });
});

describe('shortProductId', () => {
  it('keeps short ids unchanged and truncates long ids', () => {
    expect(shortProductId('user_42')).toBe('user_42');
    expect(shortProductId('abcdefghijklmnopqrstuvwxyz')).toBe('abcdefghijkl...');
  });

  it('renders an empty id as an em dash', () => {
    expect(shortProductId('')).toBe('—');
  });
});

describe('propertiesPreview', () => {
  it('renders the first primitive json entries', () => {
    expect(propertiesPreview('{"email":"a@example.com","plan":"pro","nested":{"x":1}}'))
      .toBe('email=a@example.com · plan=pro · nested={...}');
  });

  it('returns an empty string for empty or invalid json', () => {
    expect(propertiesPreview('')).toBe('');
    expect(propertiesPreview('{broken')).toBe('');
  });
});

describe('groupEventsBySession', () => {
  it('groups events by non-empty session id and computes bounds', () => {
    const sessions = groupEventsBySession([
      event({ event_id: 'a', session_id: 's1', timestamp: '2026-05-24T10:00:00.000Z' }),
      event({ event_id: 'b', session_id: 's1', timestamp: '2026-05-24T10:05:00.000Z', trace_id: 'tr1' }),
      event({ event_id: 'c', session_id: '', timestamp: '2026-05-24T10:06:00.000Z' })
    ]);

    expect(sessions).toHaveLength(1);
    expect(sessions[0]).toMatchObject({
      session_id: 's1',
      start_ts: '2026-05-24T10:00:00.000Z',
      end_ts: '2026-05-24T10:05:00.000Z',
      event_count: 2,
      trace_count: 1,
      sources: ['web']
    });
  });

  it('sorts sessions by latest event descending', () => {
    const sessions = groupEventsBySession([
      event({ event_id: 'a', session_id: 'old', timestamp: '2026-05-24T10:00:00.000Z' }),
      event({ event_id: 'b', session_id: 'new', timestamp: '2026-05-24T11:00:00.000Z' })
    ]);

    expect(sessions.map((s) => s.session_id)).toEqual(['new', 'old']);
  });
});

describe('timelineRows', () => {
  it('emits session rows before the first event of each session', () => {
    const rows = timelineRows([
      event({ event_id: 'b', session_id: 's1', timestamp: '2026-05-24T10:05:00.000Z' }),
      event({ event_id: 'a', session_id: 's1', timestamp: '2026-05-24T10:00:00.000Z' })
    ]);

    expect(rows.map((r) => r.kind)).toEqual(['session', 'event', 'event']);
    expect(rows[0]).toMatchObject({ kind: 'session', session_id: 's1', event_count: 2 });
  });
});
