<script lang="ts">
  import { onMount } from 'svelte';
  import { fetchIssues, type Issue } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, selectedProject } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';

  let issues: Issue[] = [];
  let service = '';
  let status = '';
  let error = '';
  let loading = false;

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      issues = await fetchIssues({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined,
        service: service || undefined,
        status: status || undefined
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);
  $: $timeRange, $selectedProject, load();

  const statusLabel: Record<string, string> = {
    unresolved: 'sin resolver',
    resolved: 'resuelto',
    ignored: 'ignorado',
    '': 'sin resolver'
  };
</script>

<div class="page-header">
  <h1 class="page-title">Errores</h1>
  <TimeRangePicker />
</div>

<div class="toolbar">
  <input placeholder="Servicio" bind:value={service} on:change={load} style="width: 180px" />
  <select bind:value={status} on:change={load}>
    <option value="">Todos los estados</option>
    <option value="unresolved">Sin resolver</option>
    <option value="resolved">Resueltos</option>
    <option value="ignored">Ignorados</option>
  </select>
  <button on:click={load}>{loading ? 'Cargando…' : 'Refrescar'}</button>
</div>

{#if error}<div style="color: var(--danger);">{error}</div>{/if}

<div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead>
      <tr><th>Estado</th><th>Problema</th><th>Servicio</th><th>Eventos</th><th>Última vez</th></tr>
    </thead>
    <tbody>
      {#each issues as i}
        <tr>
          <td><span class="badge {i.status || 'unresolved'}">{statusLabel[i.status] ?? 'sin resolver'}</span></td>
          <td>
            <a href="/errors/{i.fingerprint}">
              <strong>{i.exception_type || 'Error'}</strong>
            </a>
            <div class="muted" style="max-width: 600px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap;">{i.message}</div>
          </td>
          <td>{i.service_name}</td>
          <td class="tabular">{i.event_count.toLocaleString()}</td>
          <td class="muted mono">{formatTimestamp(i.last_seen)}</td>
        </tr>
      {/each}
      {#if !loading && issues.length === 0}
        <tr><td colspan="5" class="empty">Sin errores detectados.</td></tr>
      {/if}
    </tbody>
  </table>
</div>
