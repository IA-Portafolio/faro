<script lang="ts">
  import { tick } from 'svelte';
  import { paletteOpen } from '$lib/keyboard';
  import { selectedProject } from '$lib/stores';
  import {
    invalidatePaletteCache,
    jumpCommands,
    loadEntityCommands,
    nextHighlight,
    search,
    staticCommands,
    type Command,
    type CommandGroup
  } from '$lib/palette';

  let query = '';
  let highlighted = 0;
  let inputEl: HTMLInputElement | null = null;
  let listEl: HTMLDivElement | null = null;
  let entityCommands: Command[] = [];
  let loadingEntities = false;
  let entityError = '';

  const base: Command[] = staticCommands();

  // Orden visual de los grupos. Saltos directos van primero porque cuando
  // existen suelen ser exactamente lo que el usuario quería.
  const groupOrder: CommandGroup[] = [
    'Salto directo',
    'Navegar',
    'Proyectos',
    'Servicios',
    'Monitores',
    'Alertas',
    'Tema'
  ];

  $: jumps = jumpCommands(query);
  $: allCommands = [...jumps, ...base, ...entityCommands];
  $: filtered = search(allCommands, query);
  // Tope de filas renderizadas. Si el usuario no filtra y hay 1000 servicios,
  // no tiene sentido pintarlos todos — confiar en que busque.
  $: visible = filtered.slice(0, 80);
  $: groups = (() => {
    const map = new Map<CommandGroup, Command[]>();
    for (const c of visible) {
      const arr = map.get(c.group) ?? [];
      arr.push(c);
      map.set(c.group, arr);
    }
    return groupOrder
      .filter((g) => map.has(g))
      .map((g) => [g, map.get(g) as Command[]] as const);
  })();

  $: if (highlighted >= visible.length) highlighted = Math.max(0, visible.length - 1);

  async function open(): Promise<void> {
    query = '';
    highlighted = 0;
    entityError = '';
    await tick();
    inputEl?.focus();
    if (entityCommands.length === 0) await refreshEntities();
  }

  async function refreshEntities(force = false): Promise<void> {
    loadingEntities = true;
    try {
      entityCommands = await loadEntityCommands(force);
    } catch (e) {
      entityError = e instanceof Error ? e.message : String(e);
    } finally {
      loadingEntities = false;
    }
  }

  $: if ($paletteOpen) void open();

  // Si el usuario cambia de proyecto mientras la paleta está abierta, el
  // listado de servicios queda obsoleto. Invalida + recarga.
  let lastProject = $selectedProject;
  $: if ($selectedProject !== lastProject) {
    lastProject = $selectedProject;
    invalidatePaletteCache();
    if ($paletteOpen) void refreshEntities(true);
  }

  function close(): void {
    paletteOpen.set(false);
  }

  async function run(c: Command): Promise<void> {
    close();
    try {
      await c.run();
    } catch (e) {
      console.error('comando falló:', e);
    }
  }

  async function ensureHighlightedVisible(): Promise<void> {
    if (highlighted < 0 || !listEl) return;
    await tick();
    const el = listEl.querySelectorAll<HTMLElement>('.palette-item')[highlighted];
    el?.scrollIntoView({ block: 'nearest' });
  }

  function onKey(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault();
      close();
      return;
    }
    if (e.key === 'Enter') {
      e.preventDefault();
      const c = visible[highlighted];
      if (c) void run(c);
      return;
    }
    const inField = document.activeElement === inputEl;
    if (e.key === 'ArrowDown' || (e.ctrlKey && e.key === 'n') || (!e.ctrlKey && !e.metaKey && !inField && e.key === 'j')) {
      e.preventDefault();
      highlighted = nextHighlight(highlighted, visible.length, 1);
      void ensureHighlightedVisible();
      return;
    }
    if (e.key === 'ArrowUp' || (e.ctrlKey && e.key === 'p') || (!e.ctrlKey && !e.metaKey && !inField && e.key === 'k')) {
      e.preventDefault();
      highlighted = nextHighlight(highlighted, visible.length, -1);
      void ensureHighlightedVisible();
      return;
    }
  }

  // Cuando el usuario cambia la query, vuelve al primer ítem.
  $: query, (highlighted = 0);
</script>

