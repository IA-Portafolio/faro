<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { fetchTrace, type SpanRow } from '$lib/api';
  import { formatTimestamp, formatDuration } from '$lib/stores';

  let spans: SpanRow[] = [];
  let error = '';
  let loading = true;
  let selected: SpanRow | null = null;

  $: id = $page.params.id;

  async function load(): Promise<void> {
    loading = true;
    try {
      spans = await fetchTrace(id);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  onMount(load);
  $: id, load();

  $: traceStartNs = spans.length > 0 ? Math.min(...spans.map((s) => Date.parse(s.timestamp.includes('T') ? s.timestamp : s.timestamp.replace(' ', 'T') + 'Z') * 1_000_000)) : 0;
  $: traceEndNs = spans.length > 0 ? Math.max(...spans.map((s) => Date.parse(s.timestamp.includes('T') ? s.timestamp : s.timestamp.replace(' ', 'T') + 'Z') * 1_000_000 + s.duration_ns)) : 0;
  $: totalDur = traceEndNs - traceStartNs || 1;

  function spanStartOffset(s: SpanRow): number {
    const ts = Date.parse(s.timestamp.includes('T') ? s.timestamp : s.timestamp.replace(' ', 'T') + 'Z') * 1_000_000;
    return ((ts - traceStartNs) / totalDur) * 100;
  }
  function spanWidth(s: SpanRow): number {
    return Math.max(0.3, (s.duration_ns / totalDur) * 100);
  }
</script>

<div class="page-header">
  <h1 class="page-title">Traza</h1>
  <div class="muted mono" style="font-size: 12px;">{id}</div>
</div>

{#if error}<div style="color: var(--danger);">{error}</div>{/if}
{#if loading}<div class="empty"><span class="spinner"></span> Cargando…</div>{/if}

{#if spans.length > 0}
  <div class="cards">
    <div class="card"><div class="label">Spans</div><div class="value">{spans.length}</div></div>
    <div class="card"><div class="label">Duración</div><div class="value">{formatDuration(totalDur)}</div></div>
    <div class="card"><div class="label">Servicios</div><div class="value">{new Set(spans.map((s) => s.service_name)).size}</div></div>
    <div class="card"><div class="label">Inicio</div><div class="value mono" style="font-size: 14px;">{formatTimestamp(spans[0].timestamp)}</div></div>
  </div>

  <div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; padding: 8px;">
    {#each spans as s}
      <div class="span-row" on:click={() => (selected = s)} role="button" tabindex="0" on:keypress={(e) => e.key === 'Enter' && (selected = s)}>
        <div class="span-name mono">
          <span style="color: var(--text-muted);">{s.service_name}</span>
          {' '}{s.name}
        </div>
        <div class="span-track">
          <div class="span-bar {s.status_code}" style="left: {spanStartOffset(s)}%; width: {spanWidth(s)}%;"></div>
        </div>
        <div class="tabular mono" style="text-align: right;">{formatDuration(s.duration_ns)}</div>
      </div>
    {/each}
  </div>
{/if}

{#if selected}
  <div class="drawer">
    <button class="close" on:click={() => (selected = null)}>Cerrar</button>
    <h2 style="margin-top: 0;">{selected.name}</h2>
    <div class="field"><label>Servicio</label><div>{selected.service_name}</div></div>
    <div class="field"><label>Tipo</label><div>{selected.kind}</div></div>
    <div class="field"><label>Estado</label><span class="badge {selected.status_code === 'ERROR' ? 'error' : 'ok'}">{selected.status_code}</span> {selected.status_message}</div>
    <div class="field"><label>Duración</label><div>{formatDuration(selected.duration_ns)}</div></div>
    <div class="field"><label>ID del span</label><div class="mono">{selected.span_id}</div></div>
    {#if selected.parent_span_id}
      <div class="field"><label>Padre</label><div class="mono">{selected.parent_span_id}</div></div>
    {/if}
    {#if Object.keys(selected.span_attributes ?? {}).length > 0}
      <div class="field"><label>Atributos</label><pre>{JSON.stringify(selected.span_attributes, null, 2)}</pre></div>
    {/if}
    {#if selected.events_names?.length > 0}
      <div class="field"><label>Eventos ({selected.events_names.length})</label>
        <ul style="padding-left: 16px;">
          {#each selected.events_names as n, i}
            <li><span class="mono">{selected.events_timestamps[i]}</span> — {n}</li>
          {/each}
        </ul>
      </div>
    {/if}
  </div>
{/if}
