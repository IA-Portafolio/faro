# Product Users Profile Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `/users` as the product end-user list and `/users/[distinct_id]` as a linkable user profile with chronological events, session grouping, and trace links.

**Architecture:** Keep backend unchanged for this iteration and consume the existing `/api/v1/product-users` endpoints from the Svelte frontend. Put URL/timeline logic in pure TypeScript helpers with Vitest coverage, then keep Svelte pages focused on data loading and rendering.

**Tech Stack:** SvelteKit 2, Svelte 5, TypeScript, Vitest, existing Faro REST API helpers.

---

## File Structure

- Create `frontend/src/lib/product-users.ts`
  - Pure helpers for user profile URLs, properties previews, session grouping, and timeline rows.
- Create `frontend/src/lib/product-users.test.ts`
  - Vitest coverage for those helpers.
- Modify `frontend/src/lib/api.ts`
  - Add product-user response types and fetch functions for existing backend endpoints.
- Modify `frontend/src/routes/users/+page.svelte`
  - Replace the current redirect to `/settings/users` with the product end-user list.
- Create `frontend/src/routes/users/[distinct_id]/+page.svelte`
  - Dedicated profile page with summary, device/source breakdown, session grouping, event timeline, and trace links.
- Modify `frontend/src/lib/components/Sidebar.svelte`
  - Add a first-class "Usuarios" product analytics navigation item pointing to `/users`.
- Modify `frontend/src/lib/palette.ts`
  - Add a static command for `/users` and rename the dashboard-admin users command to avoid ambiguity.
- Modify `frontend/src/lib/palette.test.ts`
  - Assert the new product-users command exists and routes distinctly from settings users.

## Task 1: Product User Helpers

**Files:**

- Create: `frontend/src/lib/product-users.ts`
- Create: `frontend/src/lib/product-users.test.ts`

- [ ] **Step 1: Write the failing tests**

Create `frontend/src/lib/product-users.test.ts`:

```ts
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cd frontend
npm test -- src/lib/product-users.test.ts
```

Expected: FAIL because `./product-users` does not exist.

- [ ] **Step 3: Implement the helpers**

Create `frontend/src/lib/product-users.ts`:

```ts
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

export function buildProductUserHref(distinctId: string, opts: ProductUserHrefOptions = {}): string {
  const params = new URLSearchParams();
  if (opts.project) params.set('project', opts.project);
  if (opts.range && opts.range !== '1h') params.set('range', opts.range);
  const qs = params.toString();
  return `/users/${encodeURIComponent(distinctId)}${qs ? `?${qs}` : ''}`;
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
```

- [ ] **Step 4: Run tests to verify they pass**

Run:

```bash
cd frontend
npm test -- src/lib/product-users.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add -- frontend/src/lib/product-users.ts frontend/src/lib/product-users.test.ts
git commit -m "feat(frontend): add product user timeline helpers" -- frontend/src/lib/product-users.ts frontend/src/lib/product-users.test.ts
```

## Task 2: Product User API Client

**Files:**

- Modify: `frontend/src/lib/api.ts`

- [ ] **Step 1: Add product user types and fetchers**

Modify `frontend/src/lib/api.ts` after the product event fetchers:

```ts
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
  api<ProductUserSummary[]>(`/api/v1/product-users${productUsersQs(params)}`);

export const fetchProductUser = (distinctId: string, params: RangeArgs = {}) =>
  api<ProductUserDetail>(`/api/v1/product-users/${encodeURIComponent(distinctId)}${qs(params)}`);

export const fetchProductUserEvents = (
  distinctId: string,
  params: RangeArgs & { source?: string } = {}
) => api<ProductEvent[]>(
  `/api/v1/product-users/${encodeURIComponent(distinctId)}/events${qs(params)}`
);
```

- [ ] **Step 2: Run the TypeScript checker**

Run:

```bash
cd frontend
npm run check
```

Expected: either PASS, or unrelated existing errors. Product-user API additions must not introduce new type errors.

- [ ] **Step 3: Commit**

```bash
git add -- frontend/src/lib/api.ts
git commit -m "feat(frontend): add product user api client" -- frontend/src/lib/api.ts
```