{#if $paletteOpen}
  <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
  <div class="palette-backdrop" on:click={close} on:keydown={onKey} role="presentation">
    <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
    <div
      class="palette"
      role="dialog"
      aria-label="Paleta de comandos"
      on:click|stopPropagation
      on:keydown={onKey}
    >
      <div class="palette-input">
        <input
          bind:this={inputEl}
          bind:value={query}
          placeholder="Buscar comando, proyecto, servicio… o traces:&lt;id&gt;, errors:&lt;fp&gt;"
          spellcheck="false"
          autocomplete="off"
          autocapitalize="off"
          autocorrect="off"
        />
        {#if loadingEntities}
          <span class="palette-loading" title="Indexando entidades…"><span class="spinner"></span></span>
        {:else}
          <button
            type="button"
            class="palette-refresh"
            on:click={() => refreshEntities(true)}
            title="Refrescar índice"
            aria-label="Refrescar índice"
          >↻</button>
        {/if}
      </div>

      {#if entityError}
        <div class="palette-error">No se pudo cargar el índice: {entityError}</div>
      {/if}

      <div class="palette-list" role="listbox" bind:this={listEl}>
        {#each groups as [groupName, items]}
          <div class="palette-group">{groupName}</div>
          {#each items as c (c.id)}
            {@const idx = visible.indexOf(c)}
            <!-- svelte-ignore a11y-click-events-have-key-events -->
            <div
              role="option"
              tabindex="-1"
              aria-selected={idx === highlighted}
              class="palette-item"
              class:active={idx === highlighted}
              on:click={() => run(c)}
              on:mouseenter={() => (highlighted = idx)}
            >
              {#if c.icon}
                <span class="palette-icon mono" aria-hidden="true">{c.icon}</span>
              {/if}
              <span class="palette-label">
                <span class="palette-title">{c.label}</span>
                {#if c.sub}
                  <span class="palette-sub mono">{c.sub}</span>
                {/if}
              </span>
              {#if c.hint}
                <span class="palette-hint">{c.hint}</span>
              {/if}
              {#if c.shortcut}
                <span class="palette-shortcut mono">{c.shortcut}</span>
              {/if}
            </div>
          {/each}
        {/each}

        {#if filtered.length === 0 && !loadingEntities}
          <div class="palette-empty">
            Sin coincidencias.
            {#if query}
              <div class="muted" style="margin-top: 6px; font-size: 11.5px;">
                Prueba con <code>traces:&lt;id&gt;</code>, <code>logs:trace=&lt;id&gt;</code> o <code>errors:&lt;fp&gt;</code>.
              </div>
            {/if}
          </div>
        {/if}

        {#if filtered.length > visible.length}
          <div class="palette-overflow muted">
            +{filtered.length - visible.length} resultados más. Refina la búsqueda.
          </div>
        {/if}
      </div>

      <div class="palette-footer mono">
        <span><kbd>↑</kbd><kbd>↓</kbd> moverse</span>
        <span><kbd>↵</kbd> ejecutar</span>
        <span><kbd>Esc</kbd> cerrar</span>
        <span class="palette-count">{filtered.length} resultado{filtered.length === 1 ? '' : 's'}</span>
      </div>
    </div>
  </div>
{/if}

<style>
  .palette-backdrop {
    position: fixed;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
    z-index: 300;
    backdrop-filter: blur(2px);
  }
  .palette {
    width: min(680px, 92vw);
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 16px 64px rgba(0, 0, 0, 0.45);
    display: flex;
    flex-direction: column;
    max-height: 76vh;
    overflow: hidden;
  }
  .palette-input {
    display: flex;
    align-items: center;
    border-bottom: 1px solid var(--border);
  }
  .palette-input input {
    flex: 1;
    border: 0;
    background: transparent;
    padding: 14px 18px;
    font-size: 15px;
    border-radius: 0;
    outline: none;
  }
  .palette-input input:focus { outline: none; box-shadow: none; }
  .palette-loading {
    padding-right: 14px;
    display: inline-flex;
    align-items: center;
  }
  .palette-refresh {
    background: transparent;
    border: 0;
    color: var(--text-muted);
    padding: 0 14px;
    cursor: pointer;
    font-size: 16px;
    line-height: 1;
  }
  .palette-refresh:hover { color: var(--text); }
  .palette-error {
    padding: 8px 18px;
    color: var(--danger);
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }
  .palette-list {
    overflow-y: auto;
    padding: 6px 0;
  }
  .palette-group {
    padding: 10px 18px 4px;
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }
  .palette-item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 18px;
    cursor: pointer;
    font-size: 13.5px;
    line-height: 1.3;
  }
  .palette-item.active {
    background: var(--bg-hover);
  }
  .palette-icon {
    width: 18px;
    text-align: center;
    color: var(--text-muted);
    flex-shrink: 0;
  }
  .palette-item.active .palette-icon { color: var(--accent); }
  .palette-label {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .palette-title {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .palette-item.active .palette-title { color: var(--accent); }
  .palette-sub {
    font-size: 11.5px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .palette-hint {
    font-size: 10.5px;
    color: var(--text-muted);
    border: 1px solid var(--border);
    background: var(--bg);
    padding: 1px 6px;
    border-radius: 10px;
    text-transform: uppercase;
    letter-spacing: 0.4px;
    flex-shrink: 0;
  }
  .palette-shortcut {
    color: var(--text-muted);
    font-size: 11px;
    border: 1px solid var(--border);
    padding: 1px 6px;
    border-radius: 4px;
    flex-shrink: 0;
  }
  .palette-empty {
    padding: 28px 18px;
    text-align: center;
    color: var(--text-muted);
    font-size: 13px;
  }
  .palette-empty code {
    background: var(--bg);
    border: 1px solid var(--border);
    padding: 0 4px;
    border-radius: 3px;
  }
  .palette-overflow {
    padding: 8px 18px;
    font-size: 11.5px;
    text-align: center;
    border-top: 1px dashed var(--border);
  }
  .palette-footer {
    display: flex;
    gap: 16px;
    padding: 8px 18px;
    border-top: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 11.5px;
    background: var(--bg);
    align-items: center;
  }
  .palette-footer .palette-count {
    margin-left: auto;
  }
  .palette-footer kbd {
    border: 1px solid var(--border);
    padding: 0 5px;
    border-radius: 3px;
    margin-right: 2px;
    font-size: 10.5px;
    background: var(--bg-elev);
  }
</style>
