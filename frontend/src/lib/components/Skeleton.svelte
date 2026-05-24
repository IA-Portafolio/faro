<script lang="ts">
  /**
   * Primitivo de skeleton: un bloque rectangular con el shimmer global.
   * Las dimensiones las controla el caller para que el placeholder coincida
   * con el shape final y no haya reflow al sustituirlo por el contenido real.
   *
   * Ejemplos:
   *   <Skeleton width="120px" height="14px" />
   *   <Skeleton width="100%" height="220px" radius="6px" />
   *   <Skeleton width="60%" />   <!-- usa la altura por defecto (14px) -->
   */
  export let width: string = '100%';
  export let height: string = '14px';
  /** Radio del rectángulo. 999 para pill, 4 para texto, 50% para avatar. */
  export let radius: string = '4px';
</script>

<span
  class="skeleton"
  aria-hidden="true"
  style="width: {width}; height: {height}; border-radius: {radius};"
></span>
