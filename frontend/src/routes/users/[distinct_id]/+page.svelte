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

  $: distinctId = $page.params.distinct_id ?? '';
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

  function rowKey(row: TimelineRow, index: number): string {
    if (row.kind === 'session') return `session:${row.session_id}:${index}`;
    return `event:${eventKey(row.event)}:${index}`;
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
    const current = selected;
    const idx = events.findIndex((ev) => eventKey(ev) === eventKey(current));
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
        {#each rows as row, i (rowKey(row, i))}
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
            <div
              class="event-row"
              role="button"
              tabindex="0"
              on:click={() => (selected = ev)}
              on:keydown={(e) => e.key === 'Enter' && (selected = ev)}
            >
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
            </div>
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
  .event-row:focus {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
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
