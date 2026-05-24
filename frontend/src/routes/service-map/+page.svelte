<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import { fetchServiceMap, type ServiceMap, type ServiceMapEdge, type ServiceMapNode } from '$lib/api';
  import { timeRange, rangeMinutes, selectedProject } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';

  type Vec = { x: number; y: number };
  type SimNode = SimNodeMeta & Vec & { vx: number; vy: number };
  type SimNodeMeta = ServiceMapNode & {
    radius: number;
    errorRate: number;
    inDegree: number;
    outDegree: number;
  };
  type SimEdge = ServiceMapEdge & {
    errorRate: number;
    weight: number; // 0..1 normalized para grosor
    reverse: boolean; // hay arista inversa en el grafo
  };

  // Lienzo. El viewBox es fijo y la simulación trabaja siempre en estas coordenadas;
  // el SVG se escala con la ventana sin re-simular.
  const VIEW_W = 1200;
  const VIEW_H = 680;
  const PAD = 80;

  let data: ServiceMap = { nodes: [], edges: [] };
  let nodes: SimNode[] = [];
  let edges: SimEdge[] = [];
  let nodeById = new Map<string, SimNode>();
  let loading = false;
  let error = '';

  let hoveredEdge: SimEdge | null = null;
  let hoveredNode: SimNode | null = null;
  let mouse: Vec = { x: 0, y: 0 };

  // Estado del drag — si el usuario arrastra un nodo lo fijamos y se sigue
  // simulando alrededor; se libera al soltar.
  let dragging: SimNode | null = null;
  let svgEl: SVGSVGElement | null = null;

  $: hasData = nodes.length > 0;
  $: totalCalls = edges.reduce((acc, e) => acc + e.calls, 0);
  $: totalErrors = edges.reduce((acc, e) => acc + e.errors, 0);

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      data = await fetchServiceMap({
        project: $selectedProject || undefined,
        last_minutes: rangeMinutes($timeRange)
      });
      rebuild(data);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  function rebuild(d: ServiceMap): void {
    // Indexa nodos del backend; añadir cualquier servicio presente en aristas
    // pero ausente en la lista de nodos (no debería pasar, pero defensivo).
    const seenServices = new Set<string>();
    const metas: SimNodeMeta[] = d.nodes.map((n) => {
      seenServices.add(n.service);
      const errorRate = n.calls > 0 ? n.errors / n.calls : 0;
      // Radio entre 18 y 38 según log(calls).
      const radius = 18 + Math.min(20, Math.log10(Math.max(1, n.calls)) * 7);
      return { ...n, radius, errorRate, inDegree: 0, outDegree: 0 };
    });
    for (const e of d.edges) {
      if (!seenServices.has(e.source)) {
        seenServices.add(e.source);
        metas.push({
          service: e.source, calls: 0, errors: 0, p95_ms: 0, is_root: 1,
          radius: 22, errorRate: 0, inDegree: 0, outDegree: 0
        });
      }
      if (!seenServices.has(e.target)) {
        seenServices.add(e.target);
        metas.push({
          service: e.target, calls: 0, errors: 0, p95_ms: 0, is_root: 0,
          radius: 22, errorRate: 0, inDegree: 0, outDegree: 0
        });
      }
    }

    // Layout inicial: círculo, ordenado por calls. Da convergencia más rápida que
    // posiciones random porque el grafo arranca cerca del equilibrio para grafos
    // densos típicos. Servicios root van más a la izquierda del círculo.
    metas.sort((a, b) => (b.is_root - a.is_root) || (b.calls - a.calls));
    const cx = VIEW_W / 2;
    const cy = VIEW_H / 2;
    const baseR = Math.min(VIEW_W, VIEW_H) / 2 - PAD - 20;
    const newNodes: SimNode[] = metas.map((m, i) => {
      const angle = (i / metas.length) * Math.PI * 2 - Math.PI / 2;
      return {
        ...m,
        x: cx + Math.cos(angle) * baseR,
        y: cy + Math.sin(angle) * baseR,
        vx: 0,
        vy: 0
      };
    });
    const byId = new Map<string, SimNode>();
    for (const n of newNodes) byId.set(n.service, n);

    // Detecta aristas bidireccionales para curvar la línea hacia un lado.
    const edgeKey = (a: string, b: string) => `${a}${b}`;
    const edgeSet = new Set<string>(d.edges.map((e) => edgeKey(e.source, e.target)));
    const maxCalls = d.edges.reduce((m, e) => Math.max(m, e.calls), 0) || 1;
    const newEdges: SimEdge[] = d.edges
      .filter((e) => byId.has(e.source) && byId.has(e.target) && e.source !== e.target)
      .map((e) => {
        const reverse = edgeSet.has(edgeKey(e.target, e.source));
        const errorRate = e.calls > 0 ? e.errors / e.calls : 0;
        // Peso log-normalizado para que el rango de grosores no se aplaste.
        const weight = Math.log10(e.calls + 1) / Math.log10(maxCalls + 1);
        return { ...e, reverse, errorRate, weight };
      });

    // Cuenta grados para tamaño y para nada más por ahora.
    for (const e of newEdges) {
      byId.get(e.source)!.outDegree += 1;
      byId.get(e.target)!.inDegree += 1;
    }

    nodes = newNodes;
    edges = newEdges;
    nodeById = byId;

    runSimulation();
  }

  // Mini simulación force-directed. ~300 ticks suelen converger para grafos
  // de ≤ 80 nodos en pocos ms. Sin requestAnimationFrame para que el dibujo
  // ya salga estable.
  function runSimulation(ticks: number = 350): void {
    if (nodes.length === 0) return;
    const K_REPEL = 9000;
    const K_SPRING = 0.03;
    const REST_LEN = 180;
    const K_CENTER = 0.008;
    const DAMP = 0.82;
    const MAX_V = 28;
    const cx = VIEW_W / 2;
    const cy = VIEW_H / 2;

    for (let t = 0; t < ticks; t++) {
      // Repulsión todos vs todos.
      for (let i = 0; i < nodes.length; i++) {
        const a = nodes[i];
        if (a === dragging) continue;
        for (let j = i + 1; j < nodes.length; j++) {
          const b = nodes[j];
          const dx = a.x - b.x;
          const dy = a.y - b.y;
          let d2 = dx * dx + dy * dy;
          if (d2 < 1) d2 = 1;
          const inv = K_REPEL / d2;
          const d = Math.sqrt(d2);
          const fx = (dx / d) * inv;
          const fy = (dy / d) * inv;
          a.vx += fx; a.vy += fy;
          if (b !== dragging) { b.vx -= fx; b.vy -= fy; }
        }
      }
      // Atracción por aristas.
      for (const e of edges) {
        const a = nodeById.get(e.source)!;
        const b = nodeById.get(e.target)!;
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 1;
        const force = K_SPRING * (d - REST_LEN);
        const fx = (dx / d) * force;
        const fy = (dy / d) * force;
        if (a !== dragging) { a.vx += fx; a.vy += fy; }
        if (b !== dragging) { b.vx -= fx; b.vy -= fy; }
      }
      // Centrado.
      for (const n of nodes) {
        if (n === dragging) continue;
        n.vx += (cx - n.x) * K_CENTER;
        n.vy += (cy - n.y) * K_CENTER;
      }
      // Integrar y clamp.
      for (const n of nodes) {
        if (n === dragging) {
          n.vx = 0; n.vy = 0;
          continue;
        }
        n.vx *= DAMP;
        n.vy *= DAMP;
        const vmag = Math.hypot(n.vx, n.vy);
        if (vmag > MAX_V) {
          n.vx = (n.vx / vmag) * MAX_V;
          n.vy = (n.vy / vmag) * MAX_V;
        }
        n.x += n.vx;
        n.y += n.vy;
        // Mantener dentro del viewport con margen.
        const r = n.radius;
        if (n.x < PAD + r) n.x = PAD + r;
        if (n.x > VIEW_W - PAD - r) n.x = VIEW_W - PAD - r;
        if (n.y < PAD + r) n.y = PAD + r;
        if (n.y > VIEW_H - PAD - r) n.y = VIEW_H - PAD - r;
      }
    }
    // Trigger reactivity en Svelte 5.
    nodes = nodes;
  }

  // ---- Color helpers ----
  // Devuelve un color HSL en degradado verde→amarillo→rojo según error rate (0..1).
  function errorColor(rate: number, alpha = 1): string {
    const r = Math.max(0, Math.min(1, rate));
    // 0 → 140 (verde), 0.05 → 50 (amarillo), 0.20+ → 0 (rojo)
    let hue: number;
    if (r < 0.01) hue = 140;
    else if (r < 0.05) hue = 140 - (r / 0.05) * 90;     // 140 → 50
    else if (r < 0.2) hue = 50 - ((r - 0.05) / 0.15) * 50; // 50 → 0
    else hue = 0;
    return `hsla(${hue}, 75%, 50%, ${alpha})`;
  }

  function fmtCalls(n: number): string {
    if (n < 1000) return String(n);
    if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`;
    return `${(n / 1_000_000).toFixed(1)}M`;
  }

  function fmtPct(rate: number): string {
    if (rate === 0) return '0%';
    if (rate < 0.001) return '<0.1%';
    return `${(rate * 100).toFixed(1)}%`;
  }

  // ---- Geometría de las aristas ----
  // Bezier cuadrática con punto de control perpendicular al segmento, lo que da
  // un arco. Si hay arista inversa, separamos las dos curvas en lados opuestos.
  function edgePath(e: SimEdge): string {
    const a = nodeById.get(e.source);
    const b = nodeById.get(e.target);
    if (!a || !b) return '';
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy) || 1;
    // Acorta los extremos para que el arrowhead no se meta dentro del círculo.
    const ux = dx / len;
    const uy = dy / len;
    const startX = a.x + ux * a.radius;
    const startY = a.y + uy * a.radius;
    const endX = b.x - ux * (b.radius + 8);
    const endY = b.y - uy * (b.radius + 8);
    // Vector perpendicular (hacia la derecha del segmento).
    const px = -uy;
    const py = ux;
    const curveK = e.reverse ? 0.18 : 0.08;
    const mx = (startX + endX) / 2 + px * len * curveK;
    const my = (startY + endY) / 2 + py * len * curveK;
    return `M ${startX} ${startY} Q ${mx} ${my} ${endX} ${endY}`;
  }

  // Punto medio del arco para colocar la etiqueta de calls.
  function edgeLabelPos(e: SimEdge): Vec | null {
    const a = nodeById.get(e.source);
    const b = nodeById.get(e.target);
    if (!a || !b) return null;
    const dx = b.x - a.x;
    const dy = b.y - a.y;
    const len = Math.hypot(dx, dy) || 1;
    const ux = dx / len;
    const uy = dy / len;
    const px = -uy;
    const py = ux;
    const curveK = e.reverse ? 0.18 : 0.08;
    // Apunta cerca del punto medio del arco.
    const mx = (a.x + b.x) / 2 + px * len * curveK * 1.1;
    const my = (a.y + b.y) / 2 + py * len * curveK * 1.1;
    return { x: mx, y: my };
  }

  // ---- Eventos ----
  function viewportPoint(ev: MouseEvent): Vec {
    if (!svgEl) return { x: 0, y: 0 };
    const pt = svgEl.createSVGPoint();
    pt.x = ev.clientX;
    pt.y = ev.clientY;
    const ctm = svgEl.getScreenCTM();
    if (!ctm) return { x: 0, y: 0 };
    const tp = pt.matrixTransform(ctm.inverse());
    return { x: tp.x, y: tp.y };
  }

  function onMouseMove(ev: MouseEvent): void {
    mouse = { x: ev.clientX, y: ev.clientY };
    if (dragging) {
      const p = viewportPoint(ev);
      dragging.x = p.x;
      dragging.y = p.y;
      nodes = nodes;
    }
  }

  function startDrag(ev: MouseEvent, n: SimNode): void {
    ev.preventDefault();
    dragging = n;
    window.addEventListener('mousemove', onMouseMove);
    window.addEventListener('mouseup', endDrag, { once: true });
  }

  function endDrag(): void {
    window.removeEventListener('mousemove', onMouseMove);
    if (dragging) {
      // Pequeño settle para que los vecinos reaccionen tras soltar.
      dragging = null;
      runSimulation(80);
    }
  }

  function openTraces(n: SimNode): void {
    // Si el click fue al final de un drag, no navegues.
    goto(`/traces?service=${encodeURIComponent(n.service)}`);
  }

  let suppressClick = false;
  function handleNodeClick(n: SimNode): void {
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    openTraces(n);
  }
  function handleNodeMouseDown(ev: MouseEvent, n: SimNode): void {
    // Marca click-cancel si se mueve durante el drag, para distinguir click vs drag.
    let moved = false;
    const onMove = () => { moved = true; };
    window.addEventListener('mousemove', onMove);
    const onUp = () => {
      window.removeEventListener('mousemove', onMove);
      if (moved) suppressClick = true;
    };
    window.addEventListener('mouseup', onUp, { once: true });
    startDrag(ev, n);
  }

  onMount(load);
  onDestroy(() => {
    window.removeEventListener('mousemove', onMouseMove);
  });

  let prevRange = $timeRange;
  let prevProject = $selectedProject;
  $: {
    if (prevRange !== $timeRange || prevProject !== $selectedProject) {
      prevRange = $timeRange;
      prevProject = $selectedProject;
      load();
    }
  }
</script>

<div class="page-header">
  <h1 class="page-title">Service map</h1>
  <div class="flex gap-12 center">
    <TimeRangePicker />
    <button on:click={load} disabled={loading}>
      {loading ? 'Cargando…' : '↻ Recargar'}
    </button>
    <button on:click={() => runSimulation(200)} disabled={!hasData} title="Re-acomoda los nodos">
      ✦ Re-layout
    </button>
  </div>
</div>

<div class="map-stats">
  <div><span class="muted">Servicios:</span> <strong>{nodes.length}</strong></div>
  <div><span class="muted">Aristas:</span> <strong>{edges.length}</strong></div>
  <div><span class="muted">Llamadas:</span> <strong>{fmtCalls(totalCalls)}</strong></div>
  <div><span class="muted">Errores:</span> <strong style:color={totalErrors > 0 ? 'var(--danger)' : undefined}>{fmtCalls(totalErrors)}</strong></div>
  <div class="legend">
    <span class="dot" style:background={errorColor(0)}></span> sano
    <span class="dot" style:background={errorColor(0.05)}></span> degradado
    <span class="dot" style:background={errorColor(0.25)}></span> en error
  </div>
</div>

{#if error}
  <div style="color: var(--danger); padding: 12px;">Error: {error}</div>
{/if}

<div class="map-frame">
  {#if !loading && nodes.length === 0}
    <div class="empty">
      No hay spans con relaciones servicio→servicio en este rango.
      Genera tráfico con SDKs OTLP que propaguen contexto entre servicios.
    </div>
  {:else}
    <svg
      bind:this={svgEl}
      viewBox="0 0 {VIEW_W} {VIEW_H}"
      preserveAspectRatio="xMidYMid meet"
      role="img"
      aria-label="Mapa de servicios"
      on:mousemove={(ev) => (mouse = { x: ev.clientX, y: ev.clientY })}
    >
      <defs>
        <marker id="arrow-green" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
          <path d="M 0 0 L 10 5 L 0 10 z" fill={errorColor(0)} />
        </marker>
        <marker id="arrow-yellow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
          <path d="M 0 0 L 10 5 L 0 10 z" fill={errorColor(0.05)} />
        </marker>
        <marker id="arrow-red" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
          <path d="M 0 0 L 10 5 L 0 10 z" fill={errorColor(0.25)} />
        </marker>
      </defs>

      {#each edges as e (e.source + '->' + e.target)}
        {@const path = edgePath(e)}
        {@const lp = edgeLabelPos(e)}
        {@const stroke = errorColor(e.errorRate)}
        {@const width = 1.2 + e.weight * 5}
        {@const isHover = hoveredEdge === e || hoveredNode?.service === e.source || hoveredNode?.service === e.target}
        {@const marker = e.errorRate < 0.01 ? 'arrow-green' : e.errorRate < 0.05 ? 'arrow-yellow' : 'arrow-red'}
        <path
          d={path}
          fill="none"
          stroke={stroke}
          stroke-width={isHover ? width + 1.5 : width}
          stroke-opacity={hoveredNode && !isHover ? 0.18 : 0.85}
          marker-end="url(#{marker})"
          class="edge"
          on:mouseenter={() => (hoveredEdge = e)}
          on:mouseleave={() => (hoveredEdge = null)}
        />
        {#if lp && (e.weight > 0.35 || isHover)}
          <text x={lp.x} y={lp.y} class="edge-label" text-anchor="middle" dy="-3">
            {fmtCalls(e.calls)}
          </text>
        {/if}
      {/each}

      {#each nodes as n (n.service)}
        {@const fill = errorColor(n.errorRate, 0.18)}
        {@const stroke = errorColor(n.errorRate)}
        {@const isHover = hoveredNode === n}
        {@const dim = hoveredNode && !isHover && !edges.some((e) => (e.source === n.service || e.target === n.service) && (e.source === hoveredNode?.service || e.target === hoveredNode?.service))}
        <g
          class="node"
          class:dim
          transform="translate({n.x},{n.y})"
          on:mouseenter={() => (hoveredNode = n)}
          on:mouseleave={() => (hoveredNode = null)}
          on:mousedown={(ev) => handleNodeMouseDown(ev, n)}
          on:click={() => handleNodeClick(n)}
          on:keypress={(ev) => ev.key === 'Enter' && openTraces(n)}
          role="button"
          tabindex="0"
        >
          <circle r={n.radius} fill={fill} stroke={stroke} stroke-width="2.5" />
          {#if n.is_root === 1}
            <circle r={n.radius + 5} fill="none" stroke={stroke} stroke-width="1" stroke-dasharray="2 4" opacity="0.6" />
          {/if}
          <text class="node-label" text-anchor="middle" dy="4">{n.service}</text>
        </g>
      {/each}
    </svg>

    {#if hoveredEdge}
      <div class="tooltip" style:left="{mouse.x + 14}px" style:top="{mouse.y + 14}px">
        <div class="tt-title">{hoveredEdge.source} → {hoveredEdge.target}</div>
        <div class="tt-row"><span>Llamadas</span><strong>{hoveredEdge.calls.toLocaleString()}</strong></div>
        <div class="tt-row"><span>Errores</span>
          <strong style:color={hoveredEdge.errors > 0 ? 'var(--danger)' : undefined}>
            {hoveredEdge.errors.toLocaleString()} ({fmtPct(hoveredEdge.errorRate)})
          </strong>
        </div>
        <div class="tt-row"><span>p50</span><strong>{hoveredEdge.p50_ms} ms</strong></div>
        <div class="tt-row"><span>p95</span><strong>{hoveredEdge.p95_ms} ms</strong></div>
        <div class="tt-row"><span>p99</span><strong>{hoveredEdge.p99_ms} ms</strong></div>
      </div>
    {:else if hoveredNode}
      <div class="tooltip" style:left="{mouse.x + 14}px" style:top="{mouse.y + 14}px">
        <div class="tt-title">
          {hoveredNode.service}
          {#if hoveredNode.is_root === 1}<span class="badge-root">root</span>{/if}
        </div>
        <div class="tt-row"><span>Spans</span><strong>{hoveredNode.calls.toLocaleString()}</strong></div>
        <div class="tt-row"><span>Errores</span>
          <strong style:color={hoveredNode.errors > 0 ? 'var(--danger)' : undefined}>
            {hoveredNode.errors.toLocaleString()} ({fmtPct(hoveredNode.errorRate)})
          </strong>
        </div>
        <div class="tt-row"><span>p95</span><strong>{hoveredNode.p95_ms} ms</strong></div>
        <div class="tt-row"><span>Entrantes</span><strong>{hoveredNode.inDegree}</strong></div>
        <div class="tt-row"><span>Salientes</span><strong>{hoveredNode.outDegree}</strong></div>
        <div class="tt-hint">Click para ver trazas de este servicio</div>
      </div>
    {/if}
  {/if}
</div>

<style>
  .map-stats {
    display: flex;
    align-items: center;
    gap: 22px;
    flex-wrap: wrap;
    padding: 10px 14px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-bottom: 12px;
    font-size: 13px;
  }
  .map-stats .legend {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-muted);
    margin-left: auto;
  }
  .map-stats .dot {
    display: inline-block;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    margin-right: 2px;
    margin-left: 8px;
  }
  .map-stats .dot:first-of-type { margin-left: 4px; }

  .map-frame {
    position: relative;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    min-height: 500px;
  }
  .map-frame svg {
    width: 100%;
    height: auto;
    display: block;
  }

  .edge {
    cursor: pointer;
    transition: stroke-opacity 0.12s ease;
  }
  .edge-label {
    font-family: "JetBrains Mono", Menlo, monospace;
    font-size: 11px;
    fill: var(--text-muted);
    pointer-events: none;
    paint-order: stroke;
    stroke: var(--bg-elev);
    stroke-width: 3px;
  }

  .node {
    cursor: pointer;
    transition: opacity 0.12s ease;
  }
  .node.dim { opacity: 0.35; }
  .node text {
    font-size: 12px;
    font-weight: 600;
    fill: var(--text);
    pointer-events: none;
    paint-order: stroke;
    stroke: var(--bg-elev);
    stroke-width: 3px;
  }

  .tooltip {
    position: fixed;
    pointer-events: none;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    font-size: 12px;
    box-shadow: 0 6px 22px rgba(0, 0, 0, 0.3);
    z-index: 200;
    min-width: 200px;
  }
  .tt-title {
    font-weight: 600;
    margin-bottom: 6px;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .tt-row {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    font-variant-numeric: tabular-nums;
  }
  .tt-row span { color: var(--text-muted); }
  .tt-hint {
    margin-top: 6px;
    padding-top: 6px;
    border-top: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 11px;
  }
  .badge-root {
    font-size: 10px;
    padding: 1px 6px;
    border: 1px solid var(--border);
    border-radius: 8px;
    color: var(--text-muted);
    font-weight: 400;
  }
</style>
