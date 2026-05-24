<script lang="ts">
  import { onMount, onDestroy, tick } from 'svelte';
  import { browser } from '$app/environment';
  import { fetchEvents, apiBase, type ProductEvent } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, selectedProject } from '$lib/stores';
  import { isTyping } from '$lib/keyboard';
  import { toast } from '$lib/toasts';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import EventVolumeHistogram from '$lib/components/EventVolumeHistogram.svelte';
  import EventDetailDrawer from '$lib/components/EventDetailDrawer.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';
  import SkeletonLogRows from '$lib/components/SkeletonLogRows.svelte';

  let events: ProductEvent[] = [];
  let eventName = '';
  let distinctId = '';
  let traceId = '';
  let source = '';
  let queryStr = '';
  /** Lista de pares `key:value` aplicados como filtros de properties. */
  let props: string[] = [];
  /** Buffer del input "+ propiedad" antes de empujar al array `props`. */
  let propKey = '';
  let propValue = '';

  let loading = false;
  let error = '';
  let live = false;
  let paused = false;
  let evtSource: EventSource | null = null;
  let pendingBacklog: ProductEvent[] = [];
  let selected: ProductEvent | null = null;
  let focusedIndex = -1;
  let listEl: HTMLDivElement | null = null;
  let subRange: { from: string; to: string } | null = null;

  const LIVE_BUFFER_MAX = 2000;
  const VIEW_MAX = 500;

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const base: Record<string, unknown> = {
        project: $selectedProject || undefined,
        event_name: eventName || undefined,
        distinct_id: distinctId || undefined,
        trace_id: traceId || undefined,
        source: source || undefined,
        query: queryStr || undefined,
        prop: props.length > 0 ? props : undefined,
        limit: 500
      };
      if (subRange) {
        base.from = subRange.from;
        base.to = subRange.to;
      } else {
        base.last_minutes = rangeMinutes($timeRange);
      }
      events = await fetchEvents(base);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function onHistogramSelection(e: CustomEvent<{ from: string; to: string } | null>): void {
    subRange = e.detail;
    if (subRange && live) stopLive();
    load();
  }

  function clearSubRange(): void {
    if (!subRange) return;
    subRange = null;
    load();
  }

  function stopLive(): void {
    evtSource?.close();
    evtSource = null;
    live = false;
    paused = false;
    pendingBacklog = [];
  }

  function buildLiveUrl(): string {
    const params = new URLSearchParams();
    if ($selectedProject) params.set('project', $selectedProject);
    if (eventName) params.set('event_name', eventName);
    if (distinctId) params.set('distinct_id', distinctId);
    if (traceId) params.set('trace_id', traceId);
    if (source) params.set('source', source);
    // `query` y `prop` no se aplican en el SSE (el filtro server-side de live
    // tail solo cubre las columnas baratas). El histograma y la lista histórica
    // sí los respetan.
    return `${apiBase()}/api/v1/events/live?${params.toString()}`;
  }

  function toggleLive(): void {
    if (live) {
      stopLive();
      return;
    }
    if (subRange) subRange = null;
    error = '';
    evtSource = new EventSource(buildLiveUrl());
    evtSource.addEventListener('event', (e) => {
      try {
        const row = JSON.parse((e as MessageEvent).data) as ProductEvent;
        if (paused) {
          pendingBacklog = [row, ...pendingBacklog].slice(0, LIVE_BUFFER_MAX);
        } else {
          events = [row, ...events].slice(0, LIVE_BUFFER_MAX);
        }
      } catch (err) {
        console.warn('evento inválido', err);
      }
    });
    evtSource.onerror = () => {
      console.warn('error de SSE');
    };
    live = true;
    paused = false;
    pendingBacklog = [];
  }

  function togglePause(): void {
    if (!live) return;
    if (paused) {
      if (pendingBacklog.length > 0) {
        events = [...pendingBacklog, ...events].slice(0, LIVE_BUFFER_MAX);
        pendingBacklog = [];
      }
      paused = false;
    } else {
      paused = true;
    }
  }

  function exportLastMinute(): void {
    const cutoff = Date.now() - 60_000;
    const recent = events.filter((r) => {
      const t = Date.parse(r.timestamp);
      return Number.isFinite(t) && t >= cutoff;
    });
    if (recent.length === 0) {
      toast.info('Sin eventos en el último minuto');
      return;
    }
    const blob = new Blob([JSON.stringify(recent, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    a.download = `faro-events-${stamp}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    toast.success(`Exportados ${recent.length} eventos`);
  }

  // ---------- Property filter chips ----------

  function addProp(): void {
    const k = propKey.trim();
    const v = propValue.trim();
    if (!k || !v) return;
    const entry = `${k}:${v}`;
    if (!props.includes(entry)) {
      props = [...props, entry];
      void load();
    }
    propKey = '';
    propValue = '';
  }

  function removeProp(p: string): void {
    props = props.filter((x) => x !== p);
    void load();
  }

  // ---------- URL state ----------

  function buildShareUrl(): string {
    if (!browser) return '';
    const params = new URLSearchParams();
    if ($selectedProject) params.set('project', $selectedProject);
    if (eventName) params.set('event_name', eventName);
    if (distinctId) params.set('distinct_id', distinctId);
    if (traceId) params.set('trace_id', traceId);
    if (source) params.set('source', source);
    if (queryStr) params.set('q', queryStr);
    for (const p of props) params.append('prop', p);
    if (live) params.set('live', '1');
    if ($timeRange) params.set('range', $timeRange);
    if (selected) params.set('selected', selected.timestamp);
    const qs = params.toString();
    return `${window.location.origin}${window.location.pathname}${qs ? '?' + qs : ''}`;
  }

  function syncSelectedToUrl(): void {
    if (!browser) return;
    const u = new URL(window.location.href);
    if (selected) u.searchParams.set('selected', selected.timestamp);
    else u.searchParams.delete('selected');
    try {
      window.history.replaceState(null, '', u.toString());
    } catch {
      /* ignora */
    }
  }
  $: if (browser) syncSelectedToUrl();

  async function shareView(): Promise<void> {
    const url = buildShareUrl();
    if (!url) return;
    try {
      window.history.replaceState(null, '', url);
    } catch {
      /* no bloqueante */
    }
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(url);
        toast.success('Enlace copiado al portapapeles');
        return;
      }
    } catch {
      /* fallback */
    }
    window.prompt('Copia este enlace:', url);
  }

  function applyUrlParams(): void {
    if (!browser) return;
    const p = new URLSearchParams(window.location.search);
    const proj = p.get('project');
    if (proj && proj !== $selectedProject) selectedProject.set(proj);
    eventName = p.get('event_name') ?? eventName;
    distinctId = p.get('distinct_id') ?? distinctId;
    traceId = p.get('trace_id') ?? traceId;
    source = p.get('source') ?? source;
    queryStr = p.get('q') ?? queryStr;
    const all = p.getAll('prop').filter((s) => s.includes(':'));
    if (all.length > 0) props = all;
    const range = p.get('range');
    if (range) {
      const presets = ['5m', '15m', '1h', '6h', '24h', '7d'] as const;
      if ((presets as readonly string[]).includes(range)) {
        timeRange.set(range as typeof presets[number]);
      }
    }
  }

  function applySelectedFromUrl(): void {
    if (!browser) return;
    const sel = new URLSearchParams(window.location.search).get('selected');
    if (!sel) return;
    const idx = events.findIndex((l) => l.timestamp === sel);
    if (idx >= 0) {
      focusedIndex = idx;
      selected = events[idx];
      void ensureFocusedVisible();
    }
  }

  $: drawerPosition = (() => {
    if (!selected) return null;
    const visible = events.slice(0, VIEW_MAX);
    const idx = visible.indexOf(selected);
    if (idx < 0) return null;
    return { index: idx, total: visible.length };
  })();

  function onDrawerFilter(
    e: CustomEvent<{
      key: 'event_name' | 'distinct_id' | 'session_id' | 'trace_id' | 'source' | 'prop';
      value: string;
    }>
  ): void {
    const { key, value } = e.detail;
    if (key === 'event_name') eventName = value;
    else if (key === 'distinct_id') distinctId = value;
    else if (key === 'session_id') {
      // session_id no es un input de primera clase; lo metemos como un filtro
      // de propiedad para no perderlo aunque no haya un input dedicado.
      const entry = `session_id:${value}`;
      if (!props.includes(entry)) props = [...props, entry];
    } else if (key === 'trace_id') traceId = value;
    else if (key === 'source') source = value;
    else if (key === 'prop') {
      if (!props.includes(value)) props = [...props, value];
    }
    void load();
  }

  async function ensureFocusedVisible(): Promise<void> {
    if (focusedIndex < 0 || !listEl) return;
    await tick();
    const el = listEl.querySelectorAll<HTMLElement>('.evt-row')[focusedIndex];
    el?.scrollIntoView({ block: 'nearest' });
  }

  function onKeydown(e: KeyboardEvent): void {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isTyping(e.target)) return;

    const visible = Math.min(events.length, VIEW_MAX);

    if (e.key === 'Escape') {
      if (selected) return;
      if (focusedIndex >= 0) {
        e.preventDefault();
        focusedIndex = -1;
      }
      return;
    }
    if (e.key === 'j' || e.key === 'ArrowDown') {
      if (visible === 0) return;
      e.preventDefault();
      focusedIndex = Math.min(visible - 1, Math.max(0, focusedIndex + 1));
      if (selected) selected = events[focusedIndex];
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'k' || e.key === 'ArrowUp') {
      if (visible === 0) return;
      e.preventDefault();
      focusedIndex = Math.max(0, focusedIndex - 1);
      if (selected) selected = events[focusedIndex];
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'Enter') {
      if (focusedIndex >= 0 && focusedIndex < visible) {
        e.preventDefault();
        selected = events[focusedIndex];
      }
    }
  }

  // Distinct_id resumido para tabla — 12 char + ellipsis si es largo.
  function shortId(s: string | undefined): string {
    if (!s) return '—';
    return s.length > 14 ? s.slice(0, 12) + '…' : s;
  }

  // Snippet de properties para la fila — primeras 2-3 keys con su valor primitivo.
  function propsSummary(raw: string): string {
    if (!raw) return '';
    try {
      const v = JSON.parse(raw);
      if (v === null || typeof v !== 'object' || Array.isArray(v)) return '';
      const entries = Object.entries(v as Record<string, unknown>).slice(0, 3);
      return entries
        .map(([k, val]) => {
          const stringified =
            val === null || typeof val !== 'object' ? String(val) : '{…}';
          return `${k}=${stringified}`;
        })
        .join(' · ');
    } catch {
      return '';
    }
  }

  onMount(async () => {
    applyUrlParams();
    await load();
    applySelectedFromUrl();
    if (browser) {
      const p = new URLSearchParams(window.location.search);
      if (p.get('live') === '1') toggleLive();
      window.addEventListener('keydown', onKeydown);
    }
  });
  onDestroy(() => {
    evtSource?.close();
    if (browser) window.removeEventListener('keydown', onKeydown);
  });

  let prevRange = $timeRange;
  let prevProject = $selectedProject;
  $: {
    if (prevRange !== $timeRange || prevProject !== $selectedProject) {
      prevRange = $timeRange;
      prevProject = $selectedProject;
      subRange = null;
      load();
    }
  }
</script>

<div class="page-header">
  <h1 class="page-title">Eventos</h1>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button class:primary={live} on:click={toggleLive} disabled={!!subRange && !live} title={subRange ? 'Limpia el sub-rango para activar el tail' : ''}>
      {#if live}<span class="live-dot" class:paused></span> {paused ? 'Pausado' : 'En vivo'}{:else}▶ Activar tail{/if}
    </button>
    {#if live}
      <button on:click={togglePause} title="Pausa el render para revisar lo que ya llegó">
        {#if paused}
          ▶ Reanudar{#if pendingBacklog.length > 0} ({pendingBacklog.length}){/if}
        {:else}
          ⏸ Pausar
        {/if}
      </button>
      <button on:click={exportLastMinute} title="Descarga JSON con los eventos del último minuto">
        ⇩ Exportar 1 min
      </button>
    {/if}
    <button on:click={shareView} title="Copia un enlace con la vista actual">
      🔗 Compartir
    </button>
  </div>
</div>

<EventVolumeHistogram
  lastMinutes={rangeMinutes($timeRange)}
  eventName={eventName || undefined}
  project={$selectedProject || undefined}
  selection={subRange}
  on:selectionchange={onHistogramSelection}
/>

<div class="toolbar">
  <input placeholder="event_name" bind:value={eventName} on:change={load} class="mono" style="width: 180px" />
  <input placeholder="distinct_id" bind:value={distinctId} on:change={load} class="mono" style="width: 180px" />
  <input placeholder="trace_id" bind:value={traceId} on:change={load} class="mono" style="width: 180px" />
  <select bind:value={source} on:change={load} style="width: 110px">
    <option value="">Cualquier source</option>
    <option value="web">web</option>
    <option value="mobile">mobile</option>
    <option value="server">server</option>
  </select>
  <input
    placeholder="Buscar en properties… (substring)"
    bind:value={queryStr}
    on:keydown={(e) => e.key === 'Enter' && load()}
    style="flex: 1; min-width: 200px;"
    data-search-input
  />
  <button on:click={load}>{loading ? 'Cargando…' : 'Buscar'}</button>
  {#if subRange}
    <button class="danger" on:click={clearSubRange} title="Volver a ver todo el rango">Limpiar sub-rango</button>
  {/if}
</div>

<div class="toolbar prop-bar">
  <span class="muted" style="font-size: 11.5px;">properties.</span>
  <input placeholder="key" bind:value={propKey} on:keydown={(e) => e.key === 'Enter' && addProp()} class="mono" style="width: 140px" />
  <span class="muted">=</span>
  <input placeholder="value" bind:value={propValue} on:keydown={(e) => e.key === 'Enter' && addProp()} class="mono" style="width: 200px" />
  <button on:click={addProp} disabled={!propKey.trim() || !propValue.trim()} title="Añadir filtro de propiedad">+ Filtro</button>
  {#each props as p}
    <span class="prop-chip mono">
      {p}
      <button on:click={() => removeProp(p)} aria-label="Quitar filtro" title="Quitar">×</button>
    </span>
  {/each}
</div>

{#if error}<div style="color: var(--danger);">Error: {error}</div>{/if}

<div bind:this={listEl} style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <div style="padding: 8px 12px; background: var(--bg); border-bottom: 1px solid var(--border); font-size: 12px; color: var(--text-muted); display: grid; grid-template-columns: 180px 200px 160px 80px 60px 1fr; gap: 12px;">
    <div>Hora</div><div>Evento</div><div>distinct_id</div><div>source</div><div>traza</div><div>Properties (preview)</div>
  </div>
  {#if loading && events.length === 0}
    <SkeletonLogRows rows={12} />
  {/if}
  {#each events.slice(0, VIEW_MAX) as row, i (row.event_id || row.timestamp + row.event_name + row.distinct_id)}
    <div
      class="evt-row"
      class:focused={i === focusedIndex}
      on:click={() => { focusedIndex = i; selected = row; }}
      on:keypress={(e) => e.key === 'Enter' && (selected = row)}
      tabindex="0"
      role="button"
    >
      <div class="ts mono">{formatTimestamp(row.timestamp)}</div>
      <div class="mono evt-name" title={row.event_name}>{row.event_name}</div>
      <div class="muted mono" title={row.distinct_id}>{shortId(row.distinct_id)}</div>
      <div class="muted mono">{row.source || '—'}</div>
      <div class="mono">
        {#if row.trace_id}
          <span title={row.trace_id} style="color: var(--accent);">●</span>
        {:else}
          <span class="muted">–</span>
        {/if}
      </div>
      <div class="muted mono evt-props">{propsSummary(row.properties)}</div>
    </div>
  {/each}
</div>

{#if !loading && events.length === 0}
  {@const hasFilters = !!(eventName || distinctId || traceId || source || queryStr || props.length > 0)}
  <OnboardingEmpty kind="events" filteredOut={hasFilters} />
{/if}

<EventDetailDrawer
  event={selected}
  position={drawerPosition}
  on:close={() => (selected = null)}
  on:filter={onDrawerFilter}
/>

<style>
  .live-dot.paused {
    background: var(--text-muted);
    box-shadow: none;
    animation: none;
  }

  :global(.evt-row) {
    display: grid;
    grid-template-columns: 180px 200px 160px 80px 60px 1fr;
    gap: 12px;
    padding: 6px 12px;
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
    cursor: pointer;
    align-items: center;
  }
  :global(.evt-row:last-child) { border-bottom: 0; }
  :global(.evt-row:hover) { background: var(--bg-hover); }
  :global(.evt-row.focused) {
    background: var(--bg-hover);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .evt-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .evt-props {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
  }

  .prop-bar {
    margin-top: 8px;
    flex-wrap: wrap;
  }
  .prop-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 12px;
    padding: 1px 4px 1px 8px;
    font-size: 11.5px;
    color: var(--text);
  }
  .prop-chip button {
    background: transparent;
    border: 0;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 13px;
    line-height: 1;
    padding: 0 4px;
  }
  .prop-chip button:hover { color: var(--danger); }
</style>
