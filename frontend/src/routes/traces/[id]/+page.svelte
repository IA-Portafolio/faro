<script lang="ts">
  /**
   * Página `/traces/[id]` — detalle de una traza.
   *
   * `[id]` es el `trace_id`. Carga todos los spans de la traza (`fetchTrace`) y los
   * pinta como flamegraph jerárquico (`Flamegraph`), con un panel de detalle del
   * span seleccionado. Un span = una operación temporizada dentro de la traza.
   */
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { fetchTrace, type SpanRow } from '$lib/api';
  import { formatTimestamp, formatDuration } from '$lib/stores';
  import Flamegraph from '$lib/components/Flamegraph.svelte';

  let spans: SpanRow[] = [];
  let error = '';
  let loading = true;
  let selected: SpanRow | null = null;

  $: id = $page.params.id ?? '';

  async function load(): Promise<void> {
    if (!id) return;
    loading = true;
    error = '';
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

  // Resumen calculado solo para las cards de cabecera.
  function tsToNs(ts: string): number {
    if (!ts) return 0;
    const iso = ts.includes('T') ? ts : ts.replace(' ', 'T') + 'Z';
    return Date.parse(iso) * 1_000_000;
  }
  $: traceStartNs = spans.length > 0 ? Math.min(...spans.map((s) => tsToNs(s.timestamp))) : 0;
  $: traceEndNs   = spans.length > 0 ? Math.max(...spans.map((s) => tsToNs(s.timestamp) + s.duration_ns)) : 0;
  $: totalDur     = traceEndNs - traceStartNs || 1;

  // Parsea el JSON de atributos por evento — tolerante a inputs malos.
  function parseEventAttrs(raw: string): Record<string, unknown> | null {
    if (!raw) return null;
    try {
      const v = JSON.parse(raw);
      if (v && typeof v === 'object') return v as Record<string, unknown>;
    } catch {
      /* ignora — no se renderiza */
    }
    return null;
  }

  function onSpanSelect(e: CustomEvent<SpanRow>): void {
    selected = e.detail;
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape' && selected) {
      e.preventDefault();
      selected = null;
    }
  }
</script>

<svelte:window on:keydown={onKeydown} />

<div class="page-header">
  <h1 class="page-title">Traza</h1>
  <div class="flex gap-12 center">
    <a href="/logs?trace_id={encodeURIComponent(id)}" title="Ver logs ligados a esta traza">
      ≡ Logs de la traza
    </a>
    <div class="muted mono" style="font-size: 12px;">{id}</div>
  </div>
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

  <Flamegraph {spans} on:select={onSpanSelect} />
{/if}

{#if selected}
  <div class="drawer">
    <button class="close" on:click={() => (selected = null)}>Cerrar</button>
    <h2 style="margin-top: 0;">{selected.name}</h2>

    <div class="field"><label>Servicio</label><div>{selected.service_name}</div></div>
    <div class="field"><label>Tipo</label><div>{selected.kind || 'INTERNAL'}</div></div>
    <div class="field"><label>Estado</label>
      <span class="badge {selected.status_code === 'ERROR' ? 'error' : selected.status_code === 'OK' ? 'ok' : 'debug'}">
        {selected.status_code || 'UNSET'}
      </span>
      {#if selected.status_message}<span class="muted"> · {selected.status_message}</span>{/if}
    </div>
    <div class="field"><label>Duración</label><div>{formatDuration(selected.duration_ns)}</div></div>
    <div class="field"><label>Comienza</label><div class="mono">{formatTimestamp(selected.timestamp)}</div></div>
    <div class="field"><label>ID del span</label><div class="mono" style="word-break: break-all;">{selected.span_id}</div></div>
    {#if selected.parent_span_id}
      <div class="field"><label>Padre</label><div class="mono" style="word-break: break-all;">{selected.parent_span_id}</div></div>
    {/if}

    {#if Object.keys(selected.span_attributes ?? {}).length > 0}
      <div class="field"><label>Atributos del span</label>
        <pre>{JSON.stringify(selected.span_attributes, null, 2)}</pre>
      </div>
    {/if}
    {#if Object.keys(selected.resource_attributes ?? {}).length > 0}
      <div class="field"><label>Atributos del recurso</label>
        <pre>{JSON.stringify(selected.resource_attributes, null, 2)}</pre>
      </div>
    {/if}

    {#if (selected.events_names?.length ?? 0) > 0}
      <div class="field">
        <label>Eventos ({selected.events_names.length})</label>
        <ul class="evt-list">
          {#each selected.events_names as n, i}
            {@const attrs = parseEventAttrs(selected.events_attributes?.[i] ?? '')}
            <li>
              <div class="evt-head">
                <span class="mono evt-ts">{selected.events_timestamps?.[i] ?? ''}</span>
                <strong>{n}</strong>
              </div>
              {#if attrs}
                <pre class="evt-attrs">{JSON.stringify(attrs, null, 2)}</pre>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}

    {#if (selected.links_trace_ids?.length ?? 0) > 0}
      <div class="field">
        <label>Links salientes ({selected.links_trace_ids?.length})</label>
        <ul class="link-list">
          {#each selected.links_trace_ids ?? [] as tid, i}
            {@const sid = selected.links_span_ids?.[i] ?? ''}
            <li>
              <a href={`/traces/${encodeURIComponent(tid)}`} class="mono" title="Abrir traza enlazada">
                {tid}
              </a>
              {#if sid}
                <span class="muted mono" style="font-size: 11px;"> · span {sid}</span>
              {/if}
            </li>
          {/each}
        </ul>
      </div>
    {/if}
  </div>
{/if}

<style>
  .evt-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .evt-list li {
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 6px 8px;
    background: var(--bg);
  }
  .evt-head { display: flex; gap: 8px; align-items: baseline; }
  .evt-ts { font-size: 11.5px; color: var(--text-muted); }
  .evt-attrs {
    margin: 6px 0 0;
    padding: 6px 8px;
    font-size: 11.5px;
    max-height: 200px;
    overflow: auto;
  }
  .link-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .link-list li {
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    background: var(--bg);
    word-break: break-all;
  }
</style>
