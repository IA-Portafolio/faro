<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';

  onMount(() => {
    goto('/settings/appearance', { replaceState: true });
  });
</script>

<div class="muted" style="padding: 20px;">
  <span class="spinner"></span>
  Redirigiendo a configuración…
</div>
