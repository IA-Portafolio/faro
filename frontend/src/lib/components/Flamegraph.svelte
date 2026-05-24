<script lang="ts">
  /**
   * Flamegraph jerárquico para una traza.
   *
   * Construye el árbol con `parent_span_id`, pinta las barras proporcionales
   * a la duración relativa al rango total de la traza, colorea por servicio
   * y permite zoom horizontal con la rueda del ratón / botones, panning con
   * arrastre, y muestra un tooltip al pasar por encima de un span.
   *
   * Emite `select` con el span elegido al hacer click.
   */
  import { createEventDispatcher, onMount } from 'svelte';
  import type { SpanRow } from '$lib/api';
  import { formatDuration } from '$lib/stores';

  export let spans: SpanRow[] = [];

  const dispatch = createEventDispatcher<{ select: SpanRow }>();

  // Convierte el timestamp del span (ISO o "yyyy-mm-dd HH:MM:SS.nnn") a ns desde epoch.
  function tsToNs(ts: string): number {
    if (!ts) return 0;
    const iso = ts.includes('T') ? ts : ts.replace(' ', 'T') + 'Z';
    // Date.parse devuelve ms; el resto del nanosegundo se pierde, pero es
    // suficiente para visualizar (la duración viene como ns absolutos).
    return Date.parse(iso) * 1_000_000;
  }

  type Node = {
    span: SpanRow;
    startNs: number;
    endNs: number;
    depth: number;
    children: Node[];
  };

  // ---------- Construcción del árbol ----------
  // Reactivo: cuando cambian los spans, todo lo derivado se re-deriva.
  $: byId = (() => {
    const m = new Map<string, SpanRow>();
    for (const s of spans) m.set(s.span_id, s);
    return m;
  })();

  $: tree = (() => {
    const nodes = new Map<string, Node>();
    for (const s of spans) {
      const startNs = tsToNs(s.timestamp);
      nodes.set(s.span_id, {
        span: s,
        startNs,
        endNs: startNs + s.duration_ns,
        depth: 0,
        children: []
      });
    }
    const roots: Node[] = [];
    for (const n of nodes.values()) {
      const pid = n.span.parent_span_id;
      // Se considera root si no tiene padre o si el padre no está en este lote.
      const parent = pid && byId.has(pid) ? nodes.get(pid) : null;
      if (parent) parent.children.push(n);
      else roots.push(n);
    }
    // Ordena hijos por start time para que el dibujo sea predecible.
    function sortRec(n: Node, depth: number): void {
      n.depth = depth;
      n.children.sort((a, b) => a.startNs - b.startNs);
      for (const c of n.children) sortRec(c, depth + 1);
    }
    roots.sort((a, b) => a.startNs - b.startNs);
    for (const r of roots) sortRec(r, 0);
    return roots;
  })();

  // Aplana el árbol en pre-order para asignar índices de fila estables.
  $: flat = (() => {
    const out: Node[] = [];
    function walk(n: Node): void {
      out.push(n);
      for (const c of n.children) walk(c);
    }
    for (const r of tree) walk(r);
    return out;
  })();

  $: traceStartNs = flat.length > 0 ? Math.min(...flat.map((n) => n.startNs)) : 0;
  $: traceEndNs = flat.length > 0 ? Math.max(...flat.map((n) => n.endNs)) : 0;
  $: totalDur = Math.max(1, traceEndNs - traceStartNs);

  // ---------- Servicios (color) ----------
  $: services = (() => {
    const set = new Set<string>();
    for (const s of spans) set.add(s.service_name || 'unknown');
    return Array.from(set).sort();
  })();

  /** Hash determinístico string→entero (DJB2). */
  function hashStr(s: string): number {
    let h = 5381;
    for (let i = 0; i < s.length; i++) h = ((h << 5) + h + s.charCodeAt(i)) | 0;
    return Math.abs(h);
  }
  function serviceColor(name: string): string {
    const h = hashStr(name) % 360;
    // Saturación/luminosidad pensadas para legibilidad sobre ambos temas.
    return `hsl(${h} 70% 50%)`;
  }

  // ---------- Zoom / pan ----------
  /** Fracción [0..1] visible. 1 = todo, 0.1 = 10× zoom. */
  let viewSpan = 1;
  /** Offset [0..1-viewSpan] del lado izquierdo del viewport. */
  let viewOffset = 0;

  $: viewSpan = Math.min(1, Math.max(0.001, viewSpan));
  $: viewOffset = Math.max(0, Math.min(1 - viewSpan, viewOffset));

  function resetView(): void {
    viewSpan = 1;
    viewOffset = 0;
  }
  function zoomIn(): void {
    const center = viewOffset + viewSpan / 2;
    viewSpan = Math.max(0.001, viewSpan * 0.5);
    viewOffset = center - viewSpan / 2;
  }
  function zoomOut(): void {
    const center = viewOffset + viewSpan / 2;
    viewSpan = Math.min(1, viewSpan * 2);
    viewOffset = center - viewSpan / 2;
  }

  let trackWrapEl: HTMLDivElement | null = null;

  function onWheel(e: WheelEvent): void {
    // Zoom centrado en la posición del cursor.
    if (!trackWrapEl) return;
    e.preventDefault();
    const rect = trackWrapEl.getBoundingClientRect();
    const fx = (e.clientX - rect.left) / Math.max(1, rect.width); // 0..1 dentro del viewport
    const focusFraction = viewOffset + fx * viewSpan;
    const factor = e.deltaY > 0 ? 1.2 : 1 / 1.2;
    const newSpan = Math.min(1, Math.max(0.001, viewSpan * factor));
    viewOffset = focusFraction - fx * newSpan;
    viewSpan = newSpan;
  }

  let dragging = false;
  let dragStartX = 0;
  let dragStartOffset = 0;

  function onMouseDown(e: MouseEvent): void {
    // Solo botón principal y solo si no se está clickeando una barra (esas
    // capturan el click para selección).
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    if (target.closest('.fg-bar')) return;
    dragging = true;
    dragStartX = e.clientX;
    dragStartOffset = viewOffset;
  }

  function onMouseMove(e: MouseEvent): void {
    if (!dragging || !trackWrapEl) return;
    const dx = e.clientX - dragStartX;
    const rect = trackWrapEl.getBoundingClientRect();
    const deltaFraction = -dx / Math.max(1, rect.width) * viewSpan;
    viewOffset = Math.max(0, Math.min(1 - viewSpan, dragStartOffset + deltaFraction));
  }
  function endDrag(): void { dragging = false; }

  // ---------- Helpers de proyección ----------
  function leftPct(n: Node): number {
    const startFraction = (n.startNs - traceStartNs) / totalDur;
    return ((startFraction - viewOffset) / viewSpan) * 100;
  }
  function widthPct(n: Node): number {
    const w = (n.span.duration_ns / totalDur) / viewSpan * 100;
    return Math.max(0.15, w);
  }

  // ---------- Tooltip ----------
  let hovered: Node | null = null;
  let tooltipX = 0;
  let tooltipY = 0;

  function onBarMouseEnter(e: MouseEvent, n: Node): void {
    hovered = n;
    moveTooltip(e);
  }
  function moveTooltip(e: MouseEvent): void {
    tooltipX = e.clientX + 14;
    tooltipY = e.clientY + 14;
  }
  function onBarMouseLeave(): void { hovered = null; }

  // ---------- Listeners globales para arrastre ----------
  onMount(() => {
    const mm = (e: MouseEvent) => onMouseMove(e);
    const mu = () => endDrag();
    window.addEventListener('mousemove', mm);
    window.addEventListener('mouseup', mu);
    return () => {
      window.removeEventListener('mousemove', mm);
      window.removeEventListener('mouseup', mu);
    };
  });

  // ---------- Layout constantes ----------
  const ROW_H = 22;            // px por fila del flamegraph
  const SIDE_W = 280;          // px de la columna de etiquetas
  $: bodyHeight = flat.length * ROW_H;
  $: visibleStartNs = traceStartNs + viewOffset * totalDur;
  $: visibleEndNs = traceStartNs + (viewOffset + viewSpan) * totalDur;
  $: visibleDur = visibleEndNs - visibleStartNs;

  // Ticks de la regla superior — 5 marcas equiespaciadas dentro del viewport.
  $: ticks = (() => {
    const N = 6;
    const out: { pct: number; label: string }[] = [];
    for (let i = 0; i < N; i++) {
      const pct = (i / (N - 1)) * 100;
      const ns = visibleDur * (i / (N - 1));
      out.push({ pct, label: formatDuration(ns) });
    }
    return out;
  })();
