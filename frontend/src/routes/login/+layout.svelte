<script lang="ts">
  /**
   * Layout de la sección `/login`: renderiza solo el contenido del login.
   *
   * Es un slot "pelado" a propósito — el chrome global (sidebar, atajos, guardia
   * de auth) lo omite el layout raíz cuando la ruta empieza por `/login`, para que
   * la pantalla de acceso no muestre la navegación de la app.
   */
  // La página de login debe saltarse el layout global (sidebar + guardia de auth).
  // SvelteKit no tiene opt-out automático, así que aquí renderizamos solo el slot
  // y dependemos de que el +layout.svelte padre NO renderice su chrome cuando path === '/login'.
</script>

<slot />
