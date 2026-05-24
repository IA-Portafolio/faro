<script lang="ts">
  import { page } from '$app/stores';

  type Item = { href: string; label: string; icon: string };
  type Group = { title: string; items: Item[] };

  const groups: Group[] = [
    {
      title: 'Personal',
      items: [
        { href: '/settings/appearance', label: 'Apariencia', icon: '◐' },
        { href: '/settings/security', label: 'Seguridad', icon: '⏾' }
      ]
    },
    {
      title: 'Workspace',
      items: [
        { href: '/settings/projects', label: 'Proyectos', icon: '⚙' },
        { href: '/settings/users', label: 'Usuarios', icon: '👤' },
        { href: '/settings/alerts', label: 'Alertas', icon: '⏰' },
        { href: '/settings/integrations', label: 'Integraciones', icon: '⇆' }
      ]
    }
  ];

  $: current = $page.url.pathname;
  function isActive(href: string): boolean {
    return current === href || current.startsWith(href + '/');
  }
</script>

<div class="settings-layout">
  <aside class="settings-nav" aria-label="Sub-navegación de configuración">
    {#each groups as g}
      <div class="group-title">{g.title}</div>
      {#each g.items as it}
        <a href={it.href} class:active={isActive(it.href)} data-sveltekit-preload-data="hover">
          <span class="mono" style="width: 14px; opacity: 0.85;">{it.icon}</span>
          <span>{it.label}</span>
        </a>
      {/each}
    {/each}
  </aside>

  <section style="min-width: 0;">
    <slot />
  </section>
</div>
