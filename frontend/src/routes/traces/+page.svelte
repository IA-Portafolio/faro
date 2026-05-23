<script lang="ts">
  import { onMount } from 'svelte';
  import { fetchTraces, type TraceSummary } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, formatDuration, selectedProject } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';

  let traces: TraceSummary[] = [];
  let service = '';
  let status = '';
  let minDurationMs = 0;
  let loading = false;
  let error = '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      traces = await fetchTraces({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined,
        service: service || undefined,
        status: status || undefined,
        min_duration_ms: minDurationMs || undefined,
        limit: 300
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);
  $: $timeRange, $selectedProject, load();
</script>

<div class="page-header">
  <h1 class="page-title">Trazas</h1>
  <TimeRangePicker />
</div>

<div class="toolbar">
  <input placeholder="Servicio" bind:value={service} on:change={load} style="width: 180px" />
  <select bind:value={status} on:change={load}>
    <option value="">Cualquier estado</option>
    <option value="OK">OK</option>
    <option value="ERROR">ERROR</option>
    <option value="UNSET">UNSET</option>
  </select>
  <input type="number" placeholder="Duración mínima (ms)" bind:value={minDurationMs} on:change={load} style="width: 200px" />
  <button on:click={load}>{loading ? 'Cargando…' : 'Refrescar'}</button>
</div>

{#if error}<div style="color: var(--danger);">Error: {error}</div>{/if}

<div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead>
      <tr><th>Hora</th><th>Traza</th><th>Servicio</th><th>Span raíz</th><th>Duración</th><th>Spans</th><th>Estado</th></tr>
    </thead>
    <tbody>
      {#each traces as t}
        <tr>
          <td class="muted mono">{formatTimestamp(t.timestamp)}</td>
          <td><a href="/traces/{t.trace_id}" class="mono">{t.trace_id.slice(0, 16)}…</a></td>
          <td>{t.service_name}</td>
          <td class="mono">{t.root_name}</td>
          <td class="tabular">{formatDuration(t.duration_ns)}</td>
          <td class="tabular">{t.span_count}</td>
          <td><span class="badge {t.status_code === 'ERROR' ? 'error' : t.status_code === 'OK' ? 'ok' : 'debug'}">{t.status_code}</span></td>
        </tr>
      {/each}
      {#if !loading && traces.length === 0}
        <tr><td colspan="7" class="empty">Sin trazas todavía.</td></tr>
      {/if}
    </tbody>
  </table>
</div>