</script>

<div class="fg">
  <!-- Controles + leyenda -->
  <div class="fg-controls">
    <div class="fg-zoom">
      <button type="button" on:click={zoomOut} title="Zoom out" aria-label="Zoom out">−</button>
      <button type="button" on:click={resetView} title="Restablecer zoom">100%</button>
      <button type="button" on:click={zoomIn} title="Zoom in" aria-label="Zoom in">+</button>
      <span class="muted" style="font-size: 11.5px; margin-left: 8px;">
        Mostrando {formatDuration(visibleDur)} de {formatDuration(totalDur)}
      </span>
    </div>
    <div class="fg-legend" aria-label="Servicios">
      {#each services as svc}
        <span class="fg-legend-item">
          <span class="fg-legend-swatch" style="background: {serviceColor(svc)};"></span>
          <span>{svc}</span>
        </span>
      {/each}
    </div>
  </div>

  <!-- Cabecera con regla de tiempos -->
  <div class="fg-head" style="grid-template-columns: {SIDE_W}px 1fr;">
    <div class="fg-head-label muted">Span</div>
    <div class="fg-ticks" aria-hidden="true">
      {#each ticks as t}
        <div class="fg-tick" style="left: {t.pct}%;">
          <span class="fg-tick-label mono">{t.label}</span>
        </div>
      {/each}
    </div>
  </div>

  <!-- Cuerpo -->
  <!-- svelte-ignore a11y-no-static-element-interactions -->
  <div
    class="fg-body"
    style="grid-template-columns: {SIDE_W}px 1fr; height: {bodyHeight}px;"
    on:mousedown={onMouseDown}
  >
    <!-- Columna de etiquetas -->
    <div class="fg-side">
      {#each flat as n (n.span.span_id)}
        {@const sliceLen = Math.max(0, n.depth)}
        <div class="fg-side-row" style="height: {ROW_H}px;">
          <span class="fg-indent" style="width: {sliceLen * 10}px;"></span>
          <span class="fg-side-name mono" title={`${n.span.service_name}  ${n.span.name}`}>
            {n.span.name}
          </span>
          <span class="fg-side-svc muted">{n.span.service_name}</span>
        </div>
      {/each}
    </div>

    <!-- Track con las barras -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="fg-track"
      bind:this={trackWrapEl}
      on:wheel={onWheel}
      class:dragging
    >
      <!-- Guías verticales en cada tick. -->
      {#each ticks as t}
        <div class="fg-guide" style="left: {t.pct}%;"></div>
      {/each}

      {#each flat as n, i (n.span.span_id)}
        {@const lp = leftPct(n)}
        {@const wp = widthPct(n)}
        {@const visible = lp + wp > 0 && lp < 100}
        {#if visible}
          {@const clippedLeft = Math.max(0, lp)}
          {@const clippedRight = Math.min(100, lp + wp)}
          {@const clippedWidth = clippedRight - clippedLeft}
          <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
          <div
            class="fg-bar"
            class:err={n.span.status_code === 'ERROR'}
            style="
              top: {i * ROW_H}px;
              left: {clippedLeft}%;
              width: {clippedWidth}%;
              height: {ROW_H - 2}px;
              background: {serviceColor(n.span.service_name)};"
            on:click|stopPropagation={() => dispatch('select', n.span)}
            on:mouseenter={(e) => onBarMouseEnter(e, n)}
            on:mousemove={moveTooltip}
            on:mouseleave={onBarMouseLeave}
            title={`${n.span.service_name}  ${n.span.name}  ·  ${formatDuration(n.span.duration_ns)}`}
          >
            <span class="fg-bar-label">{n.span.name}</span>
            {#if (n.span.events_names?.length ?? 0) > 0}
              <span class="fg-bar-dot" title={`${n.span.events_names.length} eventos`}></span>
            {/if}
          </div>
        {/if}
      {/each}
    </div>
  </div>

  {#if hovered}
    <div
      class="fg-tooltip"
      role="status"
      style="top: {tooltipY}px; left: {tooltipX}px;"
    >
      <div class="fg-tooltip-head">
        <span class="fg-legend-swatch" style="background: {serviceColor(hovered.span.service_name)};"></span>
        <strong>{hovered.span.name}</strong>
      </div>
      <div class="muted" style="font-size: 11.5px;">{hovered.span.service_name} · {hovered.span.kind || 'INTERNAL'}</div>
      <div class="fg-tooltip-row">
        <span class="muted">Duración</span>
        <span class="mono">{formatDuration(hovered.span.duration_ns)}</span>
      </div>
      <div class="fg-tooltip-row">
        <span class="muted">Estado</span>
        <span class="badge {hovered.span.status_code === 'ERROR' ? 'error' : hovered.span.status_code === 'OK' ? 'ok' : 'debug'}">
          {hovered.span.status_code || 'UNSET'}
        </span>
      </div>
      {#if hovered.span.status_message}
        <div class="muted" style="font-size: 11.5px; max-width: 320px;">{hovered.span.status_message}</div>
      {/if}
      {#if (hovered.span.events_names?.length ?? 0) > 0}
        <div class="fg-tooltip-row">
          <span class="muted">Eventos</span>
          <span>{hovered.span.events_names.length}</span>
        </div>
      {/if}
      {#if (hovered.span.links_trace_ids?.length ?? 0) > 0}
        <div class="fg-tooltip-row">
          <span class="muted">Links salientes</span>
          <span>{hovered.span.links_trace_ids?.length}</span>
        </div>
      {/if}
      <div class="muted" style="font-size: 11px; margin-top: 4px;">Click para abrir el detalle</div>
    </div>
  {/if}
</div>

<style>
  .fg {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    user-select: none;
  }
  .fg-controls {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
    flex-wrap: wrap;
  }
  .fg-zoom { display: inline-flex; gap: 4px; align-items: center; }
  .fg-zoom button {
    padding: 2px 10px;
    font-size: 13px;
    line-height: 1.2;
  }
  .fg-legend {
    display: flex;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 11.5px;
  }
  .fg-legend-item { display: inline-flex; align-items: center; gap: 4px; }
  .fg-legend-swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    display: inline-block;
  }
  .fg-head {
    display: grid;
    border-bottom: 1px solid var(--border);
    background: var(--bg);
  }
  .fg-head-label {
    padding: 6px 12px;
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    border-right: 1px solid var(--border);
  }
  .fg-ticks {
    position: relative;
    height: 22px;
  }
  .fg-tick {
    position: absolute;
    top: 0; bottom: 0;
    width: 1px;
    background: var(--border);
  }
  .fg-tick-label {
    position: absolute;
    bottom: 2px;
    transform: translateX(4px);
    font-size: 10.5px;
    color: var(--text-muted);
    white-space: nowrap;
  }
  .fg-body {
    display: grid;
    position: relative;
  }
  .fg-side {
    border-right: 1px solid var(--border);
    overflow: hidden;
  }
  .fg-side-row {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 8px 0 8px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    overflow: hidden;
  }
  .fg-indent {
    flex-shrink: 0;
    border-left: 1px solid var(--border);
    height: 100%;
  }
  .fg-side-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }
  .fg-side-svc {
    font-size: 10.5px;
    text-transform: lowercase;
    flex-shrink: 0;
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fg-track {
    position: relative;
    cursor: grab;
    overflow: hidden;
  }
  .fg-track.dragging { cursor: grabbing; }
  .fg-guide {
    position: absolute;
    top: 0; bottom: 0;
    width: 1px;
    background: var(--border);
    opacity: 0.6;
    pointer-events: none;
  }
  .fg-bar {
    position: absolute;
    border-radius: 3px;
    overflow: hidden;
    cursor: pointer;
    color: rgba(0, 0, 0, 0.75);
    font-size: 11px;
    padding: 0 4px;
    display: flex;
    align-items: center;
    transition: filter 0.08s, box-shadow 0.08s;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.18);
  }
  .fg-bar:hover {
    filter: brightness(1.12);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.7);
    z-index: 2;
  }
  .fg-bar.err {
    box-shadow: inset 0 0 0 1.5px var(--danger);
  }
  .fg-bar-label {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: clip;
    pointer-events: none;
  }
  .fg-bar-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.6);
    margin-left: 4px;
    flex-shrink: 0;
  }
  .fg-tooltip {
    position: fixed;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 10px 12px;
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.35);
    z-index: 200;
    min-width: 220px;
    pointer-events: none;
  }
  .fg-tooltip-head {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-bottom: 4px;
  }
  .fg-tooltip-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    margin-top: 4px;
    font-size: 12px;
  }
</style>
