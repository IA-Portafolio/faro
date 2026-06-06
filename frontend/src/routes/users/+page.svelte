<script lang="ts">
  /**
   * Página `/users` — lista de usuarios de producto (product analytics).
   *
   * Lista los `ProductUserSummary` del proyecto/rango (`fetchProductUsers`) con
   * búsqueda y filtro por `source`, y salto al detalle `/users/<distinct_id>`. OJO:
   * son usuarios finales identificados por `distinct_id`, distintos de los usuarios
   * del workspace de `/settings/users`.
   */
  import { onMount } from 'svelte';
  import { fetchProductUsers, type ProductUserSummary } from '$lib/api';
  import { buildProductUserHref, propertiesPreview, shortProductId } from '$lib/product-users';
  import { formatTimestamp, rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import SkeletonLogRows from '$lib/components/SkeletonLogRows.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';

  let users: ProductUserSummary[] = [];
  let loading = false;
  let error = '';
  let query = '';
  let source = '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      users = await fetchProductUsers({
        project: $selectedProject || undefined,
        last_minutes: rangeMinutes($timeRange),
        query: query || undefined,
        source: source || undefined,
        limit: 500
      });
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      users = [];
    } finally {
      loading = false;
    }
  }

  function userHref(id: string): string {
    return buildProductUserHref(id, {
      project: $selectedProject || undefined,
      range: $timeRange
    });
  }

  let prevProject = $selectedProject;
  let prevRange = $timeRange;
  $: if (prevProject !== $selectedProject || prevRange !== $timeRange) {
    prevProject = $selectedProject;
    prevRange = $timeRange;
    void load();
  }

  onMount(load);
</script>

<div class="page-header">
  <div>
    <h1 class="page-title">Usuarios</h1>
    <div class="muted subtitle">End-users del producto capturados por SDKs cliente.</div>
  </div>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button on:click={load} disabled={loading}>{loading ? 'Cargando...' : 'Recargar'}</button>
  </div>
</div>

<div class="toolbar">
  <input
    placeholder="Buscar distinct_id o properties..."
    bind:value={query}
    on:keydown={(e) => e.key === 'Enter' && load()}
    style="min-width: 260px;"
    data-search-input
  />
  <select bind:value={source} on:change={load}>
    <option value="">Cualquier source</option>
    <option value="web">web</option>
    <option value="mobile">mobile</option>
    <option value="server">server</option>
    <option value="backend">backend</option>
  </select>
  <button on:click={load} disabled={loading}>Buscar</button>
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

<div class="users-table">
  <div class="users-head">
    <div>Usuario</div>
    <div>Last seen</div>
    <div>First seen</div>
    <div>Eventos</div>
    <div>Sources</div>
    <div>Anon IDs</div>
    <div>Properties</div>
  </div>

  {#if loading && users.length === 0}
    <SkeletonLogRows rows={10} />
  {:else}
    {#each users as user (user.project_id + ':' + user.distinct_id)}
      <a class="user-row" href={userHref(user.distinct_id)} data-sveltekit-preload-data="hover">
        <div class="mono user-id" title={user.distinct_id}>{shortProductId(user.distinct_id)}</div>
        <div class="mono muted">{formatTimestamp(user.last_seen)}</div>
        <div class="mono muted">{formatTimestamp(user.first_seen)}</div>
        <div class="mono tabular">{user.event_count.toLocaleString()}</div>
        <div class="chips">
          {#each user.sources as s}
            <span class="chip mono">{s}</span>
          {/each}
        </div>
        <div class="mono muted">{user.anonymous_ids.length}</div>
        <div class="mono props" title={propertiesPreview(user.properties)}>
          {propertiesPreview(user.properties)}
        </div>
      </a>
    {/each}
  {/if}
</div>

{#if !loading && users.length === 0}
  <OnboardingEmpty kind="events" filteredOut={!!(query || source)} />
{/if}

<style>
  .subtitle { font-size: 12px; margin-top: 2px; }

  .users-table {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .users-head,
  .user-row {
    display: grid;
    grid-template-columns: minmax(170px, 1.2fr) 180px 180px 90px minmax(120px, 0.8fr) 80px minmax(180px, 1.4fr);
    gap: 12px;
    align-items: center;
  }
  .users-head {
    padding: 8px 12px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
  }
  .user-row {
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    color: var(--text);
    text-decoration: none;
    font-size: 12.5px;
  }
  .user-row:hover {
    background: var(--bg-hover);
    text-decoration: none;
  }
  .user-row:last-child { border-bottom: 0; }
  .user-id,
  .props {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .chips {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .chip {
    border: 1px solid var(--border);
    background: var(--bg);
    border-radius: 10px;
    padding: 1px 7px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .error-box {
    color: var(--danger);
    padding: 10px 12px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
  }
  @media (max-width: 1000px) {
    .users-head { display: none; }
    .user-row {
      grid-template-columns: 1fr;
      gap: 4px;
    }
  }
</style>