## Task 3: Product Users List Page

**Files:**

- Modify: `frontend/src/routes/users/+page.svelte`

- [ ] **Step 1: Replace the redirect with a product user list**

Replace `frontend/src/routes/users/+page.svelte` with:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { fetchProductUsers, type ProductUserSummary } from '$lib/api';
  import { buildProductUserHref, propertiesPreview, shortProductId } from '$lib/product-users';
  import { formatTimestamp, rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import SkeletonLogRows from '$lib/components/SkeletonLogRows.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';

  let users: ProductUserSummary[] = [];
  let loading = false;
  let error = '';
  let query = '';
  let source = '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      users = await fetchProductUsers({
        project: $selectedProject || undefined,
        last_minutes: rangeMinutes($timeRange),
        query: query || undefined,
        source: source || undefined,
        limit: 500
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      users = [];
    } finally {
      loading = false;
    }
  }

  function userHref(id: string): string {
    return buildProductUserHref(id, {
      project: $selectedProject || undefined,
      range: $timeRange
    });
  }

  let prevProject = $selectedProject;
  let prevRange = $timeRange;
  $: if (prevProject !== $selectedProject || prevRange !== $timeRange) {
    prevProject = $selectedProject;
    prevRange = $timeRange;
    void load();
  }

  onMount(load);
</script>

<div class="page-header">
  <div>
    <h1 class="page-title">Usuarios</h1>
    <div class="muted subtitle">End-users del producto capturados por SDKs cliente.</div>
  </div>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button on:click={load} disabled={loading}>{loading ? 'Cargando...' : 'Recargar'}</button>
  </div>
</div>

<div class="toolbar">
  <input
    placeholder="Buscar distinct_id o properties..."
    bind:value={query}
    on:keydown={(e) => e.key === 'Enter' && load()}
    style="min-width: 260px;"
    data-search-input
  />
  <select bind:value={source} on:change={load}>
    <option value="">Cualquier source</option>
    <option value="web">web</option>
    <option value="mobile">mobile</option>
    <option value="server">server</option>
    <option value="backend">backend</option>
  </select>
  <button on:click={load} disabled={loading}>Buscar</button>
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

