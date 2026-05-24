<script lang="ts">
  import { page } from '$app/stores';
  import { onMount } from 'svelte';

  import { fetchProductSessionTraces, type TraceSummary } from '$lib/api';
  import { formatDuration, formatTimestamp } from '$lib/stores';

  let traces: TraceSummary[] = [];
  let loading = true;
  let error = '';

  $: sessionId = $page.params.session_id ?? '';
  $: project = $page.url.searchParams.get('project') ?? '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      if (!sessionId || !project) {
        throw new Error('project requerido para resolver traces de sesión');
      }
      traces = await fetchProductSessionTraces(sessionId, project);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      traces = [];
    } finally {
      loading = false;
    }
  }

  function shortId(id: string): string {
    return id.length > 22 ? `${id.slice(0, 12)}...${id.slice(-6)}` : id;
  }

  onMount(load);
</script>

<div class="page-header">
  <div>
    <h1 class="page-title">Session traces</h1>
    <div class="muted subtitle">
      <span class="mono">{shortId(sessionId)}</span>
      {#if project}
        <span> · </span><span class="mono">{project}</span>
      {/if}
    </div>
  </div>
  <div class="flex gap-12 center">
    <a class="button-link" href="/sessions">Volver a sessions</a>
    <button on:click={load} disabled={loading}>{loading ? 'Cargando...' : 'Recargar'}</button>
  </div>
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

<div class="trace-table">
  <div class="trace-head">
    <div>Hora</div>
    <div>Trace</div>
    <div>Servicio</div>
    <div>Span raíz</div>
    <div>Duración</div>
    <div>Spans</div>
    <div>Estado</div>
  </div>

  {#if loading && traces.length === 0}
    {#each Array(8) as _}
      <div class="trace-row skeleton-row">
        <span></span><span></span><span></span><span></span><span></span><span></span><span></span>
      </div>
    {/each}
  {:else}
    {#each traces as trace (trace.trace_id)}
      <a class="trace-row" class:error={trace.status_code === 'ERROR'} href={`/traces/${trace.trace_id}`}>
        <div class="mono muted">{formatTimestamp(trace.timestamp)}</div>
        <div class="mono trace-id" title={trace.trace_id}>{shortId(trace.trace_id)}</div>
        <div>{trace.service_name || '-'}</div>
        <div class="mono root" title={trace.root_name}>{trace.root_name || '-'}</div>
        <div class="mono tabular">{formatDuration(trace.duration_ns)}</div>
        <div class="mono tabular">{trace.span_count.toLocaleString()}</div>
        <div>
          <span class="badge {trace.status_code === 'ERROR' ? 'error' : trace.status_code === 'OK' ? 'ok' : 'debug'}">
            {trace.status_code || 'UNSET'}
          </span>
        </div>
      </a>
    {/each}
  {/if}
</div>

{#if !loading && !error && traces.length === 0}
  <div class="empty">
    Esta sesión no tiene traces backend materializados todavía.
  </div>
{/if}

<style>
  .subtitle { font-size: 12px; margin-top: 2px; }
  .error-box {
    color: var(--danger);
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
  }
  .trace-table {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .trace-head,
  .trace-row {
    display: grid;
    grid-template-columns: 180px minmax(160px, 1fr) minmax(120px, 0.8fr) minmax(220px, 1.2fr) 100px 70px 90px;
    gap: 12px;
    align-items: center;
  }
  .trace-head {
    padding: 8px 12px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
  }
  .trace-row {
    min-height: 48px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    text-decoration: none;
    font-size: 12.5px;
  }
  .trace-row:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }
  .trace-row.error {
    box-shadow: inset 3px 0 0 var(--danger);
  }
  .trace-row:last-child { border-bottom: 0; }
  .trace-id,
  .root {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .skeleton-row span {
    display: block;
    height: 14px;
    border-radius: 4px;
    background: var(--bg);
  }
  .empty {
    margin-top: 12px;
    color: var(--text-muted);
    border: 1px dashed var(--border);
    border-radius: 6px;
    padding: 16px;
  }
  @media (max-width: 1000px) {
    .trace-table { overflow-x: auto; }
    .trace-head,
    .trace-row {
      min-width: 980px;
    }
  }
</style>
