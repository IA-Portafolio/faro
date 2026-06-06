<script lang="ts">
  /**
   * Página `/errors/[fp]` — detalle de un Issue (grupo de errores).
   *
   * `[fp]` es el `fingerprint`: el hash que agrupa errores equivalentes en un
   * mismo Issue. Carga el Issue (`fetchIssue`), sus últimos eventos de error y las
   * sesiones afectadas, y permite cambiar su estado (abierto/resuelto/ignorado)
   * con `updateIssueStatus`.
   */
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import {
    fetchIssue,
    fetchIssueSessions,
    updateIssueStatus,
    type Issue,
    type ErrorEvent,
    type IssueSession,
  } from '$lib/api';
  import { formatTimestamp } from '$lib/stores';
  import { toast } from '$lib/toasts';

  let issue: Issue | null = null;
  let events: ErrorEvent[] = [];
  let sessions: IssueSession[] = [];
  let error = '';
  let loading = true;

  $: fp = $page.params.fp ?? '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const r = await fetchIssue(fp);
      issue = r.issue;
      events = r.events;
      // Las sesiones es una query separada y no bloqueante — si falla, el detalle
      // del error sigue cargando.
      fetchIssueSessions(fp).then((s) => { sessions = s; }).catch(() => { sessions = []; });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function setStatus(status: string): Promise<void> {
    if (!issue) return;
    const labels: Record<string, string> = {
      unresolved: 'sin resolver',
      resolved: 'resuelto',
      ignored: 'ignorado'
    };
    try {
      await updateIssueStatus(fp, { status, service_name: issue.service_name });
      await load();
      toast.success(`Issue marcado como ${labels[status] ?? status}`);
    } catch (e: unknown) {
      toast.fromError('No se pudo cambiar el estado del issue', e);
    }
  }

  onMount(load);
  $: fp, load();

  const statusLabel: Record<string, string> = {
    unresolved: 'sin resolver',
    resolved: 'resuelto',
    ignored: 'ignorado',
    '': 'sin resolver'
  };

  function sessionIdOf(e: ErrorEvent): string {
    return e.attributes?.['session.id'] ?? '';
  }

  function hasReplay(sid: string): boolean {
    return sessions.some((s) => s.session_id === sid && s.has_replay === 1);
  }
</script>

<div class="page-header">
  <h1 class="page-title">{issue?.exception_type ?? 'Problema'}</h1>
  {#if issue}
    <div class="flex gap-8">
      <button on:click={() => setStatus('resolved')}>Marcar resuelto</button>
      <button on:click={() => setStatus('ignored')}>Ignorar</button>
      <button on:click={() => setStatus('unresolved')}>Reabrir</button>
    </div>
  {/if}
</div>

{#if error}<div style="color: var(--danger);">{error}</div>{/if}
{#if loading}<div class="empty"><span class="spinner"></span></div>{/if}

{#if issue}
  <div class="card mt-8">
    <div class="muted">{issue.message}</div>
    <div class="flex gap-16 mt-8" style="flex-wrap: wrap;">
      <div><span class="muted">Estado:</span> <span class="badge {issue.status || 'unresolved'}">{statusLabel[issue.status] ?? 'sin resolver'}</span></div>
      <div><span class="muted">Servicio:</span> {issue.service_name}</div>
      <div><span class="muted">Eventos:</span> {issue.event_count.toLocaleString()}</div>
      <div><span class="muted">Primer evento:</span> {formatTimestamp(issue.first_seen)}</div>
      <div><span class="muted">Último evento:</span> {formatTimestamp(issue.last_seen)}</div>
    </div>
  </div>

  {#if sessions.length > 0}
    <h2 style="font-size: 16px; margin-top: 24px;">Sesiones afectadas</h2>
    <div class="card mt-8" style="padding: 0;">
      {#each sessions.slice(0, 10) as s}
        <div class="session-row">
          <div class="mono ts">{formatTimestamp(s.timestamp)}</div>
          <div class="mono sid" title={s.session_id}>{s.session_id.slice(0, 20)}…</div>
          <div class="muted">{s.service_name}</div>
          {#if s.has_replay === 1}
            <a class="replay-link" href="/replays/{encodeURIComponent(s.session_id)}">
              ▶ Ver replay
            </a>
          {:else}
            <span class="muted no-replay">sin replay</span>
          {/if}
        </div>
      {/each}
    </div>
  {/if}

  <h2 style="font-size: 16px; margin-top: 24px;">Eventos recientes</h2>
  {#each events as e}
    {@const sid = sessionIdOf(e)}
    <details class="card mt-8">
      <summary><span class="mono">{formatTimestamp(e.timestamp)}</span> · {e.message}</summary>
      {#if e.exception_message}<div class="mt-8"><strong>{e.exception_type}</strong>: {e.exception_message}</div>{/if}
      {#if e.stack_trace}<pre style="margin-top: 8px;">{e.stack_trace}</pre>{/if}
      <div class="mt-8 event-links">
        {#if e.trace_id}
          <a href="/traces/{e.trace_id}" class="mono">↗ traza {e.trace_id.slice(0, 16)}…</a>
        {/if}
        {#if sid && hasReplay(sid)}
          <a href="/replays/{encodeURIComponent(sid)}" class="replay-link">▶ Ver replay de esta sesión</a>
        {:else if sid}
          <span class="muted mono">sesión {sid.slice(0, 12)}… (sin replay)</span>
        {/if}
      </div>
      {#if Object.keys(e.attributes ?? {}).length > 0}
        <pre style="margin-top: 8px;">{JSON.stringify(e.attributes, null, 2)}</pre>
      {/if}
    </details>
  {/each}
{/if}

<style>
  .session-row {
    display: grid;
    grid-template-columns: 180px 240px 1fr auto;
    gap: 14px;
    align-items: center;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
    font-size: 13px;
  }
  .session-row:last-child { border-bottom: none; }
  .session-row .ts { color: var(--text-muted); white-space: nowrap; }
  .session-row .sid {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .replay-link {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 10px;
    background: var(--bg-elev);
    border: 1px solid var(--accent, var(--border));
    border-radius: 4px;
    color: var(--accent, var(--text));
    text-decoration: none;
    font-size: 12px;
    font-weight: 600;
  }
  .replay-link:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }

  .event-links {
    display: flex;
    gap: 14px;
    flex-wrap: wrap;
    align-items: center;
  }

  .no-replay { font-size: 12px; }
</style>
