<script lang="ts">
  /**
   * Página `/insights` — hallazgos combinados por servicio.
   *
   * Para un servicio/span y un par de eventos de funnel, pide a
   * `fetchServiceDashboardInsight` un resumen que cruza conversión, errores
   * linkeados y latencia p95, y lo presenta con los helpers de `$lib/insights`
   * (severidad ok/warn/danger, conteos, porcentajes) y enlaces directos a
   * errores, eventos o trazas.
   */
  import { onMount } from 'svelte';

  import {
    fetchServiceDashboardInsight,
    type ServiceDashboardInsight
  } from '$lib/api';
  import {
    errorIssueHref,
    eventsHref,
    formatInsightCount,
    formatInsightLatency,
    formatInsightPercent,
    insightSeverity,
    summarizeCombinedInsight,
    tracesHref
  } from '$lib/insights';
  import { rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import SkeletonCards from '$lib/components/SkeletonCards.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';

  let insight: ServiceDashboardInsight | null = null;
  let loading = true;
  let error = '';

  let service = 'checkout';
  let spanName = '/api/checkout';
  let funnelFrom = 'checkout_started';
  let funnelTo = 'checkout_completed';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      insight = await fetchServiceDashboardInsight({
        project: $selectedProject || undefined,
        last_minutes: rangeMinutes($timeRange),
        service: service.trim(),
        span_name: spanName.trim(),
        funnel_from: funnelFrom.trim(),
        funnel_to: funnelTo.trim(),
        limit: 8
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      insight = null;
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

  $: severity = insight ? insightSeverity(insight) : 'ok';
  $: summary = insight ? summarizeCombinedInsight(insight) : '';

  onMount(load);
</script>

<div class="page-header">
  <div>
    <h1 class="page-title">Insights</h1>
    <div class="muted subtitle">Eventos, errores, trazas y latencia en una sola lectura.</div>
  </div>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button on:click={load} disabled={loading}>{loading ? 'Calculando...' : 'Actualizar'}</button>
  </div>
</div>

<div class="lens">
  <div class="field compact">
    <label for="insight-service">Servicio</label>
    <input id="insight-service" class="mono" bind:value={service} on:keydown={(e) => e.key === 'Enter' && load()} />
  </div>
  <div class="field compact">
    <label for="insight-span">Span</label>
    <input id="insight-span" class="mono" bind:value={spanName} on:keydown={(e) => e.key === 'Enter' && load()} />
  </div>
  <div class="field compact">
    <label for="insight-from">Evento inicio</label>
    <input id="insight-from" class="mono" bind:value={funnelFrom} on:keydown={(e) => e.key === 'Enter' && load()} />
  </div>
  <div class="field compact">
    <label for="insight-to">Evento éxito</label>
    <input id="insight-to" class="mono" bind:value={funnelTo} on:keydown={(e) => e.key === 'Enter' && load()} />
  </div>
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

{#if loading && !insight}
  <SkeletonCards count={4} />
{:else if insight}
  <section class="hero" class:warn={severity === 'warn'} class:danger={severity === 'danger'}>
    <div>
      <div class="eyebrow mono">{insight.service_name} · {$timeRange}</div>
      <h2>{summary}</h2>
    </div>
    <div class="hero-actions">
      <a href={eventsHref(insight.funnel_from, $selectedProject || undefined, $timeRange)}>Eventos</a>
      <a href="/errors">Errores</a>
      <a href={tracesHref(insight.service_name, $timeRange)}>Trazas</a>
      <a href="/sessions">Sesiones</a>
    </div>
  </section>

  <div class="cards">
    <div class="card">
      <div class="label">Events</div>
      <div class="value mono">{formatInsightCount(insight.started_events)}</div>
      <div class="card-sub">
        <a href={eventsHref(insight.funnel_from, $selectedProject || undefined, $timeRange)}>{insight.funnel_from}</a>
      </div>
      <div class="metric-line mono">
        {formatInsightCount(insight.completed_events)} completados · {formatInsightPercent(insight.conversion_rate)}
      </div>
    </div>
    <div class="card">
      <div class="label">Errors linkeados</div>
      <div class="value mono" class:danger={insight.linked_error_sessions > 0}>{formatInsightCount(insight.linked_error_count)}</div>
      <div class="card-sub mono">
        {formatInsightCount(insight.linked_error_sessions)} de {formatInsightCount(insight.failed_sessions)} sesiones fallidas
      </div>
      <div class="metric-line">Join por <span class="mono">session_id</span></div>
    </div>
    <div class="card">
      <div class="label">p95 latency</div>
      <div class="value mono">{formatInsightLatency(insight.p95_latency_ms)}</div>
      <div class="card-sub mono">{insight.span_name}</div>
      <div class="metric-line mono">{formatInsightCount(insight.span_count)} spans</div>
    </div>
    <div class="card">
      <div class="label">Sesiones</div>
      <div class="value mono">{formatInsightCount(insight.started_sessions)}</div>
      <div class="card-sub mono">
        {formatInsightCount(insight.completed_sessions)} con éxito · {formatInsightCount(insight.failed_sessions)} fallidas
      </div>
    </div>
  </div>

  <section class="panel">
    <div class="panel-head">
      <div>
        <h2>Issues que rompen el journey</h2>
        <div class="muted">Errores capturados en sesiones que iniciaron el funnel y no lo completaron.</div>
      </div>
    </div>

    {#if insight.top_errors.length > 0}
      <div class="issue-table">
        <div class="issue-head">
          <div>Issue</div>
          <div>Servicio</div>
          <div>Errores</div>
          <div>Sesiones fallidas</div>
          <div>Último</div>
        </div>
        {#each insight.top_errors as issue (issue.fingerprint)}
          <a class="issue-row" href={errorIssueHref(issue.fingerprint)}>
            <div>
              <div class="issue-title">{issue.exception_type || 'Error'}</div>
              <div class="muted truncate">{issue.message || issue.fingerprint}</div>
            </div>
            <div class="mono muted">{issue.service_name}</div>
            <div class="mono">{formatInsightCount(issue.error_count)}</div>
            <div class="mono danger">{formatInsightCount(issue.affected_failed_sessions)}</div>
            <div class="mono muted">{issue.last_seen}</div>
          </a>
        {/each}
      </div>
    {:else}
      <OnboardingEmpty kind="errors" filteredOut={insight.failed_sessions > 0} />
    {/if}
  </section>
{:else}
  <OnboardingEmpty kind="events" filteredOut={false} />
{/if}

<style>
  .subtitle { font-size: 12px; margin-top: 2px; }
  .danger { color: var(--danger); }
  .lens {
    display: grid;
    grid-template-columns: repeat(4, minmax(170px, 1fr));
    gap: 10px;
    margin-bottom: 14px;
  }
  .compact {
    margin: 0;
  }
  .compact label {
    display: block;
    font-size: 11px;
    color: var(--text-muted);
    margin-bottom: 4px;
    text-transform: uppercase;
  }
  .compact input {
    width: 100%;
  }
  .hero {
    display: flex;
    justify-content: space-between;
    gap: 18px;
    align-items: flex-start;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-left: 4px solid var(--success);
    border-radius: 6px;
    padding: 16px;
    margin-bottom: 14px;
  }
  .hero.warn { border-left-color: var(--warning); }
  .hero.danger { border-left-color: var(--danger); }
  .hero h2 {
    margin: 4px 0 0;
    font-size: 18px;
    line-height: 1.35;
    font-weight: 600;
  }
  .eyebrow {
    color: var(--text-muted);
    font-size: 11px;
    text-transform: uppercase;
  }
  .hero-actions {
    display: flex;
    gap: 8px;
    flex-wrap: wrap;
    justify-content: flex-end;
  }
  .hero-actions a {
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 9px;
    color: var(--text);
    text-decoration: none;
    font-size: 12px;
  }
  .hero-actions a:hover { background: var(--bg-hover); }
  .card-sub {
    font-size: 11.5px;
    margin-top: 4px;
    color: var(--text-muted);
  }
  .card-sub a { color: var(--accent); text-decoration: none; }
  .card-sub a:hover { text-decoration: underline; }
  .metric-line {
    margin-top: 10px;
    font-size: 12px;
    color: var(--text-muted);
  }
  .panel {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .panel-head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 16px;
    border-bottom: 1px solid var(--border);
  }
  .panel-head h2 {
    margin: 0 0 4px;
    font-size: 15px;
  }
  .issue-head,
  .issue-row {
    display: grid;
    grid-template-columns: minmax(240px, 1.4fr) minmax(120px, 0.7fr) 90px 130px 180px;
    gap: 12px;
    align-items: center;
  }
  .issue-head {
    padding: 8px 16px;
    background: var(--bg);
    color: var(--text-muted);
    font-size: 12px;
  }
  .issue-row {
    min-height: 58px;
    padding: 10px 16px;
    border-top: 1px solid var(--border);
    color: var(--text);
    text-decoration: none;
    font-size: 12.5px;
  }
  .issue-row:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }
  .issue-title {
    font-weight: 600;
    margin-bottom: 3px;
  }
  .truncate {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .error-box {
    color: var(--danger);
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
  }

  @media (max-width: 1050px) {
    .lens { grid-template-columns: repeat(2, minmax(170px, 1fr)); }
    .hero { flex-direction: column; }
    .hero-actions { justify-content: flex-start; }
    .panel { overflow-x: auto; }
    .issue-head,
    .issue-row { min-width: 860px; }
  }
</style>
