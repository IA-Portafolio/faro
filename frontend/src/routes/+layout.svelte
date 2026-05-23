<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import '../app.css';
  import { me } from '$lib/api';
  import { currentUser } from '$lib/stores';
  import Sidebar from '$lib/components/Sidebar.svelte';

  let ready = false;

  // La página de login maneja su propia comprobación de auth. Todo lo demás necesita sesión.
  $: isLogin = $page.url.pathname.startsWith('/login');

  onMount(async () => {
    if (isLogin) {
      ready = true;
      return;
    }
    try {
      const u = await me();
      currentUser.set(u);
      ready = true;
    } catch (_e) {
      // Redirige a /login preservando el destino original.
      const next = $page.url.pathname + $page.url.search;
      await goto('/login?next=' + encodeURIComponent(next), { replaceState: true });
    }
  });
</script>

{#if isLogin}
  <slot />
{:else if ready}
  <div class="layout">
    <Sidebar />
    <main class="main">
      <slot />
    </main>
  </div>
{:else}
  <div style="min-height: 100vh; display: grid; place-items: center; color: var(--text-muted);">
    <span class="spinner"></span>
  </div>
{/if}
