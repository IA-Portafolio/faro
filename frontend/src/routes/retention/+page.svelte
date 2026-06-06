<script lang="ts">
  /**
   * Página `/retention` — retención de usuarios por cohortes (heatmap).
   *
   * Elegido un evento "ancla", pide `fetchRetention` y pinta el heatmap: cada fila
   * es una cohorte (usuarios vistos por primera vez un día) y cada columna su
   * retención a D1/D7/D30. El cálculo (tasa ponderada, cohortes maduras, color)
   * vive en `$lib/retention`.
   */
  import { onMount } from 'svelte';

  import {
    fetchFunnelEvents,
    fetchRetention,
    type EventCandidate,
    type RetentionCohort,
    type RetentionResult
  } from '$lib/api';
  import {
    formatRetentionPct,
    heatColor,
    isMature,
    retentionRate,
    weightedRetention,
    type RetentionDay
  } from '$lib/retention';
  import { rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';

  const days: RetentionDay[] = [1, 7, 30];

  let catalog: EventCandidate[] = [];
  let result: RetentionResult | null = null;
  let eventName = '';
  let loading = true;
  let catalogLoading = true;
  let error = '';
  let reqSeq = 0;
  let mounted = false;

  async function loadCatalog(): Promise<void> {
    catalogLoading = true;
    try {
      catalog = await fetchFunnelEvents({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
    } catch (_e) {
      catalog = [];
    } finally {
      catalogLoading = false;
    }
  }

  async function loadRetention(): Promise<void> {
    const seq = ++reqSeq;
    loading = true;
    error = '';
    try {
      const data = await fetchRetention({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined,
        event_name: eventName || undefined,
        interval: 'day'
      });
      if (seq !== reqSeq) return;
      result = data;
    } catch (e: unknown) {
      if (seq !== reqSeq) return;
      error = e instanceof Error ? e.message : String(e);
      result = null;
    } finally {
      if (seq === reqSeq) loading = false;
    }
  }

  async function reloadAll(): Promise<void> {
    await Promise.all([loadCatalog(), loadRetention()]);
  }

  let prevProject = $selectedProject;
  let prevRange = $timeRange;
  $: if (mounted && (prevProject !== $selectedProject || prevRange !== $timeRange)) {
    prevProject = $selectedProject;
    prevRange = $timeRange;
    void reloadAll();
  }

  $: if (mounted) {
    void eventName;
    void loadRetention();
  }

  onMount(async () => {
    await reloadAll();
    mounted = true;
  });

  $: cohorts = result?.cohorts ?? [];
  $: asOf = result ? new Date(result.to) : new Date();
  $: totalUsers = cohorts.reduce((sum, row) => sum + row.cohort_size, 0);
  $: weightedByDay = new Map(days.map((day) => [day, weightedRetention(cohorts, day, asOf)]));

  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return n.toLocaleString();
  }

  function fmtDate(s: string): string {
    const d = new Date(`${s}T00:00:00Z`);
    if (Number.isNaN(d.getTime())) return s;
    return d.toLocaleDateString(undefined, { month: 'short', day: '2-digit' });
  }

  function usersFor(row: RetentionCohort, day: RetentionDay): number {
    if (day === 1) return row.d1_users;
    if (day === 7) return row.d7_users;
    return row.d30_users;
  }
</script>

<div class="page-header">
  <div>
    <h1 class="page-title">Retention</h1>
    <div class="muted subtitle">Cohortes por primera actividad de usuario.</div>
  </div>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button on:click={reloadAll} disabled={loading}>{loading ? 'Cargando...' : 'Recargar'}</button>
  </div>
</div>

<div class="toolbar">
  <label class="filter">
    <span class="muted">Evento de retorno</span>
    <select bind:value={eventName} disabled={catalogLoading}>
      <option value="">Cualquier evento</option>
      {#each catalog as ev (ev.name)}
        <option value={ev.name}>{ev.name}</option>
      {/each}
    </select>
  </label>
  {#if result}
    <span class="muted mono took">{result.took_ms} ms</span>
  {/if}
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

<div class="cards">
  <div class="card">
    <div class="label">Usuarios cohort</div>
    <div class="value mono">{fmtCount(totalUsers)}</div>
  </div>
  {#each days as day}
    {@const w = weightedByDay.get(day)}
    <div class="card">
      <div class="label">D{day}</div>
      <div class="value mono">{formatRetentionPct(w?.rate ?? 0)}</div>
      <div class="muted card-sub mono">{fmtCount(w?.users ?? 0)} / {fmtCount(w?.cohortSize ?? 0)}</div>
    </div>
  {/each}
</div>

<section class="heatmap">
  <div class="heat-head">
    <div>Cohort</div>
    <div class="right">Usuarios</div>
    {#each days as day}
      <div class="centered">D{day}</div>
    {/each}
  </div>

  {#if loading && cohorts.length === 0}
    <div class="skel">
      {#each Array(8) as _}
        <Skeleton width="100%" height="42px" radius="0" />
      {/each}
    </div>
  {:else}
    {#each cohorts as row (row.cohort_date)}
      <div class="heat-row">
        <div class="cohort-date">
          <span class="mono">{fmtDate(row.cohort_date)}</span>
          <span class="muted mono full-date">{row.cohort_date}</span>
        </div>
        <div class="mono right">{fmtCount(row.cohort_size)}</div>
        {#each days as day}
          {@const mature = isMature(row.cohort_date, day, asOf)}
          {@const rate = retentionRate(row, day)}
          {@const users = usersFor(row, day)}
          <div
            class:mature
            class:pending={!mature}
            class="cell"
            style:background={heatColor(rate, mature)}
            title={mature
              ? `D${day}: ${formatRetentionPct(rate)} (${fmtCount(users)} de ${fmtCount(row.cohort_size)})`
              : `D${day} aún no maduró`}
          >
            {#if mature}
              <span class="mono pct">{formatRetentionPct(rate)}</span>
              <span class="mono users">{fmtCount(users)}</span>
            {:else}
              <span class="muted">-</span>
            {/if}
          </div>
        {/each}
      </div>
    {/each}
  {/if}
</section>

{#if !loading && cohorts.length === 0}
  <OnboardingEmpty kind="events" filteredOut={!!eventName} />
{/if}

<style>
  .subtitle { font-size: 12px; margin-top: 2px; }
  .filter {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .filter select { min-width: 240px; max-width: min(420px, 70vw); }
  .took { font-size: 11px; }
  .card-sub { font-size: 11px; margin-top: 4px; }

  .heatmap {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .heat-head,
  .heat-row {
    display: grid;
    grid-template-columns: minmax(150px, 1.2fr) 110px repeat(3, minmax(110px, 1fr));
    align-items: stretch;
  }
  .heat-head {
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
  }
  .heat-head > div {
    padding: 8px 10px;
    border-right: 1px solid var(--border);
  }
  .heat-head > div:last-child { border-right: 0; }
  .heat-row {
    min-height: 48px;
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
  }
  .heat-row:last-child { border-bottom: 0; }
  .heat-row > div {
    padding: 8px 10px;
    border-right: 1px solid var(--border);
  }
  .heat-row > div:last-child { border-right: 0; }
  .heat-row:hover { background: var(--bg-hover); }

  .cohort-date {
    display: flex;
    flex-direction: column;
    justify-content: center;
    min-width: 0;
  }
  .full-date {
    font-size: 10.5px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .right { text-align: right; }
  .centered { text-align: center; }
  .cell {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    min-height: 48px;
  }
  .cell.mature {
    color: #ffffff;
    text-shadow: 0 1px 1px rgba(0, 0, 0, 0.28);
  }
  :global([data-theme="light"]) .cell.mature {
    color: #052e16;
    text-shadow: none;
  }
  .cell.pending {
    background: var(--bg);
    color: var(--text-muted);
  }
  .pct { font-weight: 700; }
  .users { opacity: 0.82; font-size: 11px; }
  .skel {
    display: flex;
    flex-direction: column;
  }
  .error-box {
    color: var(--danger);
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
  }

  @media (max-width: 820px) {
    .heatmap { overflow-x: auto; }
    .heat-head,
    .heat-row {
      min-width: 620px;
    }
    .filter {
      align-items: flex-start;
      flex-direction: column;
    }
    .filter select { width: 100%; }
  }
</style>
