<script lang="ts">
  // The login page must bypass the global layout (sidebar + auth guard).
  // SvelteKit doesn't have an automatic opt-out, so we render only the slot here
  // and rely on the parent +layout.svelte to NOT render its chrome when path === '/login'.
</script>

<slot />
