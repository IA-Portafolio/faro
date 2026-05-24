<script lang="ts">
  import { goto } from '$app/navigation';
  import { onMount } from 'svelte';

  // /projects vivió como página de primer nivel. Ahora forma parte de
  // Configuración. Mantenemos esta ruta para no romper bookmarks ni enlaces
  // antiguos: redirige a la nueva ubicación de forma transparente.
  onMount(() => {
    goto('/settings/projects', { replaceState: true });
  });
</script>

<div class="muted" style="padding: 20px;">
  <span class="spinner"></span>
  Redirigiendo a Configuración → Proyectos…
</div>
