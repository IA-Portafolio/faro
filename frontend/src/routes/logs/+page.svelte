<script lang="ts">
  /**
   * Página `/logs` — explorador de logs.
   *
   * Lista logs con filtros por servicio, severidad mínima, texto/regex y `trace_id`,
   * con paginación por cursor keyset. Soporta modo "live" por SSE (EventSource) con
   * buffer y pausa, el histograma de volumen por severidad con drag-to-select de
   * sub-rango, y un drawer de detalle. Navegación por teclado j/k y `/` para buscar.
   */
  import { onMount, onDestroy, tick } from 'svelte';
  import { browser } from '$app/environment';
  import { fetchLogs, apiBase, type LogRow } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, selectedProject } from '$lib/stores';
  import { isTyping } from '$lib/keyboard';
  import { toast } from '$lib/toasts';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import SeverityBadge from '$lib/components/SeverityBadge.svelte';
  import LogVolumeHistogram from '$lib/components/LogVolumeHistogram.svelte';
  import LogDetailDrawer from '$lib/components/LogDetailDrawer.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';
  import SkeletonLogRows from '$lib/components/SkeletonLogRows.svelte';

  let logs: LogRow[] = [];
  let service = '';
  let minSeverity = 0;
  let queryStr = '';
  let traceId = '';
  let useRegex = false;
  let loading = false;
  let error = '';
  let live = false;
  let paused = false;
  let evtSource: EventSource | null = null;
  let pendingBacklog: LogRow[] = [];
  let selected: LogRow | null = null;
  let focusedIndex = -1;
  let listEl: HTMLDivElement | null = null;
  let subRange: { from: string; to: string } | null = null;
  let highlightRe: RegExp | null = null;
  let regexError = '';

  // Tope del buffer en vivo. Más grande que la lista visible para que el export
  // del último minuto siempre tenga datos aunque lleguen ráfagas.
  const LIVE_BUFFER_MAX = 2000;
  const VIEW_MAX = 500;

  function escapeHtml(s: string): string {
    return s.replace(/[&<>"']/g, (c) => {
      switch (c) {
        case '&': return '&amp;';
        case '<': return '&lt;';
        case '>': return '&gt;';
        case '"': return '&quot;';
        case "'": return '&#39;';
        default: return c;
      }
    });
  }

  function escapeRegex(s: string): string {
    return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  }

  function recompileHighlight(): void {
    regexError = '';
    if (!queryStr) {
      highlightRe = null;
      return;
    }
    try {
      const pattern = useRegex ? queryStr : escapeRegex(queryStr);
      highlightRe = new RegExp(pattern, 'gi');
    } catch (e) {
      regexError = e instanceof Error ? e.message : String(e);
      highlightRe = null;
    }
  }

  // Hace el resaltado seguro: itera el body crudo con `matchAll` y va
  // emitiendo HTML escapado intercalado con <mark>. Evita resaltar sobre
  // texto ya escapado, donde `&` se convierte en `&amp;` y un patrón
  // como `&` matcheearía donde no debe.
  function highlightBody(body: string | null | undefined): string {
    // Aunque el contrato del SDK promete `string`, payloads malformados
    // (e.g. un ingester antiguo que omite el campo) llegarían como undefined
    // y reventarían `matchAll`. Coercionar a '' es preferible a fallar el render.
    const s = body ?? '';
    if (!highlightRe) return escapeHtml(s);
    let out = '';
    let lastEnd = 0;
    for (const m of s.matchAll(highlightRe)) {
      const idx = m.index ?? 0;
      const text = m[0];
      out += escapeHtml(s.slice(lastEnd, idx));
      out += `<mark>${escapeHtml(text)}</mark>`;
      lastEnd = idx + text.length;
      // matchAll con regex global ya avanza correctamente — no hace falta
      // tocar lastIndex como con la API imperativa.
      if (text.length === 0) lastEnd += 1;
    }
    out += escapeHtml(s.slice(lastEnd));
    return out;
  }

  $: queryStr, useRegex, recompileHighlight();

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const base: Record<string, unknown> = {
        project: $selectedProject || undefined,
        service: service || undefined,
        min_severity: minSeverity || undefined,
        query: queryStr || undefined,
        trace_id: traceId || undefined,
        limit: 500
      };
      if (subRange) {
        base.from = subRange.from;
        base.to = subRange.to;
      } else {
        base.last_minutes = rangeMinutes($timeRange);
      }
      logs = await fetchLogs(base);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function onHistogramSelection(e: CustomEvent<{ from: string; to: string } | null>): void {
    subRange = e.detail;
    if (subRange && live) {
      // El live tail no tiene sentido sobre un sub-rango congelado — se detiene.
      stopLive();
    }
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
    if (service) params.set('service', service);
    if (minSeverity) params.set('min_severity', String(minSeverity));
    if (queryStr) params.set('query', queryStr);
    if (useRegex) params.set('regex', 'true');
    return `${apiBase()}/api/v1/logs/live?${params.toString()}`;
  }

  function toggleLive(): void {
    if (live) {
      stopLive();
      return;
    }
    if (subRange) {
      // Limpia el sub-rango al iniciar el live tail.
      subRange = null;
    }
    if (useRegex && regexError) {
      error = `Regex inválida: ${regexError}`;
      return;
    }
    error = '';
    evtSource = new EventSource(buildLiveUrl());
    evtSource.addEventListener('log', (e) => {
      try {
        const row = JSON.parse((e as MessageEvent).data) as LogRow;
        if (paused) {
          pendingBacklog = [row, ...pendingBacklog].slice(0, LIVE_BUFFER_MAX);
        } else {
          logs = [row, ...logs].slice(0, LIVE_BUFFER_MAX);
        }
      } catch (err) {
        console.warn('evento inválido', err);
      }
    });
    evtSource.onerror = () => {
      // El navegador se reconecta solo; solo dejamos rastro en consola.
      console.warn('error de SSE');
    };
    live = true;
    paused = false;
    pendingBacklog = [];
  }

  function togglePause(): void {
    if (!live) return;
    if (paused) {
      // Reanudar: prepende lo que se acumuló mientras estaba pausado.
      if (pendingBacklog.length > 0) {
        logs = [...pendingBacklog, ...logs].slice(0, LIVE_BUFFER_MAX);
        pendingBacklog = [];
      }
      paused = false;
    } else {
      paused = true;
    }
  }

  function exportLastMinute(): void {
    const cutoff = Date.now() - 60_000;
    const recent = logs.filter((r) => {
      const t = Date.parse(r.timestamp);
      return Number.isFinite(t) && t >= cutoff;
    });
    if (recent.length === 0) {
      toast.info('Sin logs en el último minuto');
      return;
    }
    const blob = new Blob([JSON.stringify(recent, null, 2)], { type: 'application/json' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    const stamp = new Date().toISOString().replace(/[:.]/g, '-');
    a.download = `faro-logs-${stamp}.json`;
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
    toast.success(`Exportados ${recent.length} logs`);
  }

  function buildShareUrl(): string {
    if (!browser) return '';
    const params = new URLSearchParams();
    if ($selectedProject) params.set('project', $selectedProject);
    if (service) params.set('service', service);
    if (minSeverity) params.set('min_severity', String(minSeverity));
    if (queryStr) params.set('q', queryStr);
    if (useRegex) params.set('regex', '1');
    if (traceId) params.set('trace_id', traceId);
    if (live) params.set('live', '1');
    if ($timeRange) params.set('range', $timeRange);
    if (selected) params.set('selected', selected.timestamp);
    const qs = params.toString();
    return `${window.location.origin}${window.location.pathname}${qs ? '?' + qs : ''}`;
  }

  /** Reescribe `?selected=…` en la URL actual sin tocar el resto del query string. */
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
  $: if (browser) syncSelectedToUrl(); // reacciona a cada cambio de `selected`

  async function shareView(): Promise<void> {
    const url = buildShareUrl();
    if (!url) return;
    // Mantiene la URL del navegador sincronizada para que un reload sea idempotente.
    try {
      window.history.replaceState(null, '', url);
    } catch {
      // No bloqueante si el navegador rechaza replaceState.
    }
    try {
      if (navigator.clipboard?.writeText) {
        await navigator.clipboard.writeText(url);
        toast.success('Enlace copiado al portapapeles');
        return;
      }
    } catch {
      // Fallback abajo.
    }
    window.prompt('Copia este enlace:', url);
  }

  function applyUrlParams(): void {
    if (!browser) return;
    const p = new URLSearchParams(window.location.search);
    const proj = p.get('project');
    if (proj && proj !== $selectedProject) selectedProject.set(proj);
    service = p.get('service') ?? service;
    const minSev = p.get('min_severity');
    if (minSev) minSeverity = Number(minSev) || 0;
    queryStr = p.get('q') ?? queryStr;
    useRegex = p.get('regex') === '1' || p.get('regex') === 'true';
    traceId = p.get('trace_id') ?? traceId;
    const range = p.get('range');
    if (range) {
      const presets = ['5m', '15m', '1h', '6h', '24h', '7d'] as const;
      if ((presets as readonly string[]).includes(range)) {
        timeRange.set(range as typeof presets[number]);
      }
    }
  }

  /** Resuelve `?selected=<timestamp>` contra el lote actual. */
  function applySelectedFromUrl(): void {
    if (!browser) return;
    const sel = new URLSearchParams(window.location.search).get('selected');
    if (!sel) return;
    const idx = logs.findIndex((l) => l.timestamp === sel);
    if (idx >= 0) {
      focusedIndex = idx;
      selected = logs[idx];
      void ensureFocusedVisible();
    }
  }

  /** Para el drawer: ¿posición del seleccionado en la lista visible? */
  $: drawerPosition = (() => {
    if (!selected) return null;
    const visible = logs.slice(0, VIEW_MAX);
    const idx = visible.indexOf(selected);
    if (idx < 0) return null;
    return { index: idx, total: visible.length };
  })();

  /** Handler: la sección "Logs ±2min" pidió saltar a otro log. */
  function onDrawerJump(e: CustomEvent<{ timestamp: string }>): void {
    const idx = logs.findIndex((l) => l.timestamp === e.detail.timestamp);
    if (idx >= 0) {
      focusedIndex = idx;
      selected = logs[idx];
      void ensureFocusedVisible();
    } else {
      // No estaba cargado en la lista actual: amplia el rango cargando logs alrededor.
      // Para no complicar, simplemente abrimos /logs con ese trace o timestamp.
      // (En un siguiente paso podríamos hacer fetch puntual.)
      toast.warning('Ese log no está en el listado actual — amplía el rango.');
    }
  }

  /** Handler: el drawer pidió aplicar un filtro. */
  function onDrawerFilter(e: CustomEvent<{ key: 'service' | 'query' | 'trace_id'; value: string }>): void {
    if (e.detail.key === 'service') service = e.detail.value;
    if (e.detail.key === 'query') queryStr = e.detail.value;
    if (e.detail.key === 'trace_id') traceId = e.detail.value;
    void load();
  }

  async function ensureFocusedVisible(): Promise<void> {
    if (focusedIndex < 0 || !listEl) return;
    await tick();
    const el = listEl.querySelectorAll<HTMLElement>('.log-row')[focusedIndex];
    el?.scrollIntoView({ block: 'nearest' });
  }

  function onKeydown(e: KeyboardEvent): void {
    // Cmd/Ctrl + K, /, ?, g+x → los gestiona el handler global.
    // El drawer gestiona Esc y Cmd+C cuando el foco está dentro de él.
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isTyping(e.target)) return;

    const visible = Math.min(logs.length, VIEW_MAX);

    if (e.key === 'Escape') {
      // Si el drawer está abierto, su propio listener lo cierra. Aquí solo
      // gestionamos quitar la selección cuando no hay drawer.
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
      // Si el drawer está abierto, navegar mueve también el seleccionado
      // — esa es la magia que pide la decisión 1 ("persistente entre clicks").
      if (selected) selected = logs[focusedIndex];
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'k' || e.key === 'ArrowUp') {
      if (visible === 0) return;
      e.preventDefault();
      focusedIndex = Math.max(0, focusedIndex - 1);
      if (selected) selected = logs[focusedIndex];
      void ensureFocusedVisible();
      return;
    }
    if (e.key === 'Enter') {
      if (focusedIndex >= 0 && focusedIndex < visible) {
        e.preventDefault();
        selected = logs[focusedIndex];
      }
    }
  }

  onMount(async () => {
    applyUrlParams();
    await load();
    // Aplica `?selected=…` después del primer fetch, cuando ya hay logs en
    // memoria sobre los que buscar por timestamp.
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

  // Recarga al cambiar rango / proyecto, y resetea cualquier selección de sub-rango
  // activa para que no se arrastre entre rangos.
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
  <h1 class="page-title">Logs</h1>
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
      <button on:click={exportLastMinute} title="Descarga JSON con los logs del último minuto">
        ⇩ Exportar 1 min
      </button>
    {/if}
    <button on:click={shareView} title="Copia un enlace con la vista actual">
      🔗 Compartir
    </button>
  </div>
</div>

<LogVolumeHistogram
  lastMinutes={rangeMinutes($timeRange)}
  service={service || undefined}
  minSeverity={minSeverity || undefined}
  query={queryStr || undefined}
  traceId={traceId || undefined}
  project={$selectedProject || undefined}
  selection={subRange}
  on:selectionchange={onHistogramSelection}
/>

<div class="toolbar">
  <input placeholder="Servicio" bind:value={service} on:change={load} style="width: 180px" />
  <select bind:value={minSeverity} on:change={load}>
    <option value={0}>Cualquier severidad</option>
    <option value={5}>DEBUG y superior</option>
    <option value={9}>INFO y superior</option>
    <option value={13}>WARN y superior</option>
    <option value={17}>ERROR y superior</option>
  </select>
  <input
    placeholder={useRegex ? 'Regex (case-insensitive)…' : 'Buscar en el mensaje… (pulsa /)'}
    bind:value={queryStr}
    on:keydown={(e) => e.key === 'Enter' && load()}
    class:invalid={useRegex && !!regexError}
    style="flex: 1; min-width: 200px;"
    data-search-input
  />
  <label class="regex-toggle" title="Interpreta el filtro como expresión regular">
    <input type="checkbox" bind:checked={useRegex} on:change={() => { if (live) { stopLive(); toggleLive(); } else { load(); } }} />
    <span>.*</span>
  </label>
  <input placeholder="ID de traza" bind:value={traceId} on:keydown={(e) => e.key === 'Enter' && load()} class="mono" style="width: 200px;" />
  <button on:click={load}>{loading ? 'Cargando…' : 'Buscar'}</button>
  {#if subRange}
    <button class="danger" on:click={clearSubRange} title="Volver a ver todo el rango">Limpiar sub-rango</button>
  {/if}
</div>

{#if useRegex && regexError}
  <div class="regex-error">Regex inválida: {regexError}</div>
{/if}
{#if error}<div style="color: var(--danger);">Error: {error}</div>{/if}

<div bind:this={listEl} style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <div style="padding: 8px 12px; background: var(--bg); border-bottom: 1px solid var(--border); font-size: 12px; color: var(--text-muted); display: grid; grid-template-columns: 180px 80px 140px 1fr; gap: 12px;">
    <div>Hora</div><div>Severidad</div><div>Servicio</div><div>Mensaje</div>
  </div>
  {#if loading && logs.length === 0}
    <SkeletonLogRows rows={12} />
  {/if}
  {#each logs.slice(0, VIEW_MAX) as row, i (row.timestamp + row.body)}
    <div
      class="log-row"
      class:focused={i === focusedIndex}
      on:click={() => { focusedIndex = i; selected = row; }}
      on:keypress={(e) => e.key === 'Enter' && (selected = row)}
      tabindex="0"
      role="button"
    >
      <div class="ts mono">{formatTimestamp(row.timestamp)}</div>
      <div><SeverityBadge severity={row.severity_text} /></div>
      <div class="muted mono">{row.service_name}</div>
      <div class="body">{@html highlightBody(row.body)}</div>
    </div>
  {/each}
</div>

{#if !loading && logs.length === 0}
  {@const hasFilters = !!(service || minSeverity || queryStr || traceId)}
  <OnboardingEmpty kind="logs" filteredOut={hasFilters} />
{/if}

<LogDetailDrawer
  log={selected}
  position={drawerPosition}
  on:close={() => (selected = null)}
  on:jump={onDrawerJump}
  on:filter={onDrawerFilter}
/>

<style>
  .regex-toggle {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg-elev);
    font-family: "JetBrains Mono", Menlo, monospace;
    font-size: 12px;
    cursor: pointer;
    user-select: none;
  }
  .regex-toggle input { margin: 0; cursor: pointer; }

  .regex-error {
    color: var(--danger);
    font-size: 12px;
    margin-top: -4px;
    margin-bottom: 8px;
  }

  input.invalid {
    border-color: var(--danger);
  }

  .live-dot.paused {
    background: var(--text-muted);
    box-shadow: none;
    animation: none;
  }

  /* Resaltado de matches dentro del body. Se aplica vía {@html} sobre HTML escapado. */
  .log-row .body :global(mark) {
    background: rgba(250, 204, 21, 0.35);
    color: inherit;
    padding: 0 2px;
    border-radius: 2px;
  }

  :global(.log-row.focused) {
    background: var(--bg-hover);
    box-shadow: inset 3px 0 0 var(--accent);
  }
</style>
