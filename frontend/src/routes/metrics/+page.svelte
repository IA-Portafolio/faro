<script lang="ts">
  import { onMount } from 'svelte';
  import { browser } from '$app/environment';
  import { fetchMetricNames, fetchMetricSeries, type MetricName, type Point } from '$lib/api';
  import { timeRange, rangeMinutes, selectedProject } from '$lib/stores';
  import { readFilters, writeFilters } from '$lib/url-filters';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import Chart from '$lib/components/Chart.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';

  let metrics: MetricName[] = [];
  let selectedName = '';
  let selectedSvc = '';
  let agg = 'avg';
  let series: Point[] = [];
  let error = '';
  let loadingNames = false;
  let loadingSeries = false;

  if (browser) {
    const f = readFilters(['metric', 'service', 'agg']);
    if (f.metric !== undefined) selectedName = f.metric;
    if (f.service !== undefined) selectedSvc = f.service;
    if (f.agg !== undefined) agg = f.agg;
  }

  $: if (browser) writeFilters({
    metric: selectedName,
    service: selectedSvc,
    agg: agg === 'avg' ? '' : agg
  });

  async function loadNames(): Promise<void> {
    loadingNames = true;
    try {
      metrics = await fetchMetricNames({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
      if (!selectedName && metrics.length > 0) {
        selectedName = metrics[0].metric_name;
      }
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loadingNames = false;
    }
  }

  async function loadSeries(): Promise<void> {
    if (!selectedName) return;
    loadingSeries = true;
    error = '';
    try {
      const isEventMetric = selectedName.startsWith('events.') && selectedName.endsWith('.count');
      series = await fetchMetricSeries({
        name: selectedName,
        service: selectedSvc || undefined,
        project: $selectedProject || undefined,
        agg,
        last_minutes: rangeMinutes($timeRange),
        bucket_seconds: isEventMetric ? 3600 : 60
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loadingSeries = false;
    }
  }

  onMount(async () => {
    await loadNames();
    await loadSeries();
  });

  $: $timeRange, $selectedProject, loadNames();
  $: selectedName, selectedSvc, agg, loadSeries();

  $: distinctServices = Array.from(new Set(metrics.filter((m) => m.metric_name === selectedName).map((m) => m.service_name)));
  $: selectedMeta = metrics.find((m) => m.metric_name === selectedName);
  $: selectedIsEventMetric = selectedName.startsWith('events.') && selectedName.endsWith('.count');
  $: chartLabel = selectedName ? `${selectedIsEventMetric ? 'count' : agg}(${selectedName})` : '';
</script>

<div class="page-header">
  <h1 class="page-title">Métricas</h1>
  <TimeRangePicker />
</div>

<div class="toolbar">
  <select bind:value={selectedName} style="min-width: 240px;">
    <option value="">{loadingNames ? 'Cargando…' : 'Elige una métrica'}</option>
    {#each [...new Set(metrics.map((m) => m.metric_name))] as name}
      <option value={name}>{name}</option>
    {/each}
  </select>
  <select bind:value={selectedSvc}>
    <option value="">Todos los servicios</option>
    {#each distinctServices as s}
      <option value={s}>{s}</option>
    {/each}
  </select>
  <select bind:value={agg}>
    <option value="avg">avg</option>
    <option value="sum">sum</option>
    <option value="max">max</option>
    <option value="min">min</option>
    <option value="count">count</option>
  </select>
  {#if selectedMeta}
    <span class="muted">{selectedMeta.metric_type}{selectedMeta.metric_unit ? ` · ${selectedMeta.metric_unit}` : ''}</span>
  {/if}
</div>

{#if error}<div style="color: var(--danger);">{error}</div>{/if}

{#if !loadingNames && metrics.length === 0}
  <OnboardingEmpty kind="metrics" />
{:else}
  <div class="card">
    {#if loadingSeries && series.length === 0}
      <Skeleton width="100%" height="280px" radius="4px" />
    {:else}
      <Chart points={series} label={chartLabel} height={280} />
    {/if}
  </div>
{/if}
