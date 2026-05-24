<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { browser } from '$app/environment';
  import { goto } from '$app/navigation';
  import { fetchTraces, type TraceSummary } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, formatDuration, selectedProject } from '$lib/stores';
  import { isTyping } from '$lib/keyboard';
  import { asNumber, readFilters, writeFilters } from '$lib/url-filters';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';
  import SkeletonTable from '$lib/components/SkeletonTable.svelte';

  let traces: TraceSummary[] = [];
  let service = '';
  let status = '';
  let minDurationMs = 0;
  let loading = false;
  let error = '';
  let focusedIndex = -1;
  let tbodyEl: HTMLTableSectionElement | null = null;

  // Hidrata filtros locales desde el query string (los globales project/range
  // los maneja el layout). El bloque solo corre en cliente.
  if (browser) {
    const f = readFilters(['service', 'status', 'min_duration_ms']);
    if (f.service !== undefined) service = f.service;
    if (f.status !== undefined) status = f.status;
    if (f.min_duration_ms !== undefined) minDurationMs = asNumber(f.min_duration_ms, 0);
  }

  function syncUrl(): void {
    writeFilters({
      service,
      status,
      min_duration_ms: minDurationMs
    });
  }

  async function load(): Promise<void> {
    loading = true;
    error = '';
    syncUrl();
    try {
      traces = await fetchTraces({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined,
        service: service || undefined,
        status: status || undefined,
        min_duration_ms: minDurationMs || undefined,
        limit: 300
      });
      // Mantén la selección dentro de los límites si la lista cambia.
      if (focusedIndex >= traces.length) focusedIndex = traces.length - 1;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function ensureFocusedVisible(): Promise<void> {
    if (focusedIndex < 0 || !tbodyEl) return;
    await tick();
    const el = tbodyEl.querySelectorAll<HTMLElement>('tr.row')[focusedIndex];
    el?.scrollIntoView({ block: 'nearest' });
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isTyping(e.target)) return;

    if (e.key === 'Escape') {
      if (focusedIndex >= 0) {
        e.preventDefault();
        focusedIndex = -1;
      }
      return;
    }
    if (e.key === 'j' || e.key === 'ArrowDown') {
      if (traces.length === 0) return;
      e.preventDefault();
      focusedIndex = Math.min(traces.length - 1, Math.max(0, focusedIndex + 1));
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'k' || e.key === 'ArrowUp') {
      if (traces.length === 0) return;
      e.preventDefault();
      focusedIndex = Math.max(0, focusedIndex - 1);
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'Enter') {
      if (focusedIndex >= 0 && focusedIndex < traces.length) {
        e.preventDefault();
        void goto(`/traces/${traces[focusedIndex].trace_id}`);
      }
    }
  }

  onMount(async () => {
    await load();
    if (browser) window.addEventListener('keydown', onKeydown);
  });
  onDestroy(() => {
    if (browser) window.removeEventListener('keydown', onKeydown);
  });
  $: $timeRange, $selectedProject, load();
</script>

<div class="page-header">
  <h1 class="page-title">Trazas</h1>
  <TimeRangePicker />
</div>

<div class="toolbar">
  <input
    placeholder="Servicio (pulsa /)"
    bind:value={service}
    on:change={load}
    style="width: 220px"
    data-search-input
  />
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
    <tbody bind:this={tbodyEl}>
      {#if loading && traces.length === 0}
        <SkeletonTable rows={10} cols={7} widths={['16%', '24%', '12%', '20%', '10%', '8%', '10%']} />
      {/if}
      {#each traces as t, i (t.trace_id)}
        <tr class="row" class:focused={i === focusedIndex} on:click={() => (focusedIndex = i)}>
          <td class="muted mono">{formatTimestamp(t.timestamp)}</td>
          <td><a href="/traces/{t.trace_id}" class="mono">{t.trace_id.slice(0, 16)}…</a></td>
          <td>{t.service_name}</td>
          <td class="mono">{t.root_name}</td>
          <td class="tabular">{formatDuration(t.duration_ns)}</td>
          <td class="tabular">{t.span_count}</td>
          <td><span class="badge {t.status_code === 'ERROR' ? 'error' : t.status_code === 'OK' ? 'ok' : 'debug'}">{t.status_code}</span></td>
        </tr>
      {/each}
    </tbody>
  </table>
</div>

{#if !loading && traces.length === 0}
  {@const hasFilters = !!(service || status || minDurationMs)}
  <OnboardingEmpty kind="traces" filteredOut={hasFilters} />
{/if}

<style>
  tr.row.focused td {
    background: var(--bg-hover);
    box-shadow: inset 3px 0 0 var(--accent);
  }
</style>
