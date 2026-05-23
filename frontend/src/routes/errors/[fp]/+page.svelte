<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { fetchIssue, updateIssueStatus, type Issue, type ErrorEvent } from '$lib/api';
  import { formatTimestamp } from '$lib/stores';

  let issue: Issue | null = null;
  let events: ErrorEvent[] = [];
  let error = '';
  let loading = true;

  $: fp = $page.params.fp;

  async function load(): Promise<void> {
    loading = true;
    try {
      const r = await fetchIssue(fp);
      issue = r.issue;
      events = r.events;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function setStatus(status: string): Promise<void> {
    if (!issue) return;
    await updateIssueStatus(fp, { status, service_name: issue.service_name });
    await load();
  }

  onMount(load);
  $: fp, load();

  const statusLabel: Record<string, string> = {
    unresolved: 'sin resolver',
    resolved: 'resuelto',
    ignored: 'ignorado',
    '': 'sin resolver'
  };
</script>

<div class="page-header">
  <h1 class="page-title">{issue?.exception_type ?? 'Issue'}</h1>
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

  <h2 style="font-size: 16px; margin-top: 24px;">Eventos recientes</h2>
  {#each events as e}
    <details class="card mt-8">
      <summary><span class="mono">{formatTimestamp(e.timestamp)}</span> · {e.message}</summary>
      {#if e.exception_message}<div class="mt-8"><strong>{e.exception_type}</strong>: {e.exception_message}</div>{/if}
      {#if e.stack_trace}<pre style="margin-top: 8px;">{e.stack_trace}</pre>{/if}
      {#if e.trace_id}
        <div class="mt-8"><a href="/traces/{e.trace_id}" class="mono">ver traza {e.trace_id.slice(0, 16)}…</a></div>
      {/if}
      {#if Object.keys(e.attributes ?? {}).length > 0}
        <pre style="margin-top: 8px;">{JSON.stringify(e.attributes, null, 2)}</pre>
      {/if}
    </details>
  {/each}
{/if}
