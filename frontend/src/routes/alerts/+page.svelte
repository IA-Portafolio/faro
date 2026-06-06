<script lang="ts">
  /**
   * Ruta `/alerts`: alias histórico que redirige a `/settings/alerts`.
   *
   * No tiene contenido propio; existe para que enlaces y atajos antiguos a
   * `/alerts` (incluido el atajo de teclado `g a`) sigan funcionando tras mover la
   * gestión de alertas a Configuración.
   */
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';

  onMount(() => {
    goto('/settings/alerts', { replaceState: true });
  });
</script>

<div class="muted" style="padding: 20px;">
  <span class="spinner"></span>
  Redirigiendo a Configuración → Alertas…
</div>
