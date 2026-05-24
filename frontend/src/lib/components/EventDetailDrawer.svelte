<script lang="ts">
  /**
   * Drawer lateral de detalle para un product event.
   *
   * Estructura: cabecera (event_name + timestamp + distinct/anon/session),
   * tres secciones JSON colapsables (properties / user_properties / context)
   * y shortcuts a traza y a filtros sobre la lista principal.
   *
   * El drawer es persistente: la página decide cuándo cerrarlo y le pasa el
   * evento seleccionado por prop. Comparte el `localStorage` key con el
   * LogDetailDrawer (`faro:drawer-width`) — así el ancho preferido sigue al
   * usuario entre vistas en vez de tener que ajustarlo por separado.
   */
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import { browser } from '$app/environment';
  import type { ProductEvent } from '$lib/api';
  import { formatTimestamp } from '$lib/stores';
  import { isTyping } from '$lib/keyboard';

  export let event: ProductEvent | null = null;
  /** Para mostrar "3 de 250" y los hint de j/k. */
  export let position: { index: number; total: number } | null = null;

  const dispatch = createEventDispatcher<{
    close: void;
    /** El usuario pidió aplicar un filtro a la lista principal. */
    filter: {
      key: 'event_name' | 'distinct_id' | 'session_id' | 'trace_id' | 'source' | 'prop';
      value: string;
    };
  }>();

  // ---------- Ancho compartido con LogDetailDrawer ----------

  const WIDTH_KEY = 'faro:drawer-width';
  const MIN_W = 360;
  function clampWidth(w: number): number {
    const max = browser ? Math.max(MIN_W, window.innerWidth - 80) : 1200;
    return Math.min(max, Math.max(MIN_W, w));
  }
  function loadWidth(): number {
    if (!browser) return 560;
    try {
      const raw = window.localStorage.getItem(WIDTH_KEY);
      const n = raw ? Number(raw) : NaN;
      if (Number.isFinite(n) && n >= MIN_W) return clampWidth(n);
    } catch {
      /* ignora */
    }
    return clampWidth(Math.round((browser ? window.innerWidth : 1200) * 0.45));
  }
  let width = loadWidth();
  let resizing = false;

  function onResizeStart(e: MouseEvent | PointerEvent): void {
    resizing = true;
    e.preventDefault();
    document.body.style.cursor = 'col-resize';
    document.body.style.userSelect = 'none';
  }
  function onResizeMove(e: MouseEvent): void {
    if (!resizing) return;
    width = clampWidth(window.innerWidth - e.clientX);
  }
  function onResizeEnd(): void {
    if (!resizing) return;
    resizing = false;
    document.body.style.cursor = '';
    document.body.style.userSelect = '';
    try {
      window.localStorage.setItem(WIDTH_KEY, String(width));
    } catch {
      /* ignora cuota */
    }
  }

  onMount(() => {
    if (!browser) return;
    const mm = (e: MouseEvent) => onResizeMove(e);
    const mu = () => onResizeEnd();
    window.addEventListener('mousemove', mm);
    window.addEventListener('mouseup', mu);
    return () => {
      window.removeEventListener('mousemove', mm);
      window.removeEventListener('mouseup', mu);
    };
  });

  $: if (browser) {
    const isMobile = window.innerWidth <= 700;
    document.body.style.paddingRight = event && !isMobile ? `${width}px` : '';
  }
  onDestroy(() => {
    if (browser) document.body.style.paddingRight = '';
  });

  // ---------- Atajos ----------

  let panelEl: HTMLElement | null = null;

  function onKeydown(e: KeyboardEvent): void {
    if (!event) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      dispatch('close');
      return;
    }
    if ((e.ctrlKey || e.metaKey) && (e.key === 'c' || e.key === 'C')) {
      const sel = window.getSelection?.()?.toString() ?? '';
      if (sel) return;
      if (isTyping(e.target)) return;
      if (panelEl && document.activeElement && !panelEl.contains(document.activeElement)) return;
      e.preventDefault();
      void copyAsJson();
    }
  }

  // ---------- Acciones ----------

  let toastMsg = '';
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  function flash(msg: string): void {
    toastMsg = msg;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toastMsg = ''; }, 1500);
  }
  onDestroy(() => { if (toastTimer) clearTimeout(toastTimer); });

  async function copy(text: string, label: string): Promise<void> {
    try {
      await navigator.clipboard?.writeText(text);
      flash(`✓ ${label} copiado`);
    } catch {
      window.prompt('Copia este texto:', text);
    }
  }

  async function copyAsJson(): Promise<void> {
    if (!event) return;
    // Devolvemos el evento con properties/user_properties/context parseados si
    // son JSON válido — así el clipboard tiene algo legible y no un blob de
    // string-escape.
    const parsed = {
      ...event,
      properties: tryParse(event.properties),
      user_properties: tryParse(event.user_properties),
      context: tryParse(event.context)
    };
    await copy(JSON.stringify(parsed, null, 2), 'JSON del evento');
  }

  function tryParse(raw: string): unknown {
    if (!raw) return null;
    try {
      return JSON.parse(raw);
    } catch {
      return raw;
    }
  }

  function applyFilter(
    key: 'event_name' | 'distinct_id' | 'session_id' | 'trace_id' | 'source' | 'prop',
    value: string
  ): void {
    if (!value) return;
    dispatch('filter', { key, value });
    flash('✓ Filtro aplicado');
  }

  // ---------- Render auxiliares ----------

  type Section = { title: string; raw: string; parsed: unknown; open: boolean };

  let propsOpen = true;
  let userPropsOpen = true;
  let ctxOpen = false;

  /** Cada vez que cambia el evento, abrimos properties por defecto y cerramos
   *  context — la mayoría de las veces lo importante son las propiedades del
   *  hecho. user_properties queda abierto si trae algo. */
  $: if (event) {
    propsOpen = true;
    userPropsOpen = (event.user_properties ?? '').trim().length > 0;
    ctxOpen = false;
  }

  function prettyJson(raw: string): string {
    if (!raw) return '';
    try {
      return JSON.stringify(JSON.parse(raw), null, 2);
    } catch {
      return raw;
    }
  }

  function entriesOf(raw: string): Array<{ key: string; value: string; nested: boolean }> {
    if (!raw) return [];
    let v: unknown;
    try {
      v = JSON.parse(raw);
    } catch {
      return [];
    }
    if (v === null || typeof v !== 'object' || Array.isArray(v)) return [];
    const out: Array<{ key: string; value: string; nested: boolean }> = [];
    for (const [k, val] of Object.entries(v as Record<string, unknown>)) {
      const nested = val !== null && typeof val === 'object';
      const stringified = nested ? JSON.stringify(val) : String(val);
      out.push({ key: k, value: stringified, nested });
    }
    out.sort((a, b) => a.key.localeCompare(b.key));
    return out;
  }

  $: propsEntries = event ? entriesOf(event.properties) : [];
  $: userPropsEntries = event ? entriesOf(event.user_properties) : [];
  $: ctxEntries = event ? entriesOf(event.context) : [];

  function pickCtx(raw: string, ...keys: string[]): string {
    if (!raw) return '';
    try {
      const v = JSON.parse(raw) as Record<string, unknown>;
      for (const k of keys) {
        const item = v?.[k];
        if (typeof item === 'string' && item) return item;
        if (typeof item === 'number') return String(item);
      }
    } catch {
      /* no es JSON */
    }
    return '';
  }
  $: pageUrl = event ? pickCtx(event.context, 'page_url', '$current_url', 'url') : '';
  $: userAgent = event ? pickCtx(event.context, 'user_agent', '$user_agent') : '';
