import { describe, expect, it } from 'vitest';

import {
  formatSessionDuration,
  sessionEventsHref,
  sessionHealth,
  sessionReplayHref,
  sessionUserHref,
  type SessionLikeRow
} from './sessions';

const row: SessionLikeRow = {
  project_id: 'app',
  session_id: 'sess_123',
  distinct_id: 'user_42',
  started_at: '2026-05-24T12:00:00.000Z',
  ended_at: '2026-05-24T12:05:00.000Z',
  duration_seconds: 305,
  pageview_count: 4,
  event_count: 19,
  error_count: 0,
  has_error: 0,
  has_replay: 1,
  replay_event_count: 500,
  replay_chunk_count: 2,
  source: 'web'
};

describe('session helpers', () => {
  it('formats durations compactly', () => {
    expect(formatSessionDuration(0)).toBe('0s');
    expect(formatSessionDuration(59)).toBe('59s');
    expect(formatSessionDuration(65)).toBe('1m 5s');
    expect(formatSessionDuration(3661)).toBe('1h 1m');
  });

  it('builds replay href only when replay exists', () => {
    expect(sessionReplayHref(row)).toBe('/replays/sess_123');
    expect(sessionReplayHref({ ...row, has_replay: 0 })).toBe('');
  });

  it('builds events href with session query and global context', () => {
    expect(sessionEventsHref(row, 'app', '7d')).toBe('/events?query=session_id%3Asess_123&project=app&range=7d');
  });

  it('builds user href preserving project and range', () => {
    expect(sessionUserHref(row, 'app', '24h')).toBe('/users/user_42?project=app&range=24h');
    expect(sessionUserHref({ ...row, distinct_id: '' }, 'app', '24h')).toBe('');
  });

  it('classifies health with errors before replay before plain', () => {
    expect(sessionHealth({ ...row, error_count: 2, has_error: 1 })).toBe('error');
    expect(sessionHealth(row)).toBe('replay');
    expect(sessionHealth({ ...row, has_replay: 0 })).toBe('plain');
  });
});
