<script lang="ts">
  /**
   * Página `/experiments` — análisis de experimentos A/B sobre feature flags.
   *
   * Dado un `flagKey` (la feature flag que reparte variantes) y un evento de
   * conversión, llama a `analyzeExperiment` y muestra el resultado por variante
   * (conversión, uplift y significancia). El catálogo de eventos candidatos se
   * precarga con `fetchFunnelEvents`.
   */
  import { onMount } from 'svelte';

  import {
    analyzeExperiment,
    fetchFunnelEvents,
    type EventCandidate,
    type ExperimentAnalyzeResult,
    type ExperimentVariantResult
  } from '$lib/api';
  import { rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';

  let flagKey = 'new-checkout';
  let conversionEvent = 'checkout_completed';
  let result: ExperimentAnalyzeResult | null = null;
  let loading = false;
  let error = '';
  let catalog: EventCandidate[] = [];

  async function loadCatalog(): Promise<void> {
    try {
      catalog = await fetchFunnelEvents({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
    } catch {
      catalog = [];
    }
  }

  async function runAnalysis(): Promise<void> {
    const flag = flagKey.trim();
    const conversion = conversionEvent.trim();
    if (!flag || !conversion) return;
    loading = true;
    error = '';
    try {
      result = await analyzeExperiment({
        flag_key: flag,
        conversion_event: conversion,
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      result = null;
    } finally {
      loading = false;
    }
  }

  onMount(async () => {
    await loadCatalog();
    await runAnalysis();
  });

  let prevRange = $timeRange;
  let prevProject = $selectedProject;
  $: {
    if (prevRange !== $timeRange || prevProject !== $selectedProject) {
      prevRange = $timeRange;
      prevProject = $selectedProject;
      void loadCatalog();
      void runAnalysis();
    }
  }

  function variant(label: string): ExperimentVariantResult {
    return result?.variants.find((v) => v.variant === label) ?? {
      variant: label,
      sample: 0,
      conversions: 0,
      conversion_rate: 0
    };
  }

  function fmtPct(v: number, digits = 1): string {
    if (!Number.isFinite(v)) return '0.0%';
    return `${(v * 100).toFixed(digits)}%`;
  }

  function fmtP(v: number): string {
    if (!Number.isFinite(v)) return '1.000';
    if (v < 0.001) return '<0.001';
    return v.toFixed(3);
  }

  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return n.toLocaleString();
  }

  function sentence(r: ExperimentAnalyzeResult): string {
    const better = r.relative_lift >= 0 ? 'mejor' : 'peor';
    return `Variante B convierte ${fmtPct(Math.abs(r.relative_lift))} ${better} (p=${fmtP(r.p_value)}, sample=${fmtCount(r.sample)}, 95% CI: ${fmtPct(r.ci95_low)} - ${fmtPct(r.ci95_high)})`;
  }
</script>

<div class="page-header">
  <h1 class="page-title">Experimentos</h1>
  <TimeRangePicker />
</div>

<section class="toolbar experiment-form" aria-label="Analizar experimento">
  <label>
    <span>Flag</span>
    <input class="mono" bind:value={flagKey} placeholder="new-checkout" on:keydown={(e) => e.key === 'Enter' && runAnalysis()} />
  </label>
  <label>
    <span>Conversión</span>
    <input
      class="mono"
      bind:value={conversionEvent}
      list="event-catalog"
      placeholder="checkout_completed"
      on:keydown={(e) => e.key === 'Enter' && runAnalysis()}
    />
    <datalist id="event-catalog">
      {#each catalog as ev}
        <option value={ev.name}>{ev.count}</option>
      {/each}
    </datalist>
  </label>
  <button class="primary" on:click={runAnalysis} disabled={loading || !flagKey.trim() || !conversionEvent.trim()}>
    {loading ? 'Calculando...' : 'Analizar'}
  </button>
</section>

{#if error}
  <div class="error">Error: {error}</div>
{/if}

{#if loading && !result}
  <div class="layout">
    <Skeleton width="100%" height="180px" radius="8px" />
    <Skeleton width="100%" height="180px" radius="8px" />
  </div>
{:else if result}
  {@const a = variant('A')}
  {@const b = variant('B')}
  <section class="summary-band">
    <div>
      <div class="eyebrow mono">{result.flag_key} -> {result.conversion_event}</div>
      <h2>{sentence(result)}</h2>
    </div>
    <div class="sig" class:good={result.p_value < 0.05}>
      <span class="sig-value mono">{fmtP(result.p_value)}</span>
      <span class="muted">p-value</span>
    </div>
  </section>

  <div class="layout">
    <section class="panel">
      <div class="panel-head">
        <h2>Variante A</h2>
        <span class="badge">Control</span>
      </div>
      <div class="rate mono">{fmtPct(a.conversion_rate)}</div>
      <div class="metric-grid">
        <div>
          <span class="muted">Sample</span>
          <strong class="mono">{fmtCount(a.sample)}</strong>
        </div>
        <div>
          <span class="muted">Conversiones</span>
          <strong class="mono">{fmtCount(a.conversions)}</strong>
        </div>
      </div>
      <div class="bar"><span style={`width: ${Math.min(100, a.conversion_rate * 100)}%`}></span></div>
    </section>

    <section class="panel treatment">
      <div class="panel-head">
        <h2>Variante B</h2>
        <span class="badge">Treatment</span>
      </div>
      <div class="rate mono">{fmtPct(b.conversion_rate)}</div>
      <div class="metric-grid">
        <div>
          <span class="muted">Sample</span>
          <strong class="mono">{fmtCount(b.sample)}</strong>
        </div>
        <div>
          <span class="muted">Conversiones</span>
          <strong class="mono">{fmtCount(b.conversions)}</strong>
        </div>
      </div>
      <div class="bar"><span style={`width: ${Math.min(100, b.conversion_rate * 100)}%`}></span></div>
    </section>
  </div>

  <section class="stats-strip">
    <div>
      <span class="muted">Delta absoluto</span>
      <strong class="mono">{fmtPct(result.absolute_delta)}</strong>
    </div>
    <div>
      <span class="muted">Lift relativo</span>
      <strong class="mono">{fmtPct(result.relative_lift)}</strong>
    </div>
    <div>
      <span class="muted">95% CI</span>
      <strong class="mono">{fmtPct(result.ci95_low)} - {fmtPct(result.ci95_high)}</strong>
    </div>
    <div>
      <span class="muted">Ganador</span>
      <strong class="mono">{result.winner}</strong>
    </div>
  </section>
{/if}

<style>
  .experiment-form {
    align-items: end;
    gap: 12px;
    margin-bottom: 16px;
  }
  .experiment-form label {
    display: grid;
    gap: 4px;
    min-width: 220px;
    font-size: 11px;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.4px;
  }
  .experiment-form input {
    min-width: 260px;
    color: var(--text);
    text-transform: none;
    letter-spacing: 0;
  }

  .summary-band {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 16px;
    margin-bottom: 16px;
    background: var(--bg-elev);
  }
  .summary-band h2 {
    margin: 4px 0 0;
    font-size: 20px;
    line-height: 1.3;
    letter-spacing: 0;
  }
  .eyebrow {
    color: var(--text-muted);
    font-size: 11px;
  }
  .sig {
    min-width: 104px;
    display: grid;
    justify-items: end;
    gap: 2px;
  }
  .sig-value {
    font-size: 26px;
    color: var(--text);
  }
  .sig.good .sig-value { color: var(--success); }

  .layout {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 16px;
  }
  @media (max-width: 800px) {
    .layout { grid-template-columns: 1fr; }
    .summary-band { align-items: flex-start; flex-direction: column; }
    .sig { justify-items: start; }
  }
  .panel {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 14px;
    background: var(--bg-elev);
    display: grid;
    gap: 12px;
  }
  .panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .panel h2 {
    margin: 0;
    font-size: 14px;
    letter-spacing: 0;
  }
  .badge {
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 2px 8px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .treatment .badge {
    border-color: var(--accent-dim);
    color: var(--accent);
  }
  .rate {
    font-size: 34px;
    line-height: 1;
  }
  .metric-grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 10px;
  }
  .metric-grid div,
  .stats-strip div {
    display: grid;
    gap: 3px;
  }
  .bar {
    height: 8px;
    border-radius: 999px;
    background: var(--bg-hover);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .stats-strip {
    margin-top: 16px;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    background: var(--bg-elev);
  }
  @media (max-width: 900px) {
    .stats-strip { grid-template-columns: repeat(2, minmax(0, 1fr)); }
  }
  .error {
    color: var(--danger);
    background: var(--badge-error-bg);
    border: 1px solid var(--danger);
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
    margin-bottom: 12px;
  }
</style>
