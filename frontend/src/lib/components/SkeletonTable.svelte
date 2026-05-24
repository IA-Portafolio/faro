<script lang="ts">
  /**
   * Skeleton para tablas con `<table>` estándar (Resumen, traces, errors,
   * monitors, alerts, users, projects). Pinta `rows` filas con `cols`
   * columnas; cada celda contiene un bloque Skeleton del ancho indicado.
   *
   * `widths`: array de % o px. Ejemplo: `['16%', '24%', '12%', '20%', '12%', '8%', '8%']`.
   * Si no se pasa, distribuye uniformemente entre `cols` columnas.
   *
   * Renderiza directamente `<tr><td>` para que se inserten como filas dentro
   * de un `<tbody>` ya existente — así reusamos el padding/borde de la tabla
   * real y el placeholder y los datos comparten exactamente el mismo grid.
   */
  import Skeleton from './Skeleton.svelte';

  export let rows: number = 8;
  export let cols: number = 4;
  export let widths: string[] | null = null;

  $: actualWidths = (() => {
    if (widths && widths.length === cols) return widths;
    const pct = Math.floor(100 / cols) + '%';
    return Array.from({ length: cols }, () => pct);
  })();

  function randPct(base: number, jitter: number): string {
    // Da un poco de varianza a cada fila para que no se vean en barras alineadas
    // perfectamente — más realista visualmente.
    const offset = (Math.random() - 0.5) * jitter;
    return Math.max(40, Math.min(96, base + offset)) + '%';
  }
</script>

{#each Array(rows) as _, r}
  <tr aria-hidden="true">
    {#each actualWidths as w, c}
      <td>
        <Skeleton width={w.endsWith('%') ? randPct(parseInt(w), 18) : w} height="13px" />
      </td>
    {/each}
  </tr>
{/each}
