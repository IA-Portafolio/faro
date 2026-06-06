<script lang="ts">
  /**
   * Barra de navegación lateral: menú del producto, selector de proyecto, usuario
   * y toggle de tema.
   *
   * Define el menú agrupado por secciones (Observabilidad, Producto, …) con iconos
   * SVG inline estilo Lucide, resalta la ruta activa según `$page`, permite fijar
   * el proyecto global (`selectedProject`) y abrir la paleta/ayuda. `ICONS` mapea
   * nombre de icono → contenido del `<svg>`.
   */
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { selectedProject, currentUser } from '$lib/stores';
  import { fetchProjects, logout, type Project } from '$lib/api';
  import { themeChoice, setTheme, type ThemeChoice } from '$lib/theme';
  import { paletteOpen, helpOpen } from '$lib/keyboard';

  // Iconos SVG estilo Lucide (stroke=currentColor, 24×24 viewBox). Cada entrada es
  // el contenido interno del <svg> — el wrapper común lo pone el render de abajo.
  const ICONS: Record<string, string> = {
    dashboard:
      '<rect width="7" height="9" x="3" y="3" rx="1"/><rect width="7" height="5" x="14" y="3" rx="1"/><rect width="7" height="9" x="14" y="12" rx="1"/><rect width="7" height="5" x="3" y="16" rx="1"/>',
    list: '<path d="M3 6h18"/><path d="M3 12h18"/><path d="M3 18h18"/>',
    route:
      '<circle cx="6" cy="19" r="3"/><path d="M9 19h8.5a3.5 3.5 0 0 0 0-7h-11a3.5 3.5 0 0 1 0-7H15"/><circle cx="18" cy="5" r="3"/>',
    network:
      '<rect x="16" y="16" width="6" height="6" rx="1"/><rect x="2" y="16" width="6" height="6" rx="1"/><rect x="9" y="2" width="6" height="6" rx="1"/><path d="M5 16v-3a1 1 0 0 1 1-1h12a1 1 0 0 1 1 1v3"/><path d="M12 12V8"/>',
    chart:
      '<path d="M3 3v18h18"/><path d="M7 16V8"/><path d="M11 16v-5"/><path d="M15 16v-9"/><path d="M19 16v-3"/>',
    alert:
      '<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3Z"/><path d="M12 9v4"/><path d="M12 17h.01"/>',
    sparkles:
      '<path d="M9.937 15.5A2 2 0 0 0 8.5 14.063l-6.135-1.582a.5.5 0 0 1 0-.962L8.5 9.936A2 2 0 0 0 9.937 8.5l1.582-6.135a.5.5 0 0 1 .963 0L14.063 8.5A2 2 0 0 0 15.5 9.937l6.135 1.581a.5.5 0 0 1 0 .964L15.5 14.063a2 2 0 0 0-1.437 1.437l-1.582 6.135a.5.5 0 0 1-.963 0z"/><path d="M20 3v4"/><path d="M22 5h-4"/><path d="M4 17v2"/><path d="M5 18H3"/>',
    zap: '<path d="M13 2 3 14h9l-1 8 10-12h-9l1-8z"/>',
    users:
      '<path d="M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2"/><circle cx="9" cy="7" r="4"/><path d="M22 21v-2a4 4 0 0 0-3-3.87"/><path d="M16 3.13a4 4 0 0 1 0 7.75"/>',
    clock: '<circle cx="12" cy="12" r="10"/><polyline points="12 6 12 12 16 14"/>',
    filter: '<polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3"/>',
    calendar:
      '<rect width="18" height="18" x="3" y="4" rx="2"/><path d="M16 2v4"/><path d="M3 10h18"/><path d="M8 2v4"/><path d="m9 16 2 2 4-4"/>',
    layers:
      '<path d="m12.83 2.18a2 2 0 0 0-1.66 0L2.6 6.08a1 1 0 0 0 0 1.83l8.58 3.91a2 2 0 0 0 1.66 0l8.58-3.9a1 1 0 0 0 0-1.83Z"/><path d="m22 17.65-9.17 4.16a2 2 0 0 1-1.66 0L2 17.65"/><path d="m22 12.65-9.17 4.16a2 2 0 0 1-1.66 0L2 12.65"/>',
    flask:
      '<path d="M10 2v7.527a2 2 0 0 1-.211.896L4.72 20.55a1 1 0 0 0 .9 1.45h12.76a1 1 0 0 0 .9-1.45l-5.069-10.127A2 2 0 0 1 14 9.527V2"/><path d="M8.5 2h7"/><path d="M7 16h10"/>',
    radar:
      '<path d="M19.07 4.93A10 10 0 0 0 6.99 3.34"/><path d="M4 6h.01"/><path d="M2.29 9.62A10 10 0 1 0 21.31 8.35"/><path d="M16.24 7.76A6 6 0 1 0 8.23 16.67"/><path d="M12 18h.01"/><path d="M17.99 11.66A6 6 0 0 1 15.77 16.67"/><circle cx="12" cy="12" r="2"/><path d="m13.41 10.59 5.66-5.66"/>',
    settings:
      '<path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z"/><circle cx="12" cy="12" r="3"/>',
    code:
      '<path d="m18 16 4-4-4-4"/><path d="m6 8-4 4 4 4"/><path d="m14.5 4-5 16"/>'
  };

  type NavItem = { href: string; label: string; icon: keyof typeof ICONS };
  type NavSection = { title: string; items: NavItem[] };

  const sections: NavSection[] = [
    {
      title: 'Observabilidad',
      items: [
        { href: '/', label: 'Resumen', icon: 'dashboard' },
        { href: '/logs', label: 'Logs', icon: 'list' },
        { href: '/traces', label: 'Trazas', icon: 'route' },
        { href: '/service-map', label: 'Service map', icon: 'network' },
        { href: '/metrics', label: 'Métricas', icon: 'chart' },
        { href: '/errors', label: 'Errores', icon: 'alert' },
        { href: '/insights', label: 'Insights', icon: 'sparkles' }
      ]
    },
    {
      title: 'Producto',
      items: [
        { href: '/events', label: 'Eventos', icon: 'zap' },
        { href: '/users', label: 'Usuarios', icon: 'users' },
        { href: '/sessions', label: 'Sesiones', icon: 'clock' },
        { href: '/funnels', label: 'Funnels', icon: 'filter' },
        { href: '/retention', label: 'Retention', icon: 'calendar' },
        { href: '/cohorts', label: 'Cohorts', icon: 'layers' },
        { href: '/experiments', label: 'Experimentos', icon: 'flask' }
      ]
    },
    {
      title: 'Operación',
      items: [
        { href: '/monitors', label: 'Monitores', icon: 'radar' },
        { href: '/docs', label: 'SDKs & API', icon: 'code' },
        { href: '/settings', label: 'Configuración', icon: 'settings' }
      ]
    }
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
    {#each sections as section}
      <div class="nav-section-title">{section.title}</div>
      {#each section.items as l}
        <a href={l.href} class:active={isActive(l.href)} data-sveltekit-preload-data="hover">
          <svg
            class="nav-icon"
            viewBox="0 0 24 24"
            width="16"
            height="16"
            fill="none"
            stroke="currentColor"
            stroke-width="1.75"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
          >
            {@html ICONS[l.icon]}
          </svg>
          <span>{l.label}</span>
        </a>
      {/each}
    {/each}
  </nav>

  <div class="sidebar-footer">
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
    gap: 6px;
    margin-top: 8px;
  }
  .kbd-btn {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 5px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 5px 7px;
    font-size: 11px;
    color: var(--text-muted);
    cursor: pointer;
    /* Permite shrink debajo del intrinsic min-width para que la grid 1fr 1fr
       no desborde el sidebar y dispare scroll horizontal. */
    min-width: 0;
    overflow: hidden;
  }
  .kbd-btn > span:first-child {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  .kbd-btn:hover { background: var(--bg-hover); color: var(--text); }
  .kbd-keys { display: inline-flex; gap: 1px; flex-shrink: 0; }
  .kbd-keys kbd {
    font-size: 9px;
    border: 1px solid var(--border);
    padding: 0 3px;
    border-radius: 3px;
    background: var(--bg);
    color: var(--text-muted);
  }
</style>
