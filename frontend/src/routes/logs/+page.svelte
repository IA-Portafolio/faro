<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fetchLogs, apiBase, type LogRow } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, selectedProject } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import SeverityBadge from '$lib/components/SeverityBadge.svelte';
  import LogVolumeHistogram from '$lib/components/LogVolumeHistogram.svelte';

  let logs: LogRow[] = [];
  let service = '';
  let minSeverity = 0;
  let queryStr = '';
  let traceId = '';
  let loading = false;
  let error = '';
  let live = false;
  let evtSource: EventSource | null = null;
  let selected: LogRow | null = null;
  let subRange: { from: string; to: string } | null = null;

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const base: Record<string, unknown> = {
        project: $selectedProject || undefined,
        service: service || undefined,
        min_severity: minSeverity || undefined,
        query: queryStr || undefined,
        trace_id: traceId || undefined,
        limit: 500
      };
      if (subRange) {
        base.from = subRange.from;
        base.to = subRange.to;
      } else {
        base.last_minutes = rangeMinutes($timeRange);
      }
      logs = await fetchLogs(base);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function onHistogramSelection(e: CustomEvent<{ from: string; to: string } | null>): void {
    subRange = e.detail;
    if (subRange && live) {
      // El live tail no tiene sentido sobre un sub-rango congelado — se detiene.
      stopLive();
    }
    load();
  }

  function clearSubRange(): void {
    if (!subRange) return;
    subRange = null;
    load();
  }

  function stopLive(): void {
    evtSource?.close();
    evtSource = null;
    live = false;
  }

  function toggleLive(): void {
    if (live) {
      stopLive();
      return;
    }
    if (subRange) {
      // Limpia el sub-rango al iniciar el live tail.
      subRange = null;
    }
    const params = new URLSearchParams();
    if ($selectedProject) params.set('project', $selectedProject);
    if (service) params.set('service', service);
    if (minSeverity) params.set('min_severity', String(minSeverity));
    if (queryStr) params.set('query', queryStr);
    const url = `${apiBase()}/api/v1/logs/live?${params.toString()}`;
    evtSource = new EventSource(url);
    evtSource.addEventListener('log', (e) => {
      try {
        const row = JSON.parse((e as MessageEvent).data) as LogRow;
        logs = [row, ...logs].slice(0, 500);
      } catch (err) {
        console.warn('evento inválido', err);
      }
    });
    evtSource.onerror = () => {
      console.warn('error de SSE');
    };
    live = true;
  }

  onMount(load);
  onDestroy(() => evtSource?.close());

  // Recarga al cambiar rango / proyecto, y resetea cualquier selección de sub-rango
  // activa para que no se arrastre entre rangos.
  let prevRange = $timeRange;
  let prevProject = $selectedProject;
  $: {
    if (prevRange !== $timeRange || prevProject !== $selectedProject) {
      prevRange = $timeRange;
      prevProject = $selectedProject;
      subRange = null;
      load();
    }
  }
</script>

<div class="page-header">
  <h1 class="page-title">Logs</h1>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button class:primary={live} on:click={toggleLive} disabled={!!subRange && !live} title={subRange ? 'Limpia el sub-rango para activar el tail' : ''}>
      {#if live}<span class="live-dot"></span> En vivo{:else}▶ Activar tail{/if}
    </button>
  </div>
</div>

<LogVolumeHistogram
  lastMinutes={rangeMinutes($timeRange)}
  service={service || undefined}
  minSeverity={minSeverity || undefined}
  query={queryStr || undefined}
  traceId={traceId || undefined}
  project={$selectedProject || undefined}
  selection={subRange}
  on:selectionchange={onHistogramSelection}
/>

<div class="toolbar">
  <input placeholder="Servicio" bind:value={service} on:change={load} style="width: 180px" />
  <select bind:value={minSeverity} on:change={load}>
    <option value={0}>Cualquier severidad</option>
    <option value={5}>DEBUG y superior</option>
    <option value={9}>INFO y superior</option>
    <option value={13}>WARN y superior</option>
    <option value={17}>ERROR y superior</option>
  </select>
  <input placeholder="Buscar en el mensaje…" bind:value={queryStr} on:keydown={(e) => e.key === 'Enter' && load()} style="flex: 1; min-width: 200px;" />
  <input placeholder="ID de traza" bind:value={traceId} on:keydown={(e) => e.key === 'Enter' && load()} class="mono" style="width: 200px;" />
  <button on:click={load}>{loading ? 'Cargando…' : 'Buscar'}</button>
  {#if subRange}
    <button class="danger" on:click={clearSubRange} title="Volver a ver todo el rango">Limpiar sub-rango</button>
  {/if}
</div>

{#if error}<div style="color: var(--danger);">Error: {error}</div>{/if}

<div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <div style="padding: 8px 12px; background: var(--bg); border-bottom: 1px solid var(--border); font-size: 12px; color: var(--text-muted); display: grid; grid-template-columns: 180px 80px 140px 1fr; gap: 12px;">
    <div>Hora</div><div>Severidad</div><div>Servicio</div><div>Mensaje</div>
  </div>
  {#each logs as row (row.timestamp + row.body)}
    <div class="log-row" on:click={() => (selected = row)} on:keypress={(e) => e.key === 'Enter' && (selected = row)} tabindex="0" role="button">
      <div class="ts mono">{formatTimestamp(row.timestamp)}</div>
      <div><SeverityBadge severity={row.severity_text} /></div>
      <div class="muted mono">{row.service_name}</div>
      <div class="body">{row.body}</div>
    </div>
  {/each}
  {#if !loading && logs.length === 0}
    <div class="empty">No hay logs en el rango seleccionado.</div>
  {/if}
</div>

{#if selected}
  <div class="drawer">
    <button class="close" on:click={() => (selected = null)}>Cerrar</button>
    <h2 style="margin-top: 0;">Detalle del log</h2>
    <div class="field"><label>Hora</label><div class="mono">{selected.timestamp}</div></div>
    <div class="field"><label>Severidad</label><SeverityBadge severity={selected.severity_text} /></div>
    <div class="field"><label>Servicio</label><div class="mono">{selected.service_name}</div></div>
    {#if selected.trace_id}
      <div class="field"><label>Traza</label>
        <a href="/traces/{selected.trace_id}" class="mono">{selected.trace_id}</a>
      </div>
    {/if}
    <div class="field"><label>Mensaje</label><pre>{selected.body}</pre></div>
    {#if Object.keys(selected.attributes ?? {}).length > 0}
      <div class="field"><label>Atributos</label>
        <pre>{JSON.stringify(selected.attributes, null, 2)}</pre>
      </div>
    {/if}
    {#if Object.keys(selected.resource_attributes ?? {}).length > 0}
      <div class="field"><label>Recurso</label>
        <pre>{JSON.stringify(selected.resource_attributes, null, 2)}</pre>
      </div>
    {/if}
  </div>
{/if}
