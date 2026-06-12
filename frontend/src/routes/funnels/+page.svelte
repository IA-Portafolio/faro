<script lang="ts">
  /**
   * Página `/funnels` — constructor de embudos de conversión.
   *
   * Composition root: mantiene el estado compartido (catálogo, funnel, caches
   * de drop-off/timing, debounce, race-condition guards con `reqSeq` y
   * `funnelVersion`) y delega el render a 3 subcomponentes presentacionales
   * (`CatalogSection`, `BuilderSection`, `ResultsSection`) en
   * `$lib/components/funnels/`. Helpers puros en `$lib/funnels`.
   */
  import { onDestroy, onMount } from 'svelte';
  import { browser } from '$app/environment';

  import {
    fetchFunnelEvents,
    previewDropOff,
    computeFunnel,
    previewTimeToConvert,
    type DropOffResult,
    type EventCandidate,
    type FunnelResult,
    type TimeToConvertResult
  } from '$lib/api';
  import { rangeMinutes, selectedProject, timeRange } from '$lib/stores';
  import type { StepView } from '$lib/funnels';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import CatalogSection from '$lib/components/funnels/CatalogSection.svelte';
  import BuilderSection from '$lib/components/funnels/BuilderSection.svelte';
  import ResultsSection from '$lib/components/funnels/ResultsSection.svelte';

  // ---------- Estado: catálogo + funnel en edición ----------
  let catalog: EventCandidate[] = [];
  let catalogError = '';
  let catalogLoading = true;

  /** Lista ordenada de event_name que arma el funnel (mín 2 para que corra). */
  let funnel: string[] = [];
  /** Texto del filtro del catálogo. */
  let filter = '';
  /** Segundos. 1d default; rango de presets sólido para exploración. */
  let windowSecs = 86_400;

  // ---------- Estado: resultado live ----------
  let result: FunnelResult | null = null;
  let previewError = '';
  let previewLoading = false;
  /** Generación monotónica para descartar respuestas fuera de orden. */
  let reqSeq = 0;
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;

  // ---------- Estado: paneles por-paso (drop-off + timing) ----------
  /** Cuál paso está actualmente abierto (-1 = ninguno). Sólo uno a la vez para no
   *  saturar la UI con N paneles. */
  let expandedStep = -1;
  /** Qué insight muestra el panel cuando está abierto. */
  let expandedView: StepView = 'dropoff';

  /** Look-ahead post drop-off en segundos. Default 300 (5 min) según el goal D.2. */
  let lookaheadSecs = 300;
  /** Cache drop-off por paso. Se invalida cuando cambia funnel/window/rango/proyecto. */
  let dropOffByStep: Record<number, DropOffResult> = {};
  let dropOffErrors: Record<number, string> = {};
  let dropOffLoading: Record<number, boolean> = {};
  let dropOffSeq: Record<number, number> = {};

  /** Tope de la ventana de conversión para time-to-convert. Default 30 días. */
  let timingMaxSecs = 30 * 86_400;
  /** Cache timing por paso. */
  let timingByStep: Record<number, TimeToConvertResult> = {};
  let timingErrors: Record<number, string> = {};
  let timingLoading: Record<number, boolean> = {};
  let timingSeq: Record<number, number> = {};

  /** Versión del funnel: cualquier cambio que invalide los caches bumpea esto. */
  let funnelVersion = 0;

  function invalidateInsights(): void {
    funnelVersion++;
    dropOffByStep = {};
    dropOffErrors = {};
    dropOffLoading = {};
    timingByStep = {};
    timingErrors = {};
    timingLoading = {};
    expandedStep = -1;
  }

  // ---------- Drag state ----------
  /** Origen del drag. `null` cuando no hay drag activo. */
  let dragFromCatalog = false;
  let dragName = '';
  let dragFromIndex = -1;

  // ---------- Carga inicial del catálogo ----------
  async function loadCatalog(): Promise<void> {
    catalogLoading = true;
    catalogError = '';
    try {
      catalog = await fetchFunnelEvents({
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
    } catch (e: unknown) {
      catalogError = e instanceof Error ? e.message : String(e);
      catalog = [];
    } finally {
      catalogLoading = false;
    }
  }

  // ---------- Preview live ----------
  function schedulePreview(): void {
    if (debounceTimer) clearTimeout(debounceTimer);
    invalidateInsights();
    if (funnel.length < 2) {
      result = null;
      previewError = '';
      previewLoading = false;
      return;
    }
    debounceTimer = setTimeout(runPreview, 200);
  }

  async function fetchDropOffFor(stepIndex: number): Promise<void> {
    if (funnel.length < 2 || stepIndex >= funnel.length - 1) return;
    const version = funnelVersion;
    const seq = (dropOffSeq[stepIndex] = (dropOffSeq[stepIndex] ?? 0) + 1);
    dropOffLoading = { ...dropOffLoading, [stepIndex]: true };
    dropOffErrors = { ...dropOffErrors, [stepIndex]: '' };
    try {
      const r = await previewDropOff({
        steps: funnel,
        step_index: stepIndex,
        window_seconds: windowSecs,
        lookahead_seconds: lookaheadSecs,
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
      // Si el funnel cambió mientras la query estaba en vuelo, descartar.
      if (version !== funnelVersion) return;
      if (seq !== dropOffSeq[stepIndex]) return;
      dropOffByStep = { ...dropOffByStep, [stepIndex]: r };
    } catch (e: unknown) {
      if (version !== funnelVersion) return;
      if (seq !== dropOffSeq[stepIndex]) return;
      dropOffErrors = {
        ...dropOffErrors,
        [stepIndex]: e instanceof Error ? e.message : String(e)
      };
    } finally {
      if (version === funnelVersion && seq === dropOffSeq[stepIndex]) {
        dropOffLoading = { ...dropOffLoading, [stepIndex]: false };
      }
    }
  }

  async function fetchTimingFor(stepIndex: number): Promise<void> {
    if (funnel.length < 2 || stepIndex >= funnel.length - 1) return;
    const version = funnelVersion;
    const seq = (timingSeq[stepIndex] = (timingSeq[stepIndex] ?? 0) + 1);
    timingLoading = { ...timingLoading, [stepIndex]: true };
    timingErrors = { ...timingErrors, [stepIndex]: '' };
    try {
      const r = await previewTimeToConvert({
        event_from: funnel[stepIndex],
        event_to: funnel[stepIndex + 1],
        max_seconds: timingMaxSecs,
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
      if (version !== funnelVersion) return;
      if (seq !== timingSeq[stepIndex]) return;
      timingByStep = { ...timingByStep, [stepIndex]: r };
    } catch (e: unknown) {
      if (version !== funnelVersion) return;
      if (seq !== timingSeq[stepIndex]) return;
      timingErrors = {
        ...timingErrors,
        [stepIndex]: e instanceof Error ? e.message : String(e)
      };
    } finally {
      if (version === funnelVersion && seq === timingSeq[stepIndex]) {
        timingLoading = { ...timingLoading, [stepIndex]: false };
      }
    }
  }

  function togglePanel(stepIndex: number, view: StepView): void {
    if (expandedStep === stepIndex && expandedView === view) {
      expandedStep = -1;
      return;
    }
    expandedStep = stepIndex;
    expandedView = view;
    if (view === 'dropoff') {
      if (!dropOffByStep[stepIndex] && !dropOffLoading[stepIndex]) {
        void fetchDropOffFor(stepIndex);
      }
    } else {
      if (!timingByStep[stepIndex] && !timingLoading[stepIndex]) {
        void fetchTimingFor(stepIndex);
      }
    }
  }

  function setLookahead(secs: number): void {
    if (secs === lookaheadSecs) return;
    lookaheadSecs = secs;
    dropOffByStep = {};
    dropOffErrors = {};
    if (expandedStep >= 0 && expandedView === 'dropoff') {
      void fetchDropOffFor(expandedStep);
    }
  }

  function setTimingMax(secs: number): void {
    if (secs === timingMaxSecs) return;
    timingMaxSecs = secs;
    timingByStep = {};
    timingErrors = {};
    if (expandedStep >= 0 && expandedView === 'timing') {
      void fetchTimingFor(expandedStep);
    }
  }

  async function runPreview(): Promise<void> {
    const seq = ++reqSeq;
    previewLoading = true;
    previewError = '';
    try {
      const r = await computeFunnel({
        steps: funnel,
        window_seconds: windowSecs,
        last_minutes: rangeMinutes($timeRange),
        project: $selectedProject || undefined
      });
      if (seq !== reqSeq) return; // llegó tarde; ya hay uno más nuevo en vuelo
      result = r;
    } catch (e: unknown) {
      if (seq !== reqSeq) return;
      previewError = e instanceof Error ? e.message : String(e);
      result = null;
    } finally {
      if (seq === reqSeq) previewLoading = false;
    }
  }

  // ---------- Construcción del funnel ----------
  function addStep(name: string): void {
    funnel = [...funnel, name];
    schedulePreview();
  }
  function removeStep(i: number): void {
    funnel = funnel.filter((_, j) => j !== i);
    schedulePreview();
  }
  function moveStep(from: number, to: number): void {
    if (from === to || from < 0 || to < 0 || from >= funnel.length || to > funnel.length) return;
    const next = funnel.slice();
    const [item] = next.splice(from, 1);
    // Si arrastramos hacia abajo y el `to` se calculó pre-splice, compensar.
    const insertAt = to > from ? to - 1 : to;
    next.splice(insertAt, 0, item);
    funnel = next;
    schedulePreview();
  }
  function clearFunnel(): void {
    funnel = [];
    schedulePreview();
  }

  // ---------- Drag & drop handlers ----------
  function onDragStartFromCatalog(e: DragEvent, name: string): void {
    dragFromCatalog = true;
    dragName = name;
    dragFromIndex = -1;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'copy';
      e.dataTransfer.setData('text/plain', name);
    }
  }
  function onDragStartFromFunnel(e: DragEvent, idx: number): void {
    dragFromCatalog = false;
    dragName = funnel[idx];
    dragFromIndex = idx;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', dragName);
    }
  }
  function onDragOver(e: DragEvent): void {
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = dragFromCatalog ? 'copy' : 'move';
    }
  }
  function onDropOnFunnel(e: DragEvent, dropIndex: number): void {
    e.preventDefault();
    if (dragFromCatalog) {
      const next = funnel.slice();
      next.splice(dropIndex, 0, dragName);
      funnel = next;
    } else if (dragFromIndex >= 0) {
      moveStep(dragFromIndex, dropIndex);
    }
    dragFromCatalog = false;
    dragName = '';
    dragFromIndex = -1;
    schedulePreview();
  }
  function onDropOnEnd(e: DragEvent): void {
    onDropOnFunnel(e, funnel.length);
  }
  /** Drop sobre el catálogo: si venía del funnel, lo quita. */
  function onDropOnCatalog(e: DragEvent): void {
    e.preventDefault();
    if (!dragFromCatalog && dragFromIndex >= 0) {
      removeStep(dragFromIndex);
    }
    dragFromCatalog = false;
    dragName = '';
    dragFromIndex = -1;
  }

  // ---------- URL state ----------
  // Persistir el funnel construido + parámetros a la URL para que un F5 no
  // pierda lo que el usuario armó. Mismo patrón que `/events` y `/logs`:
  // applyUrlParams() en onMount, syncToUrl() reactivo en cualquier cambio.
  function syncToUrl(): void {
    if (!browser) return;
    const p = new URLSearchParams();
    if ($selectedProject) p.set('project', $selectedProject);
    if (funnel.length > 0) p.set('steps', funnel.join(','));
    if (windowSecs !== 86_400) p.set('window', String(windowSecs));
    if (lookaheadSecs !== 300) p.set('lookahead', String(lookaheadSecs));
    if (timingMaxSecs !== 30 * 86_400) p.set('timing_max', String(timingMaxSecs));
    if ($timeRange) p.set('range', $timeRange);
    const qs = p.toString();
    const url = `${window.location.origin}${window.location.pathname}${qs ? '?' + qs : ''}`;
    try {
      window.history.replaceState(null, '', url);
    } catch {
      /* no bloqueante */
    }
  }

  function applyUrlParams(): void {
    if (!browser) return;
    const p = new URLSearchParams(window.location.search);
    const proj = p.get('project');
    if (proj && proj !== $selectedProject) selectedProject.set(proj);
    const steps = p.get('steps');
    if (steps) funnel = steps.split(',').map((s) => s.trim()).filter(Boolean);
    const w = Number(p.get('window'));
    if (Number.isFinite(w) && w > 0) windowSecs = w;
    const la = Number(p.get('lookahead'));
    if (Number.isFinite(la) && la > 0) lookaheadSecs = la;
    const tm = Number(p.get('timing_max'));
    if (Number.isFinite(tm) && tm > 0) timingMaxSecs = tm;
    const range = p.get('range');
    if (range) {
      const presets = ['5m', '15m', '1h', '6h', '24h', '7d'] as const;
      if ((presets as readonly string[]).includes(range)) {
        timeRange.set(range as typeof presets[number]);
      }
    }
  }

  // ---------- Reactividad: recargar catálogo cuando cambia rango/proyecto ----------
  // Y re-disparar preview también, porque los parámetros del request cambiaron.
  let inited = false;
  $: if (browser) {
    void $timeRange; void $selectedProject;
    if (inited) {
      void loadCatalog();
      schedulePreview();
      syncToUrl();
    }
  }
  // Sync URL en cualquier cambio del estado persistible.
  $: if (browser && inited) {
    void funnel; void windowSecs; void lookaheadSecs; void timingMaxSecs;
    syncToUrl();
  }

  onMount(async () => {
    applyUrlParams();
    await loadCatalog();
    inited = true;
    // Si el URL trajo un funnel pre-armado, dispará el preview de una.
    if (funnel.length >= 2) schedulePreview();
  });

  onDestroy(() => {
    if (debounceTimer) clearTimeout(debounceTimer);
  });
</script>

<div class="page-header">
  <h1 class="page-title">Funnels</h1>
  <TimeRangePicker />
</div>

<p class="muted hint">
  Arrastrá eventos desde la izquierda al builder para armar un funnel. El resultado se
  actualiza en vivo mientras editás. La ventana de conversión es el tiempo máximo
  entre el primer y el último paso por usuario.
</p>

<div class="layout">
  <CatalogSection
    {catalog}
    loading={catalogLoading}
    error={catalogError}
    bind:filter
    onFilterChange={(v) => (filter = v)}
    onAdd={addStep}
    onDragStart={onDragStartFromCatalog}
    onDragOver={onDragOver}
    onDrop={onDropOnCatalog}
  />

  <BuilderSection
    {funnel}
    {windowSecs}
    onWindowChange={(s) => { windowSecs = s; schedulePreview(); }}
    onClear={clearFunnel}
    onRemoveStep={removeStep}
    onDragStart={onDragStartFromFunnel}
    onDragOver={onDragOver}
    onDropAt={onDropOnFunnel}
    onDropEnd={onDropOnEnd}
  />

  <ResultsSection
    {result}
    loading={previewLoading}
    error={previewError}
    funnelLength={funnel.length}
    {expandedStep}
    {expandedView}
    {lookaheadSecs}
    {timingMaxSecs}
    {dropOffByStep}
    {dropOffErrors}
    {dropOffLoading}
    {timingByStep}
    {timingErrors}
    {timingLoading}
    onTogglePanel={togglePanel}
    onSetLookahead={setLookahead}
    onSetTimingMax={setTimingMax}
  />
</div>

<style>
  /* El layout grid es exclusivo de la página. */
  .layout {
    display: grid;
    grid-template-columns: 240px 1fr 1fr;
    gap: 16px;
    align-items: start;
    min-height: 60vh;
  }
  @media (max-width: 1100px) {
    .layout { grid-template-columns: 1fr; }
  }
  .hint { margin-bottom: 16px; max-width: 720px; }

  /* Clases de utilidad compartidas por los 3 subcomponentes — :global para
   * que apliquen fuera del scope de la página. */
  :global(.pane) {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  :global(.pane-title) {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin: 0;
  }
  :global(.error) {
    color: var(--danger);
    background: var(--badge-error-bg);
    border: 1px solid var(--danger);
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
  }
  :global(.empty) { padding: 24px 8px; text-align: center; font-size: 12px; }
  :global(.empty.small) { padding: 12px 8px; }
  :global(.skel-col) { display: flex; flex-direction: column; gap: 6px; }
</style>
