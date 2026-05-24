<script lang="ts">
  /**
   * Histograma de volumen de product events. Análogo a `LogVolumeHistogram`
   * pero apilando por `event_name` (top-K + un slice "otros") en vez de por
   * severidad. Mantiene la misma UX de drag-to-select sub-rango para que el
   * usuario pueda hacer "zoom" temporal sin tocar el TimeRangePicker.
   */
  import { createEventDispatcher, onMount } from 'svelte';
  import { fetchEventStats } from '$lib/api';

  export let lastMinutes: number;
  export let eventName: string | undefined = undefined;
  export let project: string | undefined = undefined;
  export let selection: { from: string; to: string } | null = null;
  export let height = 84;
  /** Tope de event_names distintos coloreados; el resto se acumula en "otros". */
  export let topK = 6;

  const dispatch = createEventDispatcher<{
    selectionchange: { from: string; to: string } | null;
  }>();

  type Bucket = {
    ts: number;
    counts: Record<string, number>;
    total: number;
  };

  // Paleta categórica estable: el mismo nombre siempre cae en el mismo color
  // dentro de una sesión. Se hashea el nombre a un índice de la paleta.
  const PALETTE = [
    '#60a5fa', // blue
    '#34d399', // emerald
    '#fbbf24', // amber
    '#f472b6', // pink
    '#a78bfa', // violet
    '#22d3ee', // cyan
    '#f87171', // red
    '#fb923c'  // orange
  ];
  const OTHERS_COLOR = 'var(--text-muted)';
  const OTHERS_KEY = '__others__';

  function colorFor(name: string): string {
    if (name === OTHERS_KEY) return OTHERS_COLOR;
    let h = 0;
    for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) | 0;
    return PALETTE[Math.abs(h) % PALETTE.length];
  }

  let buckets: Bucket[] = [];
  let loading = false;
  let error = '';
  let maxTotal = 1;
  let bucketSec = 60;
  /** Nombres visibles en la leyenda (los topK + opcionalmente "otros"), en orden
   *  de mayor a menor frecuencia agregada en el rango. */
  let visibleNames: string[] = [];
  let containerEl: HTMLDivElement | null = null;
  let svgEl: SVGSVGElement | null = null;
  let widthPx = 800;
  let hoverIdx: number | null = null;
  let dragStartIdx: number | null = null;
  let dragEndIdx: number | null = null;

  function pickBucketSeconds(minutes: number): number {
    if (minutes <= 60) return 60;
    if (minutes <= 360) return 300;
    if (minutes <= 1440) return 1800;
    return 3600;
  }

  function parseCh(ts: string): number {
    if (!ts) return NaN;
    const iso = ts.includes('T') ? ts : ts.replace(' ', 'T') + 'Z';
    return new Date(iso).getTime();
  }

  function fmtHHMM(ms: number): string {
    return new Date(ms).toLocaleTimeString(undefined, {
      hour: '2-digit',
      minute: '2-digit'
    });
  }

  function isoZ(ms: number): string {
    return new Date(ms).toISOString();
  }

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const m = Math.max(1, Math.round(lastMinutes));
      bucketSec = pickBucketSeconds(m);
      const stats = await fetchEventStats({
        last_minutes: m,
        bucket_seconds: bucketSec,
        project: project || undefined,
        event_name: eventName || undefined
      });

      const now = Date.now();
      const fromMs = now - m * 60 * 1000;
      const bucketMs = bucketSec * 1000;
      const startBucket = Math.floor(fromMs / bucketMs) * bucketMs;
      const endBucket = Math.floor(now / bucketMs) * bucketMs;
      const grid: Bucket[] = [];
      for (let t = startBucket; t <= endBucket; t += bucketMs) {
        grid.push({ ts: t, counts: {}, total: 0 });
      }
      const byTs = new Map<number, Bucket>();
      for (const b of grid) byTs.set(b.ts, b);

      // Primero acumular totales por event_name en todo el rango para decidir
      // qué entra en topK y qué cae a "otros".
      const totalByName = new Map<string, number>();
      for (const row of stats) {
        const c = Number(row.count) || 0;
        if (c <= 0) continue;
        totalByName.set(row.event_name, (totalByName.get(row.event_name) ?? 0) + c);
      }
      const ranked = Array.from(totalByName.entries())
        .sort((a, b) => b[1] - a[1])
        .map(([name]) => name);
      const top = new Set(ranked.slice(0, Math.max(1, topK)));
      const hasOthers = ranked.length > top.size;

      for (const row of stats) {
        const ms = parseCh(row.ts);
        if (!isFinite(ms)) continue;
        const key = Math.floor(ms / bucketMs) * bucketMs;
        const bucket = byTs.get(key);
        if (!bucket) continue;
        const count = Number(row.count) || 0;
        const slot = top.has(row.event_name) ? row.event_name : OTHERS_KEY;
        bucket.counts[slot] = (bucket.counts[slot] ?? 0) + count;
        bucket.total += count;
      }

      maxTotal = Math.max(1, ...grid.map((b) => b.total));
      buckets = grid;
      visibleNames = ranked.slice(0, top.size);
      if (hasOthers) visibleNames.push(OTHERS_KEY);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      buckets = [];
      maxTotal = 1;
      visibleNames = [];
    } finally {
      loading = false;
    }
  }

  function bucketDuration(): number {
    return bucketSec * 1000;
  }

  function cumulativeBelow(b: Bucket, name: string): number {
    let sum = 0;
    for (const n of visibleNames) {
      if (n === name) break;
      sum += b.counts[n] ?? 0;
    }
    return sum;
  }

  $: padding = { left: 8, right: 8, top: 6, bottom: 18 };
  $: plotW = Math.max(0, widthPx - padding.left - padding.right);
  $: plotH = Math.max(0, height - padding.top - padding.bottom);
  $: barW = buckets.length > 0 ? plotW / buckets.length : 0;

  function xForIdx(i: number): number {
    return padding.left + i * barW;
  }
  function idxForX(x: number): number {
    if (barW <= 0) return 0;
    const raw = (x - padding.left) / barW;
    return Math.max(0, Math.min(buckets.length - 1, Math.floor(raw)));
  }
  function clientToSvgX(clientX: number): number {
    if (!svgEl) return 0;
    const rect = svgEl.getBoundingClientRect();
    if (rect.width <= 0) return 0;
    const ratio = widthPx / rect.width;
    return (clientX - rect.left) * ratio;
  }

  function onPointerDown(e: PointerEvent): void {
    if (buckets.length === 0) return;
    const idx = idxForX(clientToSvgX(e.clientX));
    dragStartIdx = idx;
    dragEndIdx = idx;
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
  }
  function onPointerMove(e: PointerEvent): void {
    if (buckets.length === 0) return;
    const idx = idxForX(clientToSvgX(e.clientX));
    hoverIdx = idx;
    if (dragStartIdx !== null) dragEndIdx = idx;
  }
  function onPointerUp(e: PointerEvent): void {
    if (dragStartIdx === null) return;
    const a = Math.min(dragStartIdx, dragEndIdx ?? dragStartIdx);
    const b = Math.max(dragStartIdx, dragEndIdx ?? dragStartIdx);
    dragStartIdx = null;
    dragEndIdx = null;
    try {
      (e.currentTarget as Element).releasePointerCapture(e.pointerId);
    } catch {
      /* ignora */
    }
    if (buckets.length === 0) return;
    const start = buckets[a].ts;
    const end = buckets[b].ts + bucketDuration();
    const next = { from: isoZ(start), to: isoZ(end) };
    if (selection && selection.from === next.from && selection.to === next.to) {
      selection = null;
      dispatch('selectionchange', null);
    } else {
      selection = next;
      dispatch('selectionchange', next);
    }
  }
  function onPointerLeave(): void {
    hoverIdx = null;
  }

  function clearSelection(): void {
    if (!selection) return;
    selection = null;
    dispatch('selectionchange', null);
  }

  function resize(): void {
    if (containerEl) widthPx = Math.max(120, containerEl.clientWidth);
  }

  onMount(() => {
    resize();
    const ro = new ResizeObserver(resize);
    if (containerEl) ro.observe(containerEl);
    return () => ro.disconnect();
  });

  let loadTimer: ReturnType<typeof setTimeout> | null = null;
  function scheduleLoad(): void {
    if (loadTimer) clearTimeout(loadTimer);
    loadTimer = setTimeout(load, 80);
  }
  $: lastMinutes, eventName, project, scheduleLoad();

  $: xTicks = buckets.length > 0
    ? [
        { idx: 0, label: fmtHHMM(buckets[0].ts) },
        { idx: Math.floor(buckets.length / 2), label: fmtHHMM(buckets[Math.floor(buckets.length / 2)].ts) },
        { idx: buckets.length - 1, label: fmtHHMM(buckets[buckets.length - 1].ts + bucketDuration()) }
      ]
    : [];

  $: dragRect = (dragStartIdx !== null && dragEndIdx !== null && buckets.length > 0)
    ? (() => {
        const a = Math.min(dragStartIdx, dragEndIdx);
        const b = Math.max(dragStartIdx, dragEndIdx);
        return { x: xForIdx(a), w: Math.max(barW, (b - a + 1) * barW) };
      })()
    : null;

  $: selectionRect = (() => {
    if (!selection || buckets.length === 0) return null;
    const selStart = new Date(selection.from).getTime();
    const selEnd = new Date(selection.to).getTime();
    if (!isFinite(selStart) || !isFinite(selEnd)) return null;
    const dur = bucketDuration();
    let aIdx = -1;
    let bIdx = -1;
    for (let i = 0; i < buckets.length; i++) {
      const t = buckets[i].ts;
      if (aIdx < 0 && t + dur > selStart) aIdx = i;
      if (t < selEnd) bIdx = i;
    }
    if (aIdx < 0 || bIdx < 0 || bIdx < aIdx) return null;
    return { x: xForIdx(aIdx), w: Math.max(barW, (bIdx - aIdx + 1) * barW) };
  })();

  function bucketSummary(b: Bucket): string {
    const parts = visibleNames
      .filter((n) => (b.counts[n] ?? 0) > 0)
      .map((n) => `${displayName(n)}=${b.counts[n]}`);
    return parts.length > 0 ? parts.join(' · ') : '0';
  }

  function displayName(n: string): string {
    return n === OTHERS_KEY ? 'otros' : n;
  }

  $: rangeLabel = bucketSec < 60
    ? `${bucketSec}s`
    : bucketSec < 3600
      ? `${Math.round(bucketSec / 60)} min`
      : `${Math.round(bucketSec / 3600)} h`;

  $: totalCount = buckets.reduce((s, b) => s + b.total, 0);
