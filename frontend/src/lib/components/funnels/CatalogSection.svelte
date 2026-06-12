<script lang="ts">
  import Skeleton from '$lib/components/Skeleton.svelte';
  import { filterCatalog, fmtCount } from '$lib/funnels';
  import type { EventCandidate } from '$lib/api';

  export let catalog: EventCandidate[];
  export let loading: boolean;
  export let error: string;
  export let filter: string;

  export let onFilterChange: (v: string) => void;
  export let onAdd: (name: string) => void;
  export let onDragStart: (e: DragEvent, name: string) => void;
  export let onDragOver: (e: DragEvent) => void;
  export let onDrop: (e: DragEvent) => void;

  $: filteredCatalog = filterCatalog(catalog, filter);
</script>

<aside
  class="pane catalog"
  on:dragover={onDragOver}
  on:drop={onDrop}
  role="list"
  aria-label="Eventos disponibles"
>
  <h2 class="pane-title">Eventos</h2>
  <input
    type="search"
    placeholder="Filtrar…"
    value={filter}
    on:input={(e) => onFilterChange(e.currentTarget.value)}
    aria-label="Filtrar eventos"
  />
  {#if loading}
    <div class="skel-col">
      {#each Array(6) as _}
        <Skeleton width="100%" height="28px" radius="6px" />
      {/each}
    </div>
  {:else if error}
    <div class="error">{error}</div>
  {:else if filteredCatalog.length === 0}
    <div class="muted empty">
      {catalog.length === 0
        ? 'No hay eventos en el rango. Probá ampliar el rango temporal o disparar product.track() desde un SDK.'
        : `Ningún evento coincide con "${filter}".`}
    </div>
  {:else}
    <ul class="event-list">
      {#each filteredCatalog as ev (ev.name)}
        <li>
          <button
            type="button"
            class="event-chip"
            draggable="true"
            on:dragstart={(e) => onDragStart(e, ev.name)}
            on:dblclick={() => onAdd(ev.name)}
            title="Doble click para agregar al funnel, o arrastrá"
          >
            <span class="event-name">{ev.name}</span>
            <span class="event-count mono">{fmtCount(ev.count)}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}
</aside>

<style>
  .catalog input[type="search"] { width: 100%; }
  .event-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 60vh;
    overflow-y: auto;
  }
  .event-chip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    color: var(--text);
    cursor: grab;
    text-align: left;
    font-size: 13px;
  }
  .event-chip:hover { background: var(--bg-hover); border-color: var(--accent-dim); }
  .event-chip:active { cursor: grabbing; }
  .event-name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .event-count { color: var(--text-muted); font-size: 11px; flex-shrink: 0; }
</style>
