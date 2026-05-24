<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import { goto } from '$app/navigation';
  import { selectedProject, currentUser } from '$lib/stores';
  import { fetchProjects, logout, type Project } from '$lib/api';

  const links = [
    { href: '/', label: 'Resumen', icon: '◐' },
    { href: '/logs', label: 'Logs', icon: '≡' },
    { href: '/traces', label: 'Trazas', icon: '⤳' },
    { href: '/metrics', label: 'Métricas', icon: '◢' },
    { href: '/errors', label: 'Errores', icon: '⚠' },
    { href: '/monitors', label: 'Monitores', icon: '◉' },
    { href: '/alerts', label: 'Alertas', icon: '⏰' },
    { href: '/projects', label: 'Proyectos', icon: '⚙' },
    { href: '/users', label: 'Usuarios', icon: '👤' },
    { href: '/settings/integrations', label: 'Integraciones', icon: '⇆' }
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
        <a href="/projects" style="color: var(--accent);">Crear el primer proyecto →</a>
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
    {#if $currentUser}
      <div style="font-size: 12px;">
        <div style="font-weight: 600;">{$currentUser.name || $currentUser.email}</div>
        <div class="muted" style="font-size: 11px;">{$currentUser.email}</div>
      </div>
      <button on:click={doLogout} style="margin-top: 8px; width: 100%; font-size: 12px;">Salir</button>
    {/if}
    <div class="muted" style="font-size: 10px; margin-top: 10px;">faro v0.3.0</div>
  </div>
</aside>
