<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { selectedProject, currentUser } from '$lib/stores';
  import { fetchProjects, logout, type Project } from '$lib/api';
  import { themeChoice, setTheme, type ThemeChoice } from '$lib/theme';
  import { paletteOpen, helpOpen } from '$lib/keyboard';

  const links = [
    { href: '/', label: 'Resumen', icon: '◐' },
    { href: '/logs', label: 'Logs', icon: '≡' },
    { href: '/traces', label: 'Trazas', icon: '⤳' },
    { href: '/service-map', label: 'Service map', icon: '⌬' },
    { href: '/metrics', label: 'Métricas', icon: '◢' },
    { href: '/errors', label: 'Errores', icon: '⚠' },
    { href: '/events', label: 'Eventos', icon: '◆' },
    { href: '/users', label: 'Usuarios', icon: '◌' },
    { href: '/funnels', label: 'Funnels', icon: '▽' },
    { href: '/cohorts', label: 'Cohorts', icon: '◇' },
    { href: '/monitors', label: 'Monitores', icon: '◉' },
    { href: '/settings', label: 'Configuración', icon: '⚙' }
  ];

  let projects: Project[] = [];
  let loadingProjects = true;

  async function loadProjects(): Promise<void> {
    try {
      projects = await fetchProjects();
    } catch (_e) {
      projects = [];
    } finally {
      loadingProjects = false;
    }
  }

  onMount(loadProjects);

  $: current = $page.url.pathname;
  function isActive(href: string): boolean {
    if (href === '/') return current === '/';
    return current === href || current.startsWith(href + '/');
  }

  async function doLogout(): Promise<void> {
    try {
      await logout();
    } catch (_e) {
      // ignorar — igual redirigimos
    }
    currentUser.set(null);
    await goto('/login', { replaceState: true });
  }
</script>

<aside class="sidebar">
  <div class="brand">
    <span class="brand-dot"></span>
    <span>Faro</span>
  </div>

  <div style="padding: 0 16px 12px;">
    <label style="font-size: 11px; color: var(--text-muted); text-transform: uppercase; letter-spacing: 0.5px;">Proyecto</label>
    <select bind:value={$selectedProject} style="width: 100%; margin-top: 4px;">
      <option value="">Todos los proyectos</option>
      {#each projects as p}
        <option value={p.slug}>{p.name}</option>
      {/each}
    </select>
    {#if !loadingProjects && projects.length === 0}
      <div style="margin-top: 6px; font-size: 11px;">
        <a href="/settings/projects" style="color: var(--accent);">Crear el primer proyecto →</a>
      </div>
    {/if}
  </div>

  <nav>
    {#each links as l}
      <a href={l.href} class:active={isActive(l.href)} data-sveltekit-preload-data="hover">
        <span class="nav-icon mono">{l.icon}</span>
        <span>{l.label}</span>
      </a>
    {/each}
  </nav>

  <div style="padding: 12px 16px; margin-top: auto; border-top: 1px solid var(--border);">
    <div class="theme-toggle" role="radiogroup" aria-label="Tema del panel">
      {#each [
        { value: 'light' as ThemeChoice, icon: '☀', title: 'Tema claro' },
        { value: 'system' as ThemeChoice, icon: '◐', title: 'Seguir al sistema' },
        { value: 'dark' as ThemeChoice, icon: '☾', title: 'Tema oscuro' }
      ] as t}
        <button
          type="button"
          role="radio"
          aria-checked={$themeChoice === t.value}
          title={t.title}
          on:click={() => setTheme(t.value)}
          class:active={$themeChoice === t.value}
        >
          <span class="mono" aria-hidden="true">{t.icon}</span>
        </button>
      {/each}
    </div>

    <div class="kbd-row">
      <button
        type="button"
        class="kbd-btn"
        on:click={() => paletteOpen.set(true)}
        title="Abrir paleta de comandos"
      >
        <span>Comandos</span>
        <span class="kbd-keys mono"><kbd>⌘</kbd><kbd>K</kbd></span>
      </button>
      <button
        type="button"
        class="kbd-btn"
        on:click={() => helpOpen.set(true)}
        title="Ver atajos de teclado"
      >
        <span>Atajos</span>
        <span class="kbd-keys mono"><kbd>?</kbd></span>
      </button>
    </div>

    {#if $currentUser}
      <div style="font-size: 12px; margin-top: 12px;">
        <div style="font-weight: 600;">{$currentUser.name || $currentUser.email}</div>
        <div class="muted" style="font-size: 11px;">{$currentUser.email}</div>
      </div>
      <button on:click={doLogout} style="margin-top: 8px; width: 100%; font-size: 12px;">Salir</button>
    {/if}
    <div class="muted" style="font-size: 10px; margin-top: 10px;">faro v0.3.0</div>
  </div>
</aside>

<style>
  .theme-toggle {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .theme-toggle button {
    background: transparent;
    border: 0;
    border-radius: 0;
    padding: 6px 0;
    cursor: pointer;
    color: var(--text-muted);
    font-size: 14px;
    line-height: 1;
  }
  .theme-toggle button:hover { background: var(--bg-hover); color: var(--text); }
  .theme-toggle button.active {
    background: var(--accent);
    color: var(--accent-fg);
  }

  .kbd-row {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 4px;
    margin-top: 8px;
  }
  .kbd-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 8px;
    font-size: 11.5px;
    color: var(--text-muted);
    cursor: pointer;
  }
  .kbd-btn:hover { background: var(--bg-hover); color: var(--text); }
  .kbd-keys { display: inline-flex; gap: 2px; }
  .kbd-keys kbd {
    font-size: 10px;
    border: 1px solid var(--border);
    padding: 0 4px;
    border-radius: 3px;
    background: var(--bg);
    color: var(--text-muted);
  }
</style>
