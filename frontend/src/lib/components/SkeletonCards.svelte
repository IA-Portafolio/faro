<script lang="ts">
  /**
   * Skeleton para el grid de `.cards` del resumen. Reusa la clase `.card`
   * para que el padding y el border-radius coincidan con la card real.
   */
  import Skeleton from './Skeleton.svelte';

  export let count: number = 7;
</script>

<div class="cards" aria-hidden="true">
  {#each Array(count) as _}
    <div class="card">
      <Skeleton width="60%" height="11px" />
      <div style="margin-top: 8px;">
        <Skeleton width="40%" height="22px" />
      </div>
    </div>
  {/each}
</div>