</script>

<svelte:window on:keydown={onKeydown} />

{#if event}
  <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
  <aside
    bind:this={panelEl}
    class="ed"
    class:resizing
    style="width: {width}px"
    role="dialog"
    aria-label="Detalle del evento"
  >
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="ed-resize"
      role="separator"
      aria-orientation="vertical"
      aria-label="Redimensionar"
      on:mousedown={onResizeStart}
    ></div>

    <header class="ed-header">
      <div class="ed-header-top">
        <button
          class="ed-event-chip"
          on:click={() => applyFilter('event_name', event.event_name)}
          title="Filtrar por este evento"
        >
          {event.event_name}
        </button>
        <span class="ed-ts mono">{formatTimestamp(event.timestamp)}</span>
        <div class="ed-header-actions">
          {#if position}
            <span class="muted" style="font-size: 11.5px;">
              {position.index + 1} de {position.total}
              <span class="kbd-hint mono"><kbd>j</kbd><kbd>k</kbd></span>
            </span>
          {/if}
          <button class="ed-icon-btn" on:click={() => dispatch('close')} title="Cerrar (Esc)" aria-label="Cerrar">×</button>
        </div>
      </div>
      <div class="ed-header-sub mono">
        <span class="muted">distinct_id:</span>
        <button class="ed-link" on:click={() => applyFilter('distinct_id', event.distinct_id)}>{event.distinct_id || '—'}</button>
        {#if event.anonymous_id && event.anonymous_id !== event.distinct_id}
          <span class="ed-sep">·</span>
          <span class="muted">anon:</span>
          <span>{event.anonymous_id}</span>
        {/if}
        {#if event.session_id}
          <span class="ed-sep">·</span>
          <span class="muted">session:</span>
          <button class="ed-link" on:click={() => applyFilter('session_id', event.session_id)}>{event.session_id}</button>
        {/if}
        {#if event.source}
          <span class="ed-sep">·</span>
          <button class="ed-link" on:click={() => applyFilter('source', event.source)}>{event.source}</button>
        {/if}
      </div>
    </header>

    <div class="ed-body">
      <!-- Properties -->
      <section class="ed-section">
        <h3>
          <button class="ed-collapse" on:click={() => (propsOpen = !propsOpen)}>
            <span>{propsOpen ? '▾' : '▸'}</span>
            <span>Properties</span>
            <span class="muted" style="font-size: 11px; font-weight: normal;">({propsEntries.length})</span>
          </button>
        </h3>
        {#if propsOpen}
          {#if propsEntries.length > 0}
            <table class="ed-attrs">
              <tbody>
                {#each propsEntries as a (a.key)}
                  <tr>
                    <td class="ed-attr-key mono">{a.key}</td>
                    <td class="ed-attr-val mono" class:nested={a.nested}>{a.value}</td>
                    <td class="ed-attr-actions">
                      {#if !a.nested}
                        <button
                          class="ed-icon-btn small"
                          title="Filtrar properties por este key:value"
                          on:click={() => applyFilter('prop', `${a.key}:${a.value}`)}
                        >▾</button>
                      {/if}
                      <button
                        class="ed-icon-btn small"
                        title="Copiar valor"
                        on:click={() => copy(a.value, a.key)}
                      >⎘</button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else if (event.properties ?? '').trim().length > 0}
            <pre class="ed-json">{prettyJson(event.properties)}</pre>
          {:else}
            <div class="muted ed-empty">Sin properties.</div>
          {/if}
        {/if}
      </section>

      <!-- User properties -->
      <section class="ed-section">
        <h3>
          <button class="ed-collapse" on:click={() => (userPropsOpen = !userPropsOpen)}>
            <span>{userPropsOpen ? '▾' : '▸'}</span>
            <span>User properties</span>
            <span class="muted" style="font-size: 11px; font-weight: normal;">({userPropsEntries.length})</span>
          </button>
        </h3>
        {#if userPropsOpen}
          {#if userPropsEntries.length > 0}
            <table class="ed-attrs">
              <tbody>
                {#each userPropsEntries as a (a.key)}
                  <tr>
                    <td class="ed-attr-key mono">{a.key}</td>
                    <td class="ed-attr-val mono" class:nested={a.nested}>{a.value}</td>
                    <td class="ed-attr-actions">
                      <button
                        class="ed-icon-btn small"
                        title="Copiar valor"
                        on:click={() => copy(a.value, a.key)}
                      >⎘</button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else}
            <div class="muted ed-empty">Sin user properties para este evento.</div>
          {/if}
        {/if}
      </section>

      <!-- Context -->
      <section class="ed-section">
        <h3>
          <button class="ed-collapse" on:click={() => (ctxOpen = !ctxOpen)}>
            <span>{ctxOpen ? '▾' : '▸'}</span>
            <span>Context</span>
            <span class="muted" style="font-size: 11px; font-weight: normal;">({ctxEntries.length})</span>
          </button>
        </h3>
        {#if ctxOpen}
          {#if ctxEntries.length > 0}
            <table class="ed-attrs">
              <tbody>
                {#each ctxEntries as a (a.key)}
                  <tr>
                    <td class="ed-attr-key mono">{a.key}</td>
                    <td class="ed-attr-val mono" class:nested={a.nested}>{a.value}</td>
                    <td class="ed-attr-actions">
                      <button
                        class="ed-icon-btn small"
                        title="Copiar valor"
                        on:click={() => copy(a.value, a.key)}
                      >⎘</button>
                    </td>
                  </tr>
                {/each}
              </tbody>
            </table>
          {:else}
            <div class="muted ed-empty">Sin contexto.</div>
          {/if}
        {/if}
      </section>

      <!-- Navegación contextual -->
      <section class="ed-section">
        <h3>Contexto</h3>
        <div class="ed-shortcuts">
          {#if event.trace_id}
            <a href={`/traces/${event.trace_id}`} class="ed-shortcut">
              <span class="ed-shortcut-icon">⤳</span>
              <span class="ed-shortcut-label">Ver traza del backend</span>
              <span class="ed-shortcut-sub mono">{event.trace_id.slice(0, 16)}…</span>
            </a>
          {:else}
            <div class="ed-shortcut disabled" title="Este evento no trae trace_id">
              <span class="ed-shortcut-icon">⤳</span>
              <span class="ed-shortcut-label">Sin traza asociada</span>
              <span class="ed-shortcut-sub muted">100% client-side</span>
            </div>
          {/if}
          {#if event.distinct_id}
            <button class="ed-shortcut" on:click={() => applyFilter('distinct_id', event.distinct_id)}>
              <span class="ed-shortcut-icon">👤</span>
              <span class="ed-shortcut-label">Más eventos de este usuario</span>
            </button>
          {/if}
          {#if pageUrl}
            <a href={pageUrl} target="_blank" rel="noreferrer noopener" class="ed-shortcut">
              <span class="ed-shortcut-icon">🌐</span>
              <span class="ed-shortcut-label">Abrir page_url</span>
              <span class="ed-shortcut-sub mono">{pageUrl}</span>
            </a>
          {/if}
          {#if userAgent}
            <div class="ed-shortcut disabled" title={userAgent}>
              <span class="ed-shortcut-icon">🧭</span>
              <span class="ed-shortcut-label">User agent</span>
              <span class="ed-shortcut-sub mono">{userAgent.slice(0, 48)}{userAgent.length > 48 ? '…' : ''}</span>
            </div>
          {/if}
        </div>
      </section>
    </div>

    <footer class="ed-footer">
      <button on:click={copyAsJson} title="Copiar el evento como JSON (Cmd+C)">
        <span class="mono">⎘</span> Copiar JSON
      </button>
      {#if event.event_id}
        <button on:click={() => copy(event.event_id, 'event_id')}>
          <span class="mono">⎘</span> event_id
        </button>
      {/if}
    </footer>

    {#if toastMsg}
      <div class="ed-toast">{toastMsg}</div>
    {/if}
  </aside>
{/if}

<style>
  .ed {
    position: fixed;
    top: 0;
    right: 0;
    height: 100vh;
    background: var(--bg-elev);
    border-left: 1px solid var(--border);
    box-shadow: var(--shadow-drawer);
    z-index: 100;
    display: flex;
    flex-direction: column;
    min-width: 360px;
    max-width: 100vw;
  }
  .ed.resizing { transition: none; user-select: none; }

  .ed-resize {
    position: absolute;
    top: 0;
    left: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
    background: transparent;
    z-index: 2;
  }
  .ed-resize:hover { background: var(--accent); opacity: 0.35; }

  .ed-header {
    padding: 12px 16px 8px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .ed-header-top {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ed-event-chip {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 3px 10px;
    font-family: "JetBrains Mono", Menlo, monospace;
    font-size: 12px;
    cursor: pointer;
    color: var(--text);
    line-height: 1.4;
  }
  .ed-event-chip:hover {
    border-color: var(--accent);
    color: var(--accent);
  }
  .ed-ts {
    font-size: 12px;
    color: var(--text-muted);
  }
  .ed-header-actions {
    margin-left: auto;
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .kbd-hint { margin-left: 4px; }
  .kbd-hint kbd {
    border: 1px solid var(--border);
    padding: 0 4px;
    border-radius: 3px;
    font-size: 10px;
    margin-right: 2px;
    background: var(--bg);
  }
  .ed-header-sub {
    margin-top: 6px;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-muted);
    flex-wrap: wrap;
  }
  .ed-sep { color: var(--text-muted); }
  .ed-link {
    background: transparent;
    border: 0;
    padding: 0;
    color: var(--text);
    font-family: inherit;
    font-size: inherit;
    cursor: pointer;
    border-bottom: 1px dashed var(--border);
  }
  .ed-link:hover { color: var(--accent); border-bottom-color: var(--accent); }

  .ed-body {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
  }
  .ed-section { margin-bottom: 18px; }
  .ed-section h3 {
    margin: 0 0 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }
  .ed-collapse {
    background: transparent;
    border: 0;
    padding: 0;
    color: inherit;
    font: inherit;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    text-transform: inherit;
    letter-spacing: inherit;
  }

  .ed-attrs {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
    table-layout: fixed;
  }
  .ed-attrs td {
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    vertical-align: top;
    background: transparent;
  }
  .ed-attrs tr:last-child td { border-bottom: 0; }
  .ed-attrs tr:hover td { background: var(--bg-hover); }
  .ed-attr-key {
    width: 38%;
    color: var(--text-muted);
    word-break: break-all;
    padding-left: 8px;
  }
  .ed-attr-val {
    word-break: break-all;
    white-space: pre-wrap;
  }
  .ed-attr-val.nested {
    color: var(--text-muted);
    font-style: italic;
  }
  .ed-attr-actions {
    width: 60px;
    text-align: right;
    white-space: nowrap;
  }

  .ed-json {
    margin: 0;
    padding: 10px 12px;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 320px;
    overflow: auto;
  }
  .ed-empty {
    padding: 8px 0;
    font-size: 12px;
  }

  .ed-icon-btn {
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 2px 8px;
    cursor: pointer;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.2;
  }
  .ed-icon-btn:hover { background: var(--bg-hover); color: var(--text); border-color: var(--border); }
  .ed-icon-btn.small { padding: 0 5px; font-size: 11.5px; }

  .ed-shortcuts {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .ed-shortcut {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font: inherit;
    color: var(--text);
    cursor: pointer;
    text-decoration: none;
    text-align: left;
  }
  .ed-shortcut:hover { background: var(--bg-hover); text-decoration: none; }
  .ed-shortcut.disabled { cursor: default; opacity: 0.7; }
  .ed-shortcut.disabled:hover { background: var(--bg); }
  .ed-shortcut-icon {
    width: 18px;
    text-align: center;
    flex-shrink: 0;
  }
  .ed-shortcut-label { flex: 1; font-size: 13px; }
  .ed-shortcut-sub {
    color: var(--text-muted);
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 240px;
  }

  .ed-footer {
    display: flex;
    gap: 6px;
    padding: 8px 12px;
    border-top: 1px solid var(--border);
    background: var(--bg);
    flex-shrink: 0;
  }
  .ed-footer button {
    font-size: 12px;
    padding: 4px 10px;
  }

  .ed-toast {
    position: absolute;
    bottom: 56px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 5px 12px;
    font-size: 12px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    pointer-events: none;
  }

  @media (max-width: 700px) {
    .ed {
      width: 100vw !important;
    }
    .ed-resize { display: none; }
  }
</style>
