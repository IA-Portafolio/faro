<script lang="ts">
  /**
   * Página `/funnels` — constructor de embudos de conversión (funnels).
   *
   * El usuario arma una secuencia ordenada de `event_name` (mín. 2 pasos) desde el
   * catálogo y, en vivo y con debounce, calcula el funnel: cuántos usuarios avanzan
   * en cada paso, el drop-off (abandono) y el tiempo de conversión, dentro de una
   * ventana temporal (`windowSecs`). Un contador `reqSeq` descarta respuestas que
   * llegan fuera de orden.
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
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';

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
  type StepView = 'dropoff' | 'timing';
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

  // ---------- Derivados ----------
  $: filteredCatalog = filter.trim()
    ? catalog.filter((e) => e.name.toLowerCase().includes(filter.trim().toLowerCase()))
    : catalog;

  $: maxStepUsers = result ? Math.max(1, ...result.steps.map((s) => s.users)) : 1;

  function fmtPct(p: number): string {
    if (!isFinite(p)) return '–';
    return `${(p * 100).toFixed(1)}%`;
  }
  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return n.toLocaleString();
  }

  const windowPresets: { label: string; seconds: number }[] = [
    { label: '5 min', seconds: 300 },
    { label: '1 hora', seconds: 3600 },
    { label: '1 día', seconds: 86_400 },
    { label: '7 días', seconds: 604_800 },
    { label: '30 días', seconds: 2_592_000 }
  ];

  const lookaheadPresets: { label: string; seconds: number }[] = [
    { label: '1 min', seconds: 60 },
    { label: '5 min', seconds: 300 },
    { label: '15 min', seconds: 900 },
    { label: '1 hora', seconds: 3600 }
  ];

  const timingMaxPresets: { label: string; seconds: number }[] = [
    { label: '1 hora', seconds: 3600 },
    { label: '1 día', seconds: 86_400 },
    { label: '7 días', seconds: 604_800 },
    { label: '30 días', seconds: 2_592_000 },
    { label: '90 días', seconds: 90 * 86_400 }
  ];

  function fmtSecondsRange(lower: number, upper: number | null): string {
    const lo = fmtSeconds(lower);
    if (upper === null) return `> ${lo}`;
    return `${lo} – ${fmtSeconds(upper)}`;
  }
  function fmtSeconds(s: number): string {
    if (s < 60) return `${s}s`;
    if (s < 3600) return `${Math.round(s / 60)}m`;
    if (s < 86_400) return `${Math.round(s / 3600)}h`;
    if (s < 30 * 86_400) return `${Math.round(s / 86_400)}d`;
    return `${Math.round(s / (30 * 86_400))}mo`;
  }
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
  <!-- Catálogo de eventos disponibles -->
  <aside
    class="pane catalog"
    on:dragover={onDragOver}
    on:drop={onDropOnCatalog}
    role="list"
    aria-label="Eventos disponibles"
  >
    <h2 class="pane-title">Eventos</h2>
    <input
      type="search"
      placeholder="Filtrar…"
      bind:value={filter}
      aria-label="Filtrar eventos"
    />
    {#if catalogLoading}
      <div class="skel-col">
        {#each Array(6) as _}
          <Skeleton width="100%" height="28px" radius="6px" />
        {/each}
      </div>
    {:else if catalogError}
      <div class="error">{catalogError}</div>
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
              on:dragstart={(e) => onDragStartFromCatalog(e, ev.name)}
              on:dblclick={() => addStep(ev.name)}
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

  <!-- Builder del funnel -->
  <section class="pane builder">
    <div class="builder-head">
      <h2 class="pane-title">Construcción</h2>
      <div class="builder-controls">
        <label class="window-label">
          <span class="muted">Ventana</span>
          <select bind:value={windowSecs} on:change={schedulePreview}>
            {#each windowPresets as p}
              <option value={p.seconds}>{p.label}</option>
            {/each}
          </select>
        </label>
        {#if funnel.length > 0}
          <button type="button" class="ghost" on:click={clearFunnel}>Limpiar</button>
        {/if}
      </div>
    </div>

    {#if funnel.length === 0}
      <div
        class="dropzone empty"
        role="region"
        aria-label="Zona para soltar eventos"
        on:dragover={onDragOver}
        on:drop={(e) => onDropOnFunnel(e, 0)}
      >
        Arrastrá un evento acá para empezar.<br>
        Mínimo 2 pasos para ver resultados.
      </div>
    {:else}
      <ol class="funnel-steps">
        {#each funnel as ev, i (i + ':' + ev)}
          <!-- Drop-zone ANTES de este paso (insertar en posición i) -->
          <li
            class="drop-gap"
            on:dragover={onDragOver}
            on:drop={(e) => onDropOnFunnel(e, i)}
            aria-hidden="true"
          ></li>
          <li
            class="step"
            draggable="true"
            on:dragstart={(e) => onDragStartFromFunnel(e, i)}
          >
            <span class="step-index mono">{i + 1}</span>
            <span class="step-name">{ev}</span>
            <button
              type="button"
              class="step-remove"
              on:click={() => removeStep(i)}
              aria-label={`Eliminar paso ${i + 1}`}
              title="Eliminar"
            >×</button>
          </li>
        {/each}
        <!-- Drop-zone al final -->
        <li
          class="drop-gap last"
          on:dragover={onDragOver}
          on:drop={onDropOnEnd}
          aria-hidden="true"
        ></li>
      </ol>
    {/if}
  </section>

  <!-- Resultados -->
  <section class="pane results">
    <div class="results-head">
      <h2 class="pane-title">Resultados</h2>
      {#if result}
        <span class="muted timing mono">
          {result.took_ms} ms · {fmtCount(result.total_entered)} usuarios
        </span>
      {/if}
    </div>

    {#if previewError}
      <div class="error">{previewError}</div>
    {:else if funnel.length < 2}
      <div class="muted empty">Agregá al menos 2 pasos para ver la conversión.</div>
    {:else if !result && previewLoading}
      <div class="skel-col">
        {#each Array(funnel.length) as _}
          <Skeleton width="100%" height="36px" radius="6px" />
        {/each}
      </div>
    {:else if result}
      {@const r = result}
      <ul class="bars" class:stale={previewLoading}>
        {#each r.steps as s, i}
          {@const widthPct = (s.users / maxStepUsers) * 100}
          {@const isLastStep = i === r.steps.length - 1}
          {@const droppedAfter = isLastStep ? 0 : s.users - r.steps[i + 1].users}
          {@const droppedPct = s.users > 0 ? droppedAfter / s.users : 0}
          {@const isOpen = expandedStep === i}
          {@const isDropOpen = isOpen && expandedView === 'dropoff'}
          {@const isTimingOpen = isOpen && expandedView === 'timing'}
          {@const dropResult = dropOffByStep[i]}
          {@const dropErr = dropOffErrors[i]}
          {@const dropBusy = dropOffLoading[i]}
          {@const timingResult = timingByStep[i]}
          {@const timingErr = timingErrors[i]}
          {@const timingBusy = timingLoading[i]}
          {@const maxBinUsers = timingResult
            ? Math.max(1, ...timingResult.bins.map((b) => b.users))
            : 1}
          <li class="bar-row">
            <div class="bar-meta">
              <span class="bar-index mono">{i + 1}</span>
              <span class="bar-name">{s.event}</span>
              <span class="bar-users mono">{fmtCount(s.users)}</span>
            </div>
            <div class="bar-track" aria-hidden="true">
              <div class="bar-fill" style="width: {widthPct}%"></div>
            </div>
            <div class="bar-conv mono">
              <span title="Conversión desde el primer paso">{fmtPct(s.conversion_from_start)}</span>
              {#if i > 0}
                <span class="muted" title="Conversión vs paso anterior">
                  ↳ {fmtPct(s.conversion_from_prev)}
                </span>
              {/if}
              {#if !isLastStep}
                <span class="insight-tabs" role="tablist" aria-label={`Insights para ${s.event}`}>
                  <button
                    type="button"
                    role="tab"
                    class="insight-tab"
                    class:active={isDropOpen}
                    on:click={() => togglePanel(i, 'dropoff')}
                    title={`${fmtCount(droppedAfter)} usuarios (${fmtPct(droppedPct)}) no llegaron a ${r.steps[i + 1].event}`}
                    aria-selected={isDropOpen}
                  >
                    <span aria-hidden="true">{isDropOpen ? '▾' : '▸'}</span>
                    <span>Drop-off · {fmtCount(droppedAfter)}</span>
                  </button>
                  <button
                    type="button"
                    role="tab"
                    class="insight-tab"
                    class:active={isTimingOpen}
                    on:click={() => togglePanel(i, 'timing')}
                    title={`Tiempo entre ${s.event} y ${r.steps[i + 1].event}`}
                    aria-selected={isTimingOpen}
                  >
                    <span aria-hidden="true">{isTimingOpen ? '▾' : '▸'}</span>
                    <span>Tiempo</span>
                  </button>
                </span>
              {/if}
            </div>

            {#if !isLastStep && isDropOpen}
              <div class="drop-panel">
                <div class="drop-context muted">
                  {fmtCount(droppedAfter)} usuarios vieron <strong>{s.event}</strong>
                  y NO llegaron a <strong>{r.steps[i + 1].event}</strong>.
                  Eventos en los siguientes
                  <select
                    class="lookahead"
                    value={lookaheadSecs}
                    on:change={(e) => setLookahead(Number((e.currentTarget as HTMLSelectElement).value))}
                  >
                    {#each lookaheadPresets as p}
                      <option value={p.seconds}>{p.label}</option>
                    {/each}
                  </select>:
                </div>

                {#if dropErr}
                  <div class="error">{dropErr}</div>
                {:else if dropBusy && !dropResult}
                  <div class="skel-col">
                    {#each Array(5) as _}
                      <Skeleton width="100%" height="22px" radius="4px" />
                    {/each}
                  </div>
                {:else if dropResult}
                  {#if dropResult.dropped_users === 0}
                    <div class="muted empty small">
                      No hay usuarios en este cohort en el rango actual.
                    </div>
                  {:else if dropResult.top_events.length === 0}
                    <div class="muted empty small">
                      Ningún evento posterior dentro del look-ahead.
                      Probablemente cerraron la pestaña.
                    </div>
                  {:else}
                    <ul class="drop-list">
                      {#each dropResult.top_events as ev}
                        <li class="drop-item">
                          <div class="drop-item-bar" style="width: {ev.share * 100}%"></div>
                          <span class="drop-item-name">{ev.event_name}</span>
                          <span class="drop-item-pct mono">{fmtPct(ev.share)}</span>
                          <span class="drop-item-users mono muted">
                            {fmtCount(ev.users)} u · {fmtCount(ev.occurrences)} ev
                          </span>
                        </li>
                      {/each}
                    </ul>
                    <div class="drop-foot muted mono">
                      {dropResult.took_ms} ms · cohort: {fmtCount(dropResult.dropped_users)} usuarios
                    </div>
                  {/if}
                {/if}
              </div>
            {/if}

            {#if !isLastStep && isTimingOpen}
              <div class="drop-panel">
                <div class="drop-context muted">
                  Tiempo entre <strong>{s.event}</strong> y <strong>{r.steps[i + 1].event}</strong>
                  para usuarios que convirtieron en hasta
                  <select
                    class="lookahead"
                    value={timingMaxSecs}
                    on:change={(e) => setTimingMax(Number((e.currentTarget as HTMLSelectElement).value))}
                  >
                    {#each timingMaxPresets as p}
                      <option value={p.seconds}>{p.label}</option>
                    {/each}
                  </select>:
                </div>

                {#if timingErr}
                  <div class="error">{timingErr}</div>
                {:else if timingBusy && !timingResult}
                  <div class="skel-col">
                    {#each Array(6) as _}
                      <Skeleton width="100%" height="18px" radius="4px" />
                    {/each}
                  </div>
                {:else if timingResult}
                  {@const tr = timingResult}
                  {@const convRate = tr.total_with_from > 0
                    ? tr.total_converted / tr.total_with_from
                    : 0}
                  {#if tr.total_with_from === 0}
                    <div class="muted empty small">
                      Nadie disparó <strong>{s.event}</strong> en este rango.
                    </div>
                  {:else if tr.total_converted === 0}
                    <div class="muted empty small">
                      {fmtCount(tr.total_with_from)} usuarios vieron {s.event}
                      pero ninguno llegó a {r.steps[i + 1].event} dentro del tope.
                    </div>
                  {:else}
                    <div class="timing-stats">
                      <div class="stat">
                        <span class="stat-label muted">Conversión</span>
                        <span class="stat-value mono">{fmtPct(convRate)}</span>
                        <span class="stat-sub muted mono">
                          {fmtCount(tr.total_converted)}/{fmtCount(tr.total_with_from)}
                        </span>
                      </div>
                      <div class="stat">
                        <span class="stat-label muted">p50</span>
                        <span class="stat-value mono">{fmtSeconds(tr.p50_seconds)}</span>
                      </div>
                      <div class="stat">
                        <span class="stat-label muted">p90</span>
                        <span class="stat-value mono">{fmtSeconds(tr.p90_seconds)}</span>
                      </div>
                      <div class="stat">
                        <span class="stat-label muted">p99</span>
                        <span class="stat-value mono">{fmtSeconds(tr.p99_seconds)}</span>
                      </div>
                    </div>
                    <ul class="hist">
                      {#each tr.bins as b}
                        {@const heightPct = (b.users / maxBinUsers) * 100}
                        <li class="hist-col" title={`${fmtSecondsRange(b.lower_seconds, b.upper_seconds)}: ${fmtCount(b.users)} usuarios`}>
                          <span class="hist-count mono">{b.users > 0 ? fmtCount(b.users) : ''}</span>
                          <span class="hist-bar" style="height: {heightPct}%" aria-hidden="true"></span>
                          <span class="hist-label mono">{fmtSecondsRange(b.lower_seconds, b.upper_seconds)}</span>
                        </li>
                      {/each}
                    </ul>
                    <div class="drop-foot muted mono">
                      {tr.took_ms} ms · max delta observado: {fmtSeconds(tr.max_seconds_observed)}
                    </div>
                  {/if}
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      <div class="muted window-info">
        Ventana de conversión: {windowPresets.find((p) => p.seconds === r.window_seconds)?.label ?? `${r.window_seconds}s`}
      </div>
    {/if}
  </section>
</div>

<style>
  .hint { margin-bottom: 16px; max-width: 720px; }

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

  .pane {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .pane-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin: 0;
  }

  /* ----- Catálogo ----- */
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

  /* ----- Builder ----- */
  .builder-head, .results-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .builder-controls { display: flex; gap: 8px; align-items: center; }
  .window-label { display: flex; gap: 6px; align-items: center; font-size: 12px; }
  .ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
    padding: 4px 10px;
  }
  .ghost:hover { color: var(--text); }

  .dropzone {
    border: 2px dashed var(--border);
    border-radius: 8px;
    padding: 40px 16px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.6;
  }
  .dropzone.empty { background: transparent; }

  .funnel-steps {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
  }
  .step {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    cursor: grab;
  }
  .step:active { cursor: grabbing; }
  .step-index {
    width: 22px; height: 22px;
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: 11px;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .step-name { flex: 1; font-size: 13px; overflow: hidden; text-overflow: ellipsis; }
  .step-remove {
    background: transparent;
    border: 0;
    color: var(--text-muted);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
    padding: 0 6px;
  }
  .step-remove:hover { color: var(--danger); }
  .drop-gap {
    height: 8px;
    border-radius: 4px;
    transition: background 80ms;
  }
  .drop-gap.last { height: 16px; }
  .drop-gap:hover { background: var(--bg-hover); }

  /* ----- Resultados ----- */
  .timing { font-size: 11px; }
  .bars {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .bars.stale { opacity: 0.6; }
  .bar-row { display: flex; flex-direction: column; gap: 4px; }
  .bar-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .bar-index {
    width: 18px; height: 18px;
    border-radius: 50%;
    background: var(--bg-hover);
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .bar-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar-users { color: var(--text); font-size: 12px; }
  .bar-track {
    height: 10px;
    background: var(--bg-hover);
    border-radius: 5px;
    overflow: hidden;
  }
  .bar-fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-dim));
    transition: width 150ms ease-out;
  }
  .bar-conv {
    display: flex;
    gap: 10px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .window-info { font-size: 11px; margin-top: 8px; }

  .error {
    color: var(--danger);
    background: var(--badge-error-bg);
    border: 1px solid var(--danger);
    padding: 8px 10px;
    border-radius: 6px;
    font-size: 12px;
  }
  .empty { padding: 24px 8px; text-align: center; font-size: 12px; }
  .empty.small { padding: 12px 8px; }
  .skel-col { display: flex; flex-direction: column; gap: 6px; }

  /* ----- Insight tabs (Drop-off / Tiempo) ----- */
  .insight-tabs {
    display: inline-flex;
    gap: 4px;
    margin-left: auto;
  }
  .insight-tab {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-muted);
    font-size: 11px;
    padding: 2px 8px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .insight-tab:hover { color: var(--text); border-color: var(--accent-dim); }
  .insight-tab.active { color: var(--text); background: var(--bg-hover); border-color: var(--accent-dim); }
  .bar-conv { align-items: center; flex-wrap: wrap; }

  .drop-panel {
    margin-top: 8px;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .drop-context { font-size: 11.5px; line-height: 1.5; }
  .drop-context strong { color: var(--text); font-weight: 600; }
  .lookahead {
    margin: 0 4px;
    padding: 1px 4px;
    font-size: 11px;
  }
  .drop-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .drop-item {
    position: relative;
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 12px;
    isolation: isolate;
  }
  .drop-item-bar {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--badge-info-bg);
    border-radius: 4px;
    z-index: -1;
    transition: width 150ms ease-out;
  }
  .drop-item-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drop-item-pct { font-size: 12px; }
  .drop-item-users { font-size: 10.5px; }
  .drop-foot { font-size: 10.5px; text-align: right; }

  /* ----- Time-to-convert ----- */
  .timing-stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
    padding: 4px 0;
  }
  .stat {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 6px 8px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 4px;
  }
  .stat-label { font-size: 10px; text-transform: uppercase; letter-spacing: 0.5px; }
  .stat-value { font-size: 13px; color: var(--text); }
  .stat-sub { font-size: 10px; }

  .hist {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
    display: grid;
    /* Una columna por bin; se reparten el ancho disponible del panel. */
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 4px;
    align-items: end;
    /* Altura suficiente para que las barras se lean; las labels se acomodan abajo. */
    min-height: 130px;
  }
  .hist-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    height: 100%;
    justify-content: flex-end;
  }
  .hist-count {
    font-size: 9.5px;
    color: var(--text-muted);
    min-height: 12px;
    line-height: 1;
  }
  .hist-bar {
    width: 100%;
    background: linear-gradient(180deg, var(--accent), var(--accent-dim));
    border-radius: 2px 2px 0 0;
    min-height: 1px;
    transition: height 200ms ease-out;
  }
  .hist-label {
    font-size: 9px;
    color: var(--text-muted);
    text-align: center;
    line-height: 1.1;
    /* Las labels son cortas ("1d-7d"), pero permitir wrap evita corte feo. */
    word-break: break-all;
    margin-top: 2px;
  }
</style>
