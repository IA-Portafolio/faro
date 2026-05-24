<script lang="ts">
  /**
   * Skeleton específico para la lista de logs. La fila real usa
   * `display: grid; grid-template-columns: 180px 80px 140px 1fr` con
   * padding `6px 8px` (ver `.log-row` en app.css). Replicamos exactamente
   * ese grid para que la carga no provoque reflow vertical.
   */
  import Skeleton from './Skeleton.svelte';

  export let rows: number = 12;

  function bodyWidth(i: number): string {
    // Varía el ancho del "mensaje" por fila para que se vea natural.
    const widths = ['90%', '76%', '84%', '58%', '92%', '70%', '64%', '88%', '52%', '80%', '74%', '66%'];
    return widths[i % widths.length];
  }
</script>

{#each Array(rows) as _, i}
  <div class="log-row" aria-hidden="true" style="cursor: default;">
    <div><Skeleton width="170px" height="12px" /></div>
    <div><Skeleton width="56px" height="14px" radius="10px" /></div>
    <div><Skeleton width="90%" height="12px" /></div>
    <div><Skeleton width={bodyWidth(i)} height="12px" /></div>
  </div>
{/each}
