<script lang="ts">
  import { page } from '$app/stores';

  type Item = { href: string; label: string; group: 'Personal' | 'Workspace' };

  // Tabs horizontales agrupados visualmente con un separador entre Personal y
  // Workspace. El grupo se mantiene como información secundaria — los labels
  // de los grupos no se muestran como headers (eso volvería a vender el
  // sub-sidebar viejo), pero el agrupamiento mantiene el orden lógico.
  const items: Item[] = [
    { href: '/settings/appearance', label: 'Apariencia', group: 'Personal' },
    { href: '/settings/security', label: 'Seguridad', group: 'Personal' },
    { href: '/settings/projects', label: 'Proyectos', group: 'Workspace' },
    { href: '/settings/users', label: 'Usuarios', group: 'Workspace' },
    { href: '/settings/alerts', label: 'Alertas', group: 'Workspace' },
    { href: '/settings/integrations', label: 'Integraciones', group: 'Workspace' }
  ];

  $: current = $page.url.pathname;
  function isActive(href: string): boolean {
    return current === href || current.startsWith(href + '/');
  }
</script>

<nav class="settings-tabs" aria-label="Sub-navegación de configuración">
  {#each items as it, i (it.href)}
    {#if i > 0 && it.group !== items[i - 1].group}
      <span class="settings-tabs-divider" aria-hidden="true"></span>
    {/if}
    <a href={it.href} class:active={isActive(it.href)} data-sveltekit-preload-data="hover">
      {it.label}
    </a>
  {/each}
</nav>

<section class="settings-content">
  <slot />
</section>
