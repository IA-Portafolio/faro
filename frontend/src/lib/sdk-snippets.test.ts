import { describe, expect, it } from 'vitest';

import type { Project } from './api';
import {
  curlProbe,
  groupLabels,
  groupOrder,
  otlpCurlProbe,
  snippetsFor
} from './sdk-snippets';

const project: Project = {
  id: '11111111-1111-1111-1111-111111111111',
  slug: 'mi-proyecto',
  name: 'Mi Proyecto',
  description: '',
  ingest_token: 'ingest_abc123def456',
  dsn: 'http://ingest_abc123def456@localhost:8080/1',
  created_at: '2026-01-01T00:00:00Z',
  updated_at: '2026-01-01T00:00:00Z'
};

// In the unit-test environment apiBase() falls back to localhost:8080
// (no PUBLIC_API_BASE, no window) — every snippet should embed that base.
const BASE = 'http://localhost:8080';

describe('curlProbe', () => {
  it('targets the native logs ingest endpoint', () => {
    expect(curlProbe(project)).toContain(`${BASE}/api/v1/ingest/logs`);
  });

  it('embeds the project ingest token as a bearer credential', () => {
    expect(curlProbe(project)).toContain(`Authorization: Bearer ${project.ingest_token}`);
  });

  it('references the project slug in the sample payload', () => {
    expect(curlProbe(project)).toContain(project.slug);
  });
});

describe('otlpCurlProbe', () => {
  it('builds a metrics probe against /v1/metrics', () => {
    const out = otlpCurlProbe(project, 'metrics');
    expect(out).toContain(`${BASE}/v1/metrics`);
    expect(out).toContain('resourceMetrics');
    expect(out).toContain(project.ingest_token);
  });

  it('builds a traces probe against /v1/traces', () => {
    const out = otlpCurlProbe(project, 'traces');
    expect(out).toContain(`${BASE}/v1/traces`);
    expect(out).toContain('resourceSpans');
    expect(out).toContain('"spanId"');
  });

  it('always declares service.name in the resource attributes', () => {
    for (const signal of ['metrics', 'traces'] as const) {
      expect(otlpCurlProbe(project, signal)).toContain('service.name');
    }
  });
});

describe('snippetsFor', () => {
  const snippets = snippetsFor(project);

  it('returns a non-empty list of snippets', () => {
    expect(snippets.length).toBeGreaterThan(0);
  });

  it('gives every snippet a stable shape', () => {
    for (const s of snippets) {
      expect(typeof s.id).toBe('string');
      expect(s.id.length).toBeGreaterThan(0);
      expect(typeof s.label).toBe('string');
      expect(typeof s.install).toBe('string');
      expect(typeof s.code).toBe('string');
      expect(s.code.length).toBeGreaterThan(0);
    }
  });

  it('uses only declared groups', () => {
    for (const s of snippets) {
      expect(groupOrder).toContain(s.group);
    }
  });

  it('uses unique snippet ids', () => {
    const ids = snippets.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('embeds the api base in at least one snippet', () => {
    expect(snippets.some((s) => s.code.includes(BASE))).toBe(true);
  });

  it('masks the token to a prefix…suffix in the node snippet', () => {
    const node = snippets.find((s) => s.id === 'node');
    expect(node).toBeDefined();
    const masked = `${project.ingest_token.slice(0, 6)}…${project.ingest_token.slice(-4)}`;
    expect(node!.code).toContain(masked);
  });
});

describe('group metadata', () => {
  it('groupOrder covers exactly the keys of groupLabels', () => {
    expect([...groupOrder].sort()).toEqual(Object.keys(groupLabels).sort());
  });
});
