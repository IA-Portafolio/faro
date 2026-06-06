import { describe, expect, it } from 'vitest';

import { apiBase, parseCohortDefinition } from './api';

describe('parseCohortDefinition', () => {
  it('parses a well-formed cohort definition', () => {
    const def = parseCohortDefinition(
      JSON.stringify({ event: 'signup', op: '>=', count: 3, last_days: 30 })
    );
    expect(def).toEqual({ event: 'signup', op: '>=', count: 3, last_days: 30 });
  });

  it('preserves optional filters', () => {
    const raw = JSON.stringify({
      event: 'purchase',
      op: '>',
      count: 1,
      last_days: 7,
      filters: [{ key: 'plan', value: 'pro' }]
    });
    const def = parseCohortDefinition(raw);
    expect(def?.filters).toEqual([{ key: 'plan', value: 'pro' }]);
  });

  it('returns null when event is missing or not a string', () => {
    expect(parseCohortDefinition(JSON.stringify({ op: '>=', count: 1, last_days: 7 }))).toBeNull();
    expect(
      parseCohortDefinition(JSON.stringify({ event: 5, op: '>=', count: 1, last_days: 7 }))
    ).toBeNull();
  });

  it('returns null when count is not a number', () => {
    expect(
      parseCohortDefinition(JSON.stringify({ event: 'x', op: '>=', count: '1', last_days: 7 }))
    ).toBeNull();
  });

  it('returns null when last_days is not a number', () => {
    expect(
      parseCohortDefinition(JSON.stringify({ event: 'x', op: '>=', count: 1, last_days: '7' }))
    ).toBeNull();
  });

  it('returns null when op is missing', () => {
    expect(parseCohortDefinition(JSON.stringify({ event: 'x', count: 1, last_days: 7 }))).toBeNull();
  });

  it('returns null for non-JSON input', () => {
    expect(parseCohortDefinition('not json {')).toBeNull();
    expect(parseCohortDefinition('')).toBeNull();
  });
});

describe('apiBase', () => {
  it('falls back to localhost:8080 outside the browser', () => {
    // No PUBLIC_API_BASE env and no window in the node test env.
    expect(apiBase()).toBe('http://localhost:8080');
  });
});