<div class="users-table">
  <div class="users-head">
    <div>Usuario</div>
    <div>Last seen</div>
    <div>First seen</div>
    <div>Eventos</div>
    <div>Sources</div>
    <div>Anon IDs</div>
    <div>Properties</div>
  </div>

  {#if loading && users.length === 0}
    <SkeletonLogRows rows={10} />
  {:else}
    {#each users as user (user.project_id + ':' + user.distinct_id)}
      <a class="user-row" href={userHref(user.distinct_id)} data-sveltekit-preload-data="hover">
        <div class="mono user-id" title={user.distinct_id}>{shortProductId(user.distinct_id)}</div>
        <div class="mono muted">{formatTimestamp(user.last_seen)}</div>
        <div class="mono muted">{formatTimestamp(user.first_seen)}</div>
        <div class="mono tabular">{user.event_count.toLocaleString()}</div>
        <div class="chips">
          {#each user.sources as s}
            <span class="chip mono">{s}</span>
          {/each}
        </div>
        <div class="mono muted">{user.anonymous_ids.length}</div>
        <div class="mono props" title={propertiesPreview(user.properties)}>
          {propertiesPreview(user.properties)}
        </div>
      </a>
    {/each}
  {/if}
</div>

{#if !loading && users.length === 0}
  <OnboardingEmpty kind="events" filteredOut={!!(query || source)} />
{/if}

<style>
  .subtitle { font-size: 12px; margin-top: 2px; }

  .users-table {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .users-head,
  .user-row {
    display: grid;
    grid-template-columns: minmax(170px, 1.2fr) 180px 180px 90px minmax(120px, 0.8fr) 80px minmax(180px, 1.4fr);
    gap: 12px;
    align-items: center;
  }
  .users-head {
    padding: 8px 12px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
  }
  .user-row {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    text-decoration: none;
    font-size: 12.5px;
  }
  .user-row:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }
  .user-row:last-child { border-bottom: 0; }
  .user-id,
  .props {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chips {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip {
    border: 1px solid var(--border);
    background: var(--bg);
    border-radius: 10px;
    padding: 1px 7px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .error-box {
    color: var(--danger);
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
  }
  @media (max-width: 1000px) {
    .users-head { display: none; }
    .user-row {
      grid-template-columns: 1fr;
      gap: 4px;
    }
  }
</style>
```

- [ ] **Step 2: Run frontend check**

Run:

```bash
cd frontend
npm run check
```

Expected: PASS, or unrelated existing errors. This page must not introduce Svelte/TypeScript errors.

- [ ] **Step 3: Commit**

```bash
git add -- frontend/src/routes/users/+page.svelte
git commit -m "feat(frontend): list product users" -- frontend/src/routes/users/+page.svelte
```

## Task 4: Product User Profile Page

**Files:**

- Create: `frontend/src/routes/users/[distinct_id]/+page.svelte`

- [ ] **Step 1: Create the dedicated profile page**

Create `frontend/src/routes/users/[distinct_id]/+page.svelte`:

```svelte
<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import {
    fetchProductUser,
    fetchProductUserEvents,
    type ProductEvent,
    type ProductUserDetail
  } from '$lib/api';
  import {
    groupEventsBySession,
    propertiesPreview,
    shortProductId,
    timelineRows,
    type TimelineRow
  } from '$lib/product-users';
  import { formatTimestamp, rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import EventDetailDrawer from '$lib/components/EventDetailDrawer.svelte';
  import SkeletonCards from '$lib/components/SkeletonCards.svelte';
  import SkeletonLogRows from '$lib/components/SkeletonLogRows.svelte';

  let user: ProductUserDetail | null = null;
  let events: ProductEvent[] = [];
  let rows: TimelineRow[] = [];
  let loading = true;
  let error = '';
  let selected: ProductEvent | null = null;
  let source = '';

  $: distinctId = $page.params.distinct_id ? decodeURIComponent($page.params.distinct_id) : '';
  $: sessions = groupEventsBySession(events);
  $: rows = timelineRows(events);

  async function load(): Promise<void> {
    if (!distinctId) return;
    loading = true;
    error = '';
    try {
      const params = {
        project: $selectedProject || undefined,
        last_minutes: rangeMinutes($timeRange),
        limit: 500
      };
      const [detail, evs] = await Promise.all([
        fetchProductUser(distinctId, params),
        fetchProductUserEvents(distinctId, { ...params, source: source || undefined })
      ]);
      user = detail;
      events = evs;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      user = null;
      events = [];
    } finally {
      loading = false;
    }
  }

  function eventKey(ev: ProductEvent): string {
    return ev.event_id || `${ev.timestamp}:${ev.event_name}:${ev.session_id}`;
  }

  function traceHref(traceId: string): string {
    return `/traces/${encodeURIComponent(traceId)}`;
  }

  function eventsHref(): string {
    const p = new URLSearchParams();
    p.set('distinct_id', distinctId);
    if ($selectedProject) p.set('project', $selectedProject);
    if ($timeRange && $timeRange !== '1h') p.set('range', $timeRange);
    return `/events?${p.toString()}`;
  }

  function drawerPosition(): { index: number; total: number } | null {
    if (!selected) return null;
    const idx = events.findIndex((ev) => eventKey(ev) === eventKey(selected));
    return idx >= 0 ? { index: idx, total: events.length } : null;
  }

  let prevProject = $selectedProject;
  let prevRange = $timeRange;
  $: if (prevProject !== $selectedProject || prevRange !== $timeRange) {
    prevProject = $selectedProject;
    prevRange = $timeRange;
    void load();
  }

  onMount(load);
</script>

<div class="page-header">
  <div>
    <a href="/users" class="back-link">← Usuarios</a>
    <h1 class="page-title mono">{shortProductId(distinctId)}</h1>
  </div>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <select bind:value={source} on:change={load}>
      <option value="">Cualquier source</option>
      <option value="web">web</option>
      <option value="mobile">mobile</option>
      <option value="server">server</option>
      <option value="backend">backend</option>
    </select>
    <a class="button-link" href={eventsHref()}>Abrir en eventos</a>
    <button on:click={load} disabled={loading}>{loading ? 'Cargando...' : 'Recargar'}</button>
  </div>
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

{#if loading && !user}
  <SkeletonCards count={4} />
  <div class="timeline-shell"><SkeletonLogRows rows={12} /></div>
{:else if user}
  <section class="profile-grid">
    <div class="profile-card main-card">
      <div class="label">distinct_id</div>
      <div class="value mono" title={user.distinct_id}>{user.distinct_id}</div>
      <div class="meta-line">
        <span>First seen <strong class="mono">{formatTimestamp(user.first_seen)}</strong></span>
        <span>Last seen <strong class="mono">{formatTimestamp(user.last_seen)}</strong></span>
      </div>
      {#if propertiesPreview(user.properties)}
        <div class="props mono">{propertiesPreview(user.properties, 6)}</div>
      {/if}
    </div>
    <div class="profile-card">
      <div class="label">Eventos</div>
      <div class="stat mono">{user.event_count.toLocaleString()}</div>
      <div class="muted small">histórico del usuario</div>
    </div>
    <div class="profile-card">
      <div class="label">Sessions</div>
      <div class="stat mono">{sessions.length.toLocaleString()}</div>
      <div class="muted small">en este rango</div>
    </div>
    <div class="profile-card">
      <div class="label">Anonymous IDs</div>
      <div class="stat mono">{user.anonymous_ids.length}</div>
      <div class="anon-list mono">{user.anonymous_ids.slice(0, 4).join(', ')}</div>
    </div>
  </section>

  <section class="devices">
    <h2>Sources</h2>
    <div class="device-grid">
      {#each user.devices as d (d.source)}
        <div class="device">
          <div class="device-head">
            <strong class="mono">{d.source || 'unknown'}</strong>
            <span class="mono">{d.event_count.toLocaleString()} eventos</span>
          </div>
          <div class="muted small">Last seen {formatTimestamp(d.last_seen)}</div>
          <div class="muted small">{d.anonymous_id_count} anonymous IDs</div>
        </div>
      {/each}
    </div>
  </section>

  <section class="timeline-shell">
    <div class="timeline-head">
      <h2>Timeline</h2>
      <span class="muted">{events.length.toLocaleString()} eventos en el rango</span>
    </div>

    {#if events.length === 0}
      <div class="empty">No hay eventos para este usuario en el rango seleccionado.</div>
    {:else}
      <div class="timeline">
        {#each rows as row, i (`${row.kind}:${row.kind === 'session' ? row.session_id : eventKey(row.event)}:${i}`)}
          {#if row.kind === 'session'}
            <div class="session-row">
              <div class="session-dot"></div>
              <div class="session-body">
                <div class="session-title mono">session {row.session_id}</div>
                <div class="muted small">
                  {formatTimestamp(row.start_ts)} → {formatTimestamp(row.end_ts)}
                  · {row.event_count} eventos
                  · {row.trace_count} traces
                  {#if row.sources.length > 0} · {row.sources.join(', ')}{/if}
                </div>
              </div>
            </div>
          {:else}
            {@const ev = row.event}
            <button type="button" class="event-row" on:click={() => (selected = ev)}>
              <div class="event-time mono">{formatTimestamp(ev.timestamp)}</div>
              <div class="event-main">
                <div class="event-title">
                  <span class="mono">{ev.event_name}</span>
                  {#if ev.source}<span class="chip mono">{ev.source}</span>{/if}
                </div>
                {#if propertiesPreview(ev.properties)}
                  <div class="muted mono event-props">{propertiesPreview(ev.properties)}</div>
                {/if}
              </div>
              <div class="event-links">
                {#if ev.trace_id}
                  <a href={traceHref(ev.trace_id)} on:click|stopPropagation>Trace</a>
                {:else}
                  <span class="muted">—</span>
                {/if}
              </div>
            </button>
          {/if}
        {/each}
      </div>
    {/if}
  </section>
{/if}

<EventDetailDrawer
  event={selected}
  position={drawerPosition()}
  on:close={() => (selected = null)}
/>

<style>
  .back-link {
    display: inline-block;
    margin-bottom: 4px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .button-link {
    display: inline-flex;
    align-items: center;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 6px 12px;
    color: var(--text);
    text-decoration: none;
    background: var(--bg-elev);
  }
  .button-link:hover { background: var(--bg-hover); text-decoration: none; }

  .profile-grid {
    display: grid;
    grid-template-columns: minmax(280px, 2fr) repeat(3, minmax(150px, 1fr));
    gap: 12px;
    margin-bottom: 16px;
  }
  .profile-card {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 12px 14px;
    min-width: 0;
  }
  .main-card .value {
    font-size: 18px;
    overflow-wrap: anywhere;
  }
  .label {
    font-size: 11px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .stat {
    font-size: 24px;
    font-weight: 700;
    margin-top: 4px;
  }
  .meta-line {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    color: var(--text-muted);
    font-size: 12px;
    margin-top: 8px;
  }
  .props,
  .anon-list {
    color: var(--text-muted);
    font-size: 11.5px;
    margin-top: 8px;
    overflow-wrap: anywhere;
  }

  .devices,
  .timeline-shell {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-bottom: 16px;
    padding: 12px;
  }
  .devices h2,
  .timeline-head h2 {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.5px;
  }
  .device-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: 8px;
    margin-top: 10px;
  }
  .device {
    border: 1px solid var(--border);
    background: var(--bg);
    border-radius: 6px;
    padding: 10px;
  }
  .device-head {
    display: flex;
    justify-content: space-between;
    gap: 8px;
    font-size: 12.5px;
  }

  .timeline-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 10px;
  }
  .timeline {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .session-row,
  .event-row {
    display: grid;
    grid-template-columns: 170px 1fr 90px;
    gap: 12px;
    align-items: center;
  }
  .session-row {
    grid-template-columns: 24px 1fr;
    padding: 10px 8px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-top: 8px;
  }
  .session-dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: var(--accent);
    justify-self: center;
  }
  .session-body {
    display: flex;
    align-items: baseline;
    gap: 12px;
    flex-wrap: wrap;
  }
  .session-title { font-size: 12.5px; }

  .event-row {
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid transparent;
    border-bottom-color: var(--border);
    border-radius: 4px;
    padding: 7px 8px;
    cursor: pointer;
  }
  .event-row:hover {
    background: var(--bg-hover);
    border-color: var(--border);
  }
  .event-time {
    color: var(--text-muted);
    font-size: 11.5px;
  }
  .event-title {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .chip {
    border: 1px solid var(--border);
    background: var(--bg);
    border-radius: 10px;
    padding: 1px 7px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .event-props {
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .event-links {
    font-size: 12px;
    text-align: right;
  }
  .empty {
    color: var(--text-muted);
    text-align: center;
    padding: 30px 12px;
  }
  .error-box {
    color: var(--danger);
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
  }
  .small { font-size: 11.5px; }

  @media (max-width: 1000px) {
    .profile-grid { grid-template-columns: 1fr; }
    .session-row,
    .event-row {
      grid-template-columns: 1fr;
    }
    .event-links { text-align: left; }
  }
</style>
```

- [ ] **Step 2: Run frontend check**

Run:

```bash
cd frontend
npm run check
```

Expected: PASS, or unrelated existing errors. The profile page must not introduce new Svelte/TypeScript errors.

- [ ] **Step 3: Commit**

```bash
git add -- frontend/src/routes/users/[distinct_id]/+page.svelte
git commit -m "feat(frontend): add product user profile" -- frontend/src/routes/users/[distinct_id]/+page.svelte
```

## Task 5: Navigation and Command Palette

**Files:**

- Modify: `frontend/src/lib/components/Sidebar.svelte`
- Modify: `frontend/src/lib/palette.ts`
- Modify: `frontend/src/lib/palette.test.ts`

- [ ] **Step 1: Write the failing palette test**

In `frontend/src/lib/palette.test.ts`, update the `staticCommands` smoke test:

```ts
expect(ids).toContain('nav.product-users');
expect(ids).toContain('nav.settings-users');
```

- [ ] **Step 2: Run the palette test to verify it fails**

Run:

```bash
cd frontend
npm test -- src/lib/palette.test.ts
```

Expected: FAIL because the new command ids are not present yet.

- [ ] **Step 3: Update sidebar navigation**

In `frontend/src/lib/components/Sidebar.svelte`, add product users after Eventos:

```ts
{ href: '/events', label: 'Eventos', icon: '◆' },
{ href: '/users', label: 'Usuarios', icon: '◌' },
{ href: '/funnels', label: 'Funnels', icon: '▽' },
```

- [ ] **Step 4: Update static commands**

In `frontend/src/lib/palette.ts`, replace the existing `nav.users` command with two distinct commands:

```ts
{ id: 'nav.product-users', group: 'Navegar', icon: '◌', label: 'Ir a Usuarios de producto', shortcut: 'g u', run: () => goto('/users') },
{ id: 'nav.settings-users', group: 'Navegar', icon: '👤', label: 'Ir a Usuarios del dashboard', run: () => goto('/settings/users') },
```

- [ ] **Step 5: Run tests to verify they pass**

Run:

```bash
cd frontend
npm test -- src/lib/palette.test.ts
```

Expected: PASS.

- [ ] **Step 6: Run frontend check**

Run:

```bash
cd frontend
npm run check
```

Expected: PASS, or unrelated existing errors.

- [ ] **Step 7: Commit**

```bash
git add -- frontend/src/lib/components/Sidebar.svelte frontend/src/lib/palette.ts frontend/src/lib/palette.test.ts
git commit -m "feat(frontend): expose product users navigation" -- frontend/src/lib/components/Sidebar.svelte frontend/src/lib/palette.ts frontend/src/lib/palette.test.ts
```

## Task 6: Final Verification

**Files:**

- Verify the files changed by Tasks 1-5.

- [ ] **Step 1: Run all frontend unit tests**

Run:

```bash
cd frontend
npm test
```

Expected: PASS.

- [ ] **Step 2: Run Svelte/TypeScript check**

Run:

```bash
cd frontend
npm run check
```

Expected: PASS.

- [ ] **Step 3: Run frontend build**

Run:

```bash
cd frontend
npm run build
```

Expected: PASS.

- [ ] **Step 4: Inspect git diff**

Run:

```bash
git diff --stat HEAD
git diff -- frontend/src/lib/api.ts frontend/src/lib/product-users.ts frontend/src/lib/product-users.test.ts frontend/src/routes/users/+page.svelte frontend/src/routes/users/[distinct_id]/+page.svelte frontend/src/lib/components/Sidebar.svelte frontend/src/lib/palette.ts frontend/src/lib/palette.test.ts
```

Expected: only the product-users implementation files are present in the diff.

- [ ] **Step 5: Commit any remaining verified changes**

If any tracked task files remain uncommitted, commit only those files:

```bash
git add -- frontend/src/lib/api.ts frontend/src/lib/product-users.ts frontend/src/lib/product-users.test.ts frontend/src/routes/users/+page.svelte frontend/src/routes/users/[distinct_id]/+page.svelte frontend/src/lib/components/Sidebar.svelte frontend/src/lib/palette.ts frontend/src/lib/palette.test.ts
git commit -m "feat(frontend): add product user profiles"
```

## Self-Review

- Spec coverage:
  - `/users` product list is covered by Task 3.
  - `/users/[distinct_id]` profile route is covered by Task 4.
  - Existing backend endpoints are consumed by Task 2.
  - Session grouping and timeline are covered by Task 1 and Task 4.
  - Trace links are covered by Task 4.
  - Navigation distinction from dashboard users is covered by Task 5.
- Placeholder scan:
  - No placeholder markers or vague deferred-work steps remain.
- Type consistency:
  - `ProductUserSummary`, `ProductUserDetail`, and `ProductEvent` names match imports used in Svelte pages.
  - `buildProductUserHref`, `groupEventsBySession`, `timelineRows`, `propertiesPreview`, and `shortProductId` names match test and page usage.
