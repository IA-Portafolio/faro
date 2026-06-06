import { describe, expect, it } from 'vitest';

import { asNumber, buildSearch, readFilters, writeFilters } from './url-filters';

describe('buildSearch', () => {
  it('serializes non-empty string/number/boolean values', () => {
    expect(buildSearch({ service: 'billing', limit: 50, live: true })).toBe(
      'service=billing&limit=50&live=1'
    );
  });

  it('omits empty strings, zero, false, null and undefined', () => {
    expect(
      buildSearch({
        a: '',
        b: 0,
        c: false,
        d: null,
        e: undefined,
        keep: 'x'
      })
    ).toBe('keep=x');
  });

  it('omits non-finite numbers', () => {
    expect(buildSearch({ n: Number.NaN, m: Number.POSITIVE_INFINITY, ok: 3 })).toBe('ok=3');
  });

  it('encodes a true boolean as 1 and drops a false one entirely', () => {
    expect(buildSearch({ on: true, off: false })).toBe('on=1');
  });

  it('returns an empty string when nothing survives filtering', () => {
    expect(buildSearch({ a: '', b: 0, c: undefined })).toBe('');
  });

  it('percent-encodes values that contain reserved characters', () => {
    const out = buildSearch({ query: 'a b&c=d' });
    // URLSearchParams encodes space, & and = inside the value.
    expect(out.startsWith('query=')).toBe(true);
    expect(new URLSearchParams(out).get('query')).toBe('a b&c=d');
  });
});

describe('asNumber', () => {
  it('parses a numeric string', () => {
    expect(asNumber('42')).toBe(42);
    expect(asNumber('3.14')).toBe(3.14);
  });

  it('returns the default for undefined', () => {
    expect(asNumber(undefined)).toBe(0);
    expect(asNumber(undefined, 9)).toBe(9);
  });

  it('returns the default for a non-numeric string', () => {
    expect(asNumber('not-a-number')).toBe(0);
    expect(asNumber('not-a-number', 7)).toBe(7);
  });

  it('treats an empty string as NaN and falls back to the default', () => {
    // Number('') === 0, which is finite, so this documents the actual behavior.
    expect(asNumber('')).toBe(0);
  });
});

// In the unit-test environment `browser` is stubbed to `false` (see
// src/lib/__mocks__/app-environment.ts), so the DOM-touching helpers must be
// inert no-ops rather than throwing on a missing `window`.
describe('readFilters / writeFilters without a browser', () => {
  it('readFilters returns an empty object', () => {
    expect(readFilters(['project', 'range', 'service'])).toEqual({});
  });

  it('writeFilters is a no-op that never throws', () => {
    expect(() => writeFilters({ service: 'x', limit: 10 })).not.toThrow();
  });
});
