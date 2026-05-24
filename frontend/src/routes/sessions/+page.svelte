<script lang="ts">
  import { onMount } from 'svelte';

  import { fetchProductSessions, type ProductSessionSummary } from '$lib/api';
  import {
    formatSessionDuration,
    sessionEventsHref,
    sessionHealth,
    sessionReplayHref,
    sessionUserHref
  } from '$lib/sessions';
  import { formatTimestamp, rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import SkeletonLogRows from '$lib/components/SkeletonLogRows.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';

  let sessions: ProductSessionSummary[] = [];
  let loading = true;
  let error = '';
  let query = '';
  let replayOnly = false;
  let errorOnly = false;

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      sessions = await fetchProductSessions({
        project: $selectedProject || undefined,
        last_minutes: rangeMinutes($timeRange),
        has_replay: replayOnly ? 1 : undefined,
        has_error: errorOnly ? 1 : undefined,
        limit: 500
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      sessions = [];
    } finally {
      loading = false;
    }
  }

  let prevProject = $selectedProject;
  let prevRange = $timeRange;
  $: if (prevProject !== $selectedProject || prevRange !== $timeRange) {
    prevProject = $selectedProject;
    prevRange = $timeRange;
    void load();
  }

  $: filtered = (() => {
    const q = query.trim().toLowerCase();
    if (!q) return sessions;
    return sessions.filter((s) =>
      s.session_id.toLowerCase().includes(q) ||
      s.distinct_id.toLowerCase().includes(q) ||
      s.source.toLowerCase().includes(q)
    );
  })();

  $: replayCount = filtered.filter((s) => s.has_replay === 1).length;
  $: errorCount = filtered.filter((s) => s.has_error === 1 || s.error_count > 0).length;
  $: pageviewCount = filtered.reduce((sum, s) => sum + s.pageview_count, 0);
  $: totalDuration = filtered.reduce((sum, s) => sum + s.duration_seconds, 0);

  onMount(load);

  function shortId(id: string): string {
    if (!id) return '-';
    return id.length > 28 ? `${id.slice(0, 12)}...${id.slice(-8)}` : id;
  }

  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return n.toLocaleString();
  }
</script>

<div class="page-header">
  <div>
    <h1 class="page-title">Sessions</h1>
    <div class="muted subtitle">Sesiones recientes de usuarios finales con replay y errores.</div>
  </div>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button on:click={load} disabled={loading}>{loading ? 'Cargando...' : 'Recargar'}</button>
  </div>
</div>

<div class="cards">
  <div class="card">
    <div class="label">Sesiones</div>
    <div class="value mono">{fmtCount(filtered.length)}</div>
  </div>
  <div class="card">
    <div class="label">Con replay</div>
    <div class="value mono">{fmtCount(replayCount)}</div>
  </div>
  <div class="card">
    <div class="label">Con error</div>
    <div class="value mono" class:danger={errorCount > 0}>{fmtCount(errorCount)}</div>
  </div>
  <div class="card">
    <div class="label">Pageviews</div>
    <div class="value mono">{fmtCount(pageviewCount)}</div>
    <div class="muted card-sub mono">{formatSessionDuration(totalDuration)} acumulado</div>
  </div>
</div>

<div class="toolbar">
  <input
    placeholder="Buscar session_id, distinct_id o source..."
    bind:value={query}
    style="min-width: 300px;"
    data-search-input
  />
  <label class="toggle">
    <input type="checkbox" bind:checked={replayOnly} on:change={load} />
    <span>Solo con replay</span>
  </label>
  <label class="toggle">
    <input type="checkbox" bind:checked={errorOnly} on:change={load} />
    <span>Solo con error</span>
  </label>
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