</script>

<div class="hist" bind:this={containerEl}>
  <div class="hist-header">
    <span class="muted hist-label">
      Volumen ({rangeLabel}/bar) · total {totalCount.toLocaleString()} eventos
    </span>
    {#if loading}<span class="spinner" aria-label="cargando"></span>{/if}
    {#if selection}
      <span class="hist-sel-chip">
        Sub-rango: {new Date(selection.from).toLocaleTimeString()} → {new Date(selection.to).toLocaleTimeString()}
        <button class="hist-clear" on:click={clearSelection} title="Limpiar selección" aria-label="Limpiar selección">×</button>
      </span>
    {/if}
    {#if hoverIdx !== null && buckets[hoverIdx]}
      <span class="muted hist-tooltip mono">
        {fmtHHMM(buckets[hoverIdx].ts)}–{fmtHHMM(buckets[hoverIdx].ts + bucketDuration())} · {buckets[hoverIdx].total.toLocaleString()} · {bucketSummary(buckets[hoverIdx])}
      </span>
    {/if}
  </div>

  {#if visibleNames.length > 0}
    <div class="hist-legend">
      {#each visibleNames as n}
        <span class="hist-legend-item">
          <span class="hist-legend-swatch" style="background: {colorFor(n)};"></span>
          <span class="mono">{displayName(n)}</span>
        </span>
      {/each}
    </div>
  {/if}

  {#if error}
    <div class="hist-error">Error en histograma: {error}</div>
  {/if}

  <svg
    bind:this={svgEl}
    viewBox="0 0 {widthPx} {height}"
    style="width: 100%; height: {height}px; cursor: crosshair; user-select: none; touch-action: none;"
    role="slider"
    aria-label="Histograma de eventos — arrastra para seleccionar un sub-rango"
    aria-valuemin="0"
    aria-valuemax={Math.max(1, buckets.length - 1)}
    aria-valuenow={hoverIdx ?? 0}
    tabindex="0"
    on:pointerdown={onPointerDown}
    on:pointermove={onPointerMove}
    on:pointerup={onPointerUp}
    on:pointerleave={onPointerLeave}
    on:pointercancel={onPointerUp}
  >
    <rect class="hist-bg" x={padding.left} y={padding.top} width={plotW} height={plotH} />

    {#each buckets as b, i (b.ts)}
      {@const x = xForIdx(i)}
      {@const barWidth = Math.max(0.5, barW - 1)}
      {#if b.total > 0}
        {@const totalH = (b.total / maxTotal) * plotH}
        {@const yTop = padding.top + plotH - totalH}
        {#each visibleNames as name}
          {@const c = b.counts[name] ?? 0}
          {#if c > 0}
            {@const below = cumulativeBelow(b, name)}
            {@const sliceH = (c / b.total) * totalH}
            <rect
              class="hist-bar"
              style="fill: {colorFor(name)};"
              x={x + 0.5}
              width={barWidth}
              y={yTop + totalH - ((below + c) / b.total) * totalH}
              height={Math.max(0.5, sliceH)}
            />
          {/if}
        {/each}
      {/if}
      {#if hoverIdx === i}
        <rect class="hist-hover" x={x} y={padding.top} width={Math.max(1, barW)} height={plotH} />
      {/if}
    {/each}

    {#if selectionRect}
      <rect class="hist-sel" x={selectionRect.x} y={padding.top} width={selectionRect.w} height={plotH} />
    {/if}
    {#if dragRect}
      <rect class="hist-drag" x={dragRect.x} y={padding.top} width={dragRect.w} height={plotH} />
    {/if}

    {#each xTicks as t}
      <text
        class="chart-axis"
        x={Math.min(widthPx - padding.right, Math.max(padding.left, xForIdx(t.idx) + barW / 2))}
        y={height - 4}
        text-anchor="middle">{t.label}</text>
    {/each}
  </svg>
</div>

<style>
  .hist {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px 4px;
    margin-bottom: 12px;
  }
  .hist-header {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 4px;
    font-size: 12px;
  }
  .hist-label { white-space: nowrap; }
  .hist-tooltip {
    margin-left: auto;
    font-size: 11.5px;
    color: var(--text-muted);
  }
  .hist-sel-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    background: rgba(250, 204, 21, 0.12);
    border: 1px solid var(--accent);
    color: var(--accent);
    border-radius: 12px;
    padding: 2px 8px;
    font-size: 11.5px;
    font-variant-numeric: tabular-nums;
  }
  .hist-clear {
    background: transparent;
    border: none;
    color: var(--accent);
    padding: 0 2px;
    cursor: pointer;
    font-size: 14px;
    line-height: 1;
  }
  .hist-clear:hover { color: #fff; background: transparent; }
  .hist-error {
    color: var(--danger);
    font-size: 12px;
    margin-bottom: 4px;
  }
  .hist-bg { fill: var(--bg); stroke: var(--border); stroke-width: 0.5; }
  .hist-bar { shape-rendering: crispEdges; }
  .hist-hover {
    fill: var(--text);
    fill-opacity: 0.05;
    pointer-events: none;
  }
  .hist-sel {
    fill: var(--accent);
    fill-opacity: 0.12;
    stroke: var(--accent);
    stroke-width: 1;
    stroke-dasharray: 3 2;
    pointer-events: none;
  }
  .hist-drag {
    fill: var(--accent);
    fill-opacity: 0.22;
    stroke: var(--accent);
    stroke-width: 1;
    pointer-events: none;
  }
  .hist-legend {
    display: flex;
    flex-wrap: wrap;
    gap: 8px 14px;
    margin-bottom: 4px;
    font-size: 11.5px;
    color: var(--text-muted);
  }
  .hist-legend-item {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .hist-legend-swatch {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 2px;
    flex-shrink: 0;
  }
</style>
