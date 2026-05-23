<script lang="ts">
  // La página de login debe saltarse el layout global (sidebar + guardia de auth).
  // SvelteKit no tiene opt-out automático, así que aquí renderizamos solo el slot
  // y dependemos de que el +layout.svelte padre NO renderice su chrome cuando path === '/login'.
</script>

<slot />
