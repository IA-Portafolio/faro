<script lang="ts">
  import { onMount } from 'svelte';
  import { browser } from '$app/environment';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import '../app.css';
  import { me, fetchPreferences } from '$lib/api';
  import {
    applyGlobalUrlParams,
    currentUser,
    isValidRange,
    selectedProject,
    timeRange
  } from '$lib/stores';
  import { bootstrapThemeFromLocal, hydrateFromServer } from '$lib/theme';
  import Sidebar from '$lib/components/Sidebar.svelte';
  import CommandPalette from '$lib/components/CommandPalette.svelte';
  import KeyboardHelp from '$lib/components/KeyboardHelp.svelte';
  import KeyboardShortcuts from '$lib/components/KeyboardShortcuts.svelte';
  import Toasts from '$lib/components/Toasts.svelte';

  let ready = false;

  // La página de login maneja su propia comprobación de auth. Todo lo demás necesita sesión.
  $: isLogin = $page.url.pathname.startsWith('/login');

  // Aplica el tema desde localStorage cuanto antes — antes de cualquier render
  // de la app — para evitar flash de tema incorrecto.
  bootstrapThemeFromLocal();

  // El query string siempre gana: si el usuario abrió una URL con `?project=`
  // o `?range=`, esos valores se aplican antes de pedir al backend para que
  // un fetch lento no pise el deep link recién abierto.
  const urlOverrides = applyGlobalUrlParams();

  /**
   * Reescribe `?project=` y `?range=` en la URL actual sin tocar el resto del
   * query string. Es la dirección inversa de `applyGlobalUrlParams`: cuando
   * el usuario cambia el proyecto desde el sidebar o el rango desde el
   * TimeRangePicker, queremos que un F5 reconstruya la misma vista.
   */
  function syncGlobalToUrl(project: string, range: string): void {
    if (!browser) return;
    const u = new URL(window.location.href);
    if (project) u.searchParams.set('project', project);
    else u.searchParams.delete('project');
    if (range && range !== '1h') u.searchParams.set('range', range);
    else u.searchParams.delete('range');
    try {
      window.history.replaceState(null, '', u.toString());
    } catch {
      /* ignora — algunos navegadores rechazan replaceState bajo carga */
    }
  }

  // Subscribirse con reactividad: cada cambio en los stores globales se
  // refleja en la URL. La login page no monta el layout autenticado, así
  // que solo se ejecuta cuando hay sesión.
  $: if (browser && ready) syncGlobalToUrl($selectedProject, $timeRange);

  onMount(async () => {
    if (isLogin) {
      ready = true;
      return;
    }
    try {
      const u = await me();
      currentUser.set(u);
      // Carga la preferencia persistida en backend y sincroniza si difiere.
      try {
        const prefs = await fetchPreferences();
        hydrateFromServer(prefs.theme);
        // Defaults de exploración: solo se aplican si la URL no los traía.
        if (!urlOverrides.hasProject) {
          selectedProject.set(prefs.default_project ?? '');
        }
        if (!urlOverrides.hasRange && isValidRange(prefs.default_time_range)) {
          timeRange.set(prefs.default_time_range);
        }
      } catch {
        /* no bloquea el render — el tema ya está aplicado desde localStorage */
      }
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
  <KeyboardShortcuts />
  <CommandPalette />
  <KeyboardHelp />
{:else}
  <div style="min-height: 100vh; display: grid; place-items: center; color: var(--text-muted);">
    <span class="spinner"></span>
  </div>
{/if}

<!-- Toasts globales: disponibles en cualquier ruta (incluido /login) y
     persistentes entre transiciones de página. -->
<Toasts />
