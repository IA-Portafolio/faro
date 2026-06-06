<script lang="ts">
  /**
   * Página `/errors` — lista de "Issues" (grupos de errores).
   *
   * Un Issue agrupa todos los errores con el mismo `fingerprint`. Aquí se listan
   * con filtros por servicio y estado (sincronizados con la URL vía `url-filters`),
   * navegación por teclado j/k y salto al detalle `/errors/<fingerprint>`.
   */
  import { onMount, onDestroy, tick } from 'svelte';
  import { browser } from '$app/environment';
  import { goto } from '$app/navigation';
  import { fetchIssues, type Issue } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, selectedProject } from '$lib/stores';
  import { isTyping } from '$lib/keyboard';
  import { readFilters, writeFilters } from '$lib/url-filters';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';
  import SkeletonTable from '$lib/components/SkeletonTable.svelte';

  let issues: Issue[] = [];
  let service = '';
  let status = '';
  let error = '';
  let loading = false;
  let focusedIndex = -1;
  let tbodyEl: HTMLTableSectionElement | null = null;

  if (browser) {
    const f = readFilters(['service', 'status']);
    if (f.service !== undefined) service = f.service;
    if (f.status !== undefined) status = f.status;
  }

  function syncUrl(): void {
    writeFilters({ service, status });
  }

  async function load(): Promise<void> {
    loading = true;
    error = '';
    syncUrl();
    try {
      issues = await fetchIssues({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined,
        service: service || undefined,
        status: status || undefined
      });
      if (focusedIndex >= issues.length) focusedIndex = issues.length - 1;
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
      if (issues.length === 0) return;
      e.preventDefault();
      focusedIndex = Math.min(issues.length - 1, Math.max(0, focusedIndex + 1));
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'k' || e.key === 'ArrowUp') {
      if (issues.length === 0) return;
      e.preventDefault();
      focusedIndex = Math.max(0, focusedIndex - 1);
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'Enter') {
      if (focusedIndex >= 0 && focusedIndex < issues.length) {
        e.preventDefault();
        void goto(`/errors/${issues[focusedIndex].fingerprint}`);
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
  <input
    placeholder="Servicio (pulsa /)"
    bind:value={service}
    on:change={load}
    style="width: 220px"
    data-search-input
  />
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
    <tbody bind:this={tbodyEl}>
      {#if loading && issues.length === 0}
        <SkeletonTable rows={10} cols={5} widths={['14%', '40%', '20%', '10%', '16%']} />
      {/if}
      {#each issues as i, idx (i.fingerprint)}
        <tr class="row" class:focused={idx === focusedIndex} on:click={() => (focusedIndex = idx)}>
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
    </tbody>
  </table>
</div>

{#if !loading && issues.length === 0}
  {@const hasFilters = !!(service || status)}
  <OnboardingEmpty kind="errors" filteredOut={hasFilters} />
{/if}

<style>
  tr.row.focused td {
    background: var(--bg-hover);
    box-shadow: inset 3px 0 0 var(--accent);
  }
</style>
