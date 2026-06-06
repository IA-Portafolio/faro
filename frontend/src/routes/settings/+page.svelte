<script lang="ts">
  /**
   * Ruta `/settings` (índice): redirige a la primera pestaña, `/settings/appearance`.
   *
   * No tiene UI propia; Configuración no tiene "portada", así que se entra directo
   * a Apariencia.
   */
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