<div class="sessions-table">
  <div class="sessions-head">
    <div>Sesión</div>
    <div>Usuario</div>
    <div>Inicio</div>
    <div>Duración</div>
    <div>Pageviews</div>
    <div>Eventos</div>
    <div>Errores</div>
    <div>Replay</div>
  </div>

  {#if loading && sessions.length === 0}
    <SkeletonLogRows rows={10} />
  {:else}
    {#each filtered as row (row.project_id + ':' + row.session_id)}
      {@const replayHref = sessionReplayHref(row)}
      {@const health = sessionHealth(row)}
      {@const userHref = sessionUserHref(row, $selectedProject || undefined, $timeRange)}
      {@const eventsHref = sessionEventsHref(row, $selectedProject || undefined, $timeRange)}
      {#if replayHref}
        <a class="session-row" class:error={health === 'error'} href={replayHref} data-sveltekit-preload-data="hover">
          <div class="session-cell">
            <span class="status-dot" class:error={health === 'error'} class:replay={health === 'replay'}></span>
            <span class="mono sid" title={row.session_id}>{shortId(row.session_id)}</span>
            <span class="chip mono">{row.source}</span>
          </div>
          <div class="mono muted" title={row.distinct_id}>{shortId(row.distinct_id)}</div>
          <div class="mono muted">{formatTimestamp(row.started_at)}</div>
          <div class="mono">{formatSessionDuration(row.duration_seconds)}</div>
          <div class="mono tabular">{row.pageview_count.toLocaleString()}</div>
          <div class="mono tabular">{row.event_count.toLocaleString()}</div>
          <div class="mono tabular" class:danger={row.error_count > 0}>{row.error_count.toLocaleString()}</div>
          <div class="replay-cell">
            <span class="play">▶ Reproducir</span>
            <span class="muted mono">{row.replay_chunk_count} chunks</span>
          </div>
        </a>
      {:else}
        <div class="session-row muted-row" class:error={health === 'error'}>
          <div class="session-cell">
            <span class="status-dot" class:error={health === 'error'}></span>
            <span class="mono sid" title={row.session_id}>{shortId(row.session_id)}</span>
            <span class="chip mono">{row.source}</span>
          </div>
          <div>
            {#if userHref}
              <a class="inline-link mono" href={userHref}>{shortId(row.distinct_id)}</a>
            {:else}
              <span class="mono muted">-</span>
            {/if}
          </div>
          <div class="mono muted">{formatTimestamp(row.started_at)}</div>
          <div class="mono">{formatSessionDuration(row.duration_seconds)}</div>
          <div class="mono tabular">{row.pageview_count.toLocaleString()}</div>
          <div class="mono tabular">{row.event_count.toLocaleString()}</div>
          <div class="mono tabular" class:danger={row.error_count > 0}>{row.error_count.toLocaleString()}</div>
          <div class="replay-cell">
            <span class="muted">Sin replay</span>
            <a class="inline-link mono" href={eventsHref}>eventos</a>
          </div>
        </div>
      {/if}
    {/each}
  {/if}
</div>

{#if !loading && filtered.length === 0}
  <OnboardingEmpty kind="events" filteredOut={!!(query || replayOnly || errorOnly)} />
{/if}

<style>
  .subtitle { font-size: 12px; margin-top: 2px; }
  .card-sub { font-size: 11px; margin-top: 4px; }
  .danger { color: var(--danger); }
  .toggle {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .toggle input { accent-color: var(--accent); }

  .sessions-table {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .sessions-head,
  .session-row {
    display: grid;
    grid-template-columns: minmax(190px, 1.25fr) minmax(150px, 1fr) 190px 90px 90px 80px 80px minmax(150px, 0.9fr);
    gap: 12px;
    align-items: center;
  }
  .sessions-head {
    padding: 8px 12px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
  }
  .session-row {
    min-height: 52px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    text-decoration: none;
    font-size: 12.5px;
  }
  .session-row:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }
  .session-row.error {
    box-shadow: inset 3px 0 0 var(--danger);
  }
  .session-row:last-child { border-bottom: 0; }
  .muted-row { color: var(--text); }
  .session-cell {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .status-dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--debug);
    flex: 0 0 auto;
  }
  .status-dot.replay { background: var(--success); }
  .status-dot.error {
    background: var(--danger);
    box-shadow: 0 0 8px rgba(239, 68, 68, 0.45);
  }
  .sid {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chip {
    border: 1px solid var(--border);
    background: var(--bg);
    border-radius: 10px;
    padding: 1px 7px;
    font-size: 11px;
    color: var(--text-muted);
    flex: 0 0 auto;
  }
  .replay-cell {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .play {
    color: var(--accent);
    font-weight: 600;
  }
  .inline-link {
    color: var(--accent);
    text-decoration: none;
  }
  .inline-link:hover { text-decoration: underline; }
  .error-box {
    color: var(--danger);
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
  }

  @media (max-width: 1100px) {
    .sessions-table { overflow-x: auto; }
    .sessions-head,
    .session-row {
      min-width: 1020px;
    }
  }
</style>
