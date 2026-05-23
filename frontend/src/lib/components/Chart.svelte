<script lang="ts">
  export let points: { ts: string; value: number }[] = [];
  export let height = 200;
  export let label = '';

  const width = 800;
  const padding = { top: 10, right: 12, bottom: 24, left: 48 };

  $: numeric = points.map((p, i) => ({ x: i, ts: p.ts, y: p.value }));
  $: minY = Math.min(0, ...numeric.map((p) => p.y));
  $: maxY = Math.max(1, ...numeric.map((p) => p.y));
  $: yRange = maxY - minY || 1;

  $: plotW = width - padding.left - padding.right;
  $: plotH = height - padding.top - padding.bottom;

  $: stepX = numeric.length > 1 ? plotW / (numeric.length - 1) : plotW;

  $: path = numeric
    .map((p, i) => {
      const x = padding.left + i * stepX;
      const y = padding.top + plotH - ((p.y - minY) / yRange) * plotH;
      return `${i === 0 ? 'M' : 'L'} ${x} ${y}`;
    })
    .join(' ');

  $: area = path
    ? `${path} L ${padding.left + (numeric.length - 1) * stepX} ${padding.top + plotH} L ${padding.left} ${padding.top + plotH} Z`
    : '';

  $: gridLines = [0, 0.25, 0.5, 0.75, 1].map((f) => {
    const y = padding.top + plotH - f * plotH;
    return { y, value: minY + f * yRange };
  });

  $: xTicks = numeric.length > 0
    ? [0, Math.floor(numeric.length / 2), numeric.length - 1].map((i) => ({
        x: padding.left + i * stepX,
        label: numeric[i]?.ts ?? ''
      }))
    : [];

  function fmtTick(s: string): string {
    if (!s) return '';
    const d = new Date(s.includes('T') ? s : s.replace(' ', 'T') + 'Z');
    if (isNaN(d.getTime())) return s.slice(11, 16);
    return d.toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit' });
  }

  function fmtValue(v: number): string {
    if (Math.abs(v) >= 1000) return v.toFixed(0);
    if (Math.abs(v) >= 1) return v.toFixed(2);
    return v.toFixed(4);
  }
</script>

<div style="width: 100%; overflow: hidden;">
  {#if label}<div class="muted" style="font-size: 12px; margin-bottom: 4px;">{label}</div>{/if}
  {#if numeric.length === 0}
    <div class="empty">No data</div>
  {:else}
    <svg viewBox="0 0 {width} {height}" style="width: 100%; height: {height}px;" preserveAspectRatio="none">
      {#each gridLines as gl}
        <line class="chart-grid" x1={padding.left} y1={gl.y} x2={width - padding.right} y2={gl.y} />
        <text class="chart-axis" x={padding.left - 6} y={gl.y + 3} text-anchor="end">{fmtValue(gl.value)}</text>
      {/each}
      <path class="chart-area" d={area} />
      <path class="chart-line" d={path} />
      {#each xTicks as t}
        <text class="chart-axis" x={t.x} y={height - 6} text-anchor="middle">{fmtTick(t.label)}</text>
      {/each}
    </svg>
  {/if}
</div>
