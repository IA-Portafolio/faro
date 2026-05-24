<script lang="ts">
  /**
   * Drawer lateral de detalle para un log.
   *
   * Decisiones de diseño:
   *   - **Persistente**: queda abierto entre clicks; cambiar de log solo
   *     actualiza su contenido. La página lo controla con la prop `log`.
   *   - **Resizable**: drag horizontal en el borde izquierdo. Ancho
   *     persistido en localStorage (`faro:drawer-width`) para que cada user
   *     recupere su tamaño preferido entre sesiones.
   *   - **Keyboard**: la página delega j/k (mover selección, actualiza este
   *     drawer); el drawer solo reclama `Esc` (cerrar) y `Cmd/Ctrl+C` cuando
   *     el foco está dentro de él (copia el JSON del log).
   *   - **Logs ±2min**: sección inline que carga la ventana de contexto al
   *     vuelo usando el mismo `/api/v1/logs` con `from`/`to` derivados del
   *     timestamp del log mostrado.
   *
   * Emite eventos para que la página decida qué hacer cuando el usuario
   * pide aplicar un filtro (`filter`), cerrar (`close`) o saltar a otro
   * log dentro del contexto (`jump`).
   */
  import { createEventDispatcher, onDestroy, onMount, tick } from 'svelte';
  import { browser } from '$app/environment';
  import { fetchLogs, type LogRow } from '$lib/api';
  import { formatTimestamp, selectedProject } from '$lib/stores';
  import { isTyping } from '$lib/keyboard';
  import SeverityBadge from './SeverityBadge.svelte';

  export let log: LogRow | null = null;
  /** Para mostrar la posición ("3 de 250") y permitir navegación con flechas/Esc. */
  export let position: { index: number; total: number } | null = null;

  const dispatch = createEventDispatcher<{
    close: void;
    /** El usuario pidió aplicar un filtro nuevo (clave/valor) a la lista principal. */
    filter: { key: 'service' | 'query' | 'trace_id'; value: string };
    /** El usuario pidió saltar al log clicado dentro del bloque de contexto. */
    jump: { timestamp: string };
  }>();

  // ---------- Ancho resizable ----------

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
    // Default razonable: 45vw, con 560 como min sensato.
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
    const next = clampWidth(window.innerWidth - e.clientX);
    width = next;
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

  /**
   * Aplica un `padding-right` al body para que el contenido de la página se
   * encoja en vez de quedar oculto bajo el drawer. Esto es lo que mantiene
   * el contexto del listado visible mientras se inspecciona un log.
   * En mobile el drawer es fullscreen, así que ahí no aplicamos nada.
   */
  $: if (browser) {
    const isMobile = window.innerWidth <= 700;
    document.body.style.paddingRight = log && !isMobile ? `${width}px` : '';
  }
  onDestroy(() => {
    if (browser) document.body.style.paddingRight = '';
  });

  // ---------- Atajos de teclado dentro del drawer ----------

  let panelEl: HTMLElement | null = null;

  function onKeydown(e: KeyboardEvent): void {
    if (!log) return;

    // Esc cierra. Esc puede llegarnos también si el foco está en un input
    // del drawer — es lo correcto: cerrar.
    if (e.key === 'Escape') {
      e.preventDefault();
      dispatch('close');
      return;
    }

    // Cmd/Ctrl+C: si el usuario tiene una selección de texto activa, le dejamos
    // el comportamiento nativo (copiar lo seleccionado). Si no, copiamos el log
    // entero como JSON.
    if ((e.ctrlKey || e.metaKey) && (e.key === 'c' || e.key === 'C')) {
      const sel = window.getSelection?.()?.toString() ?? '';
      if (sel) return;
      if (isTyping(e.target)) return;
      // Solo si el foco está dentro del drawer — si no, la página de logs lo gestiona.
      if (panelEl && document.activeElement && !panelEl.contains(document.activeElement)) return;
      e.preventDefault();
      void copyAsJson();
    }
  }

  // ---------- Logs cercanos ±2min ----------

  type ContextState =
    | { kind: 'idle' }
    | { kind: 'loading' }
    | { kind: 'ok'; rows: LogRow[] }
    | { kind: 'err'; message: string };

  let contextOpen = false;
  let context: ContextState = { kind: 'idle' };
  let contextSameService = true;
  /** Cache para no recargar al re-expandir el panel si el log no cambió. */
  let contextKey = '';

  function buildContextKey(l: LogRow, sameSvc: boolean): string {
    return `${l.timestamp}|${sameSvc ? l.service_name : ''}|${$selectedProject}`;
  }

  async function loadContext(): Promise<void> {
    if (!log) return;
    const key = buildContextKey(log, contextSameService);
    if (contextKey === key && context.kind === 'ok') return;
    contextKey = key;
    context = { kind: 'loading' };
    try {
      // ±2 minutos alrededor del log.
      const ts = parseTs(log.timestamp);
      if (ts === null) {
        context = { kind: 'err', message: 'timestamp inválido' };
        return;
      }
      const fromIso = new Date(ts - 120_000).toISOString();
      const toIso = new Date(ts + 120_000).toISOString();
      const rows = await fetchLogs({
        from: fromIso,
        to: toIso,
        project: $selectedProject || undefined,
        service: contextSameService ? log.service_name : undefined,
        limit: 200
      });
      // Asegura orden ascendente por timestamp para que la ventana se lea
      // naturalmente de pasado a futuro. El endpoint puede devolver desc.
      rows.sort((a, b) => (a.timestamp < b.timestamp ? -1 : 1));
      context = { kind: 'ok', rows };
    } catch (e: unknown) {
      context = { kind: 'err', message: e instanceof Error ? e.message : String(e) };
    }
  }

  function parseTs(s: string): number | null {
    if (!s) return null;
    const iso = s.includes('T') ? s : s.replace(' ', 'T') + 'Z';
    const t = Date.parse(iso);
    return Number.isFinite(t) ? t : null;
  }

  function isFocusedRow(r: LogRow): boolean {
    return !!log && r.timestamp === log.timestamp && r.body === log.body;
  }

  // Cuando cambia el log seleccionado, invalida el bloque de contexto pero
  // mantiene si estaba abierto o cerrado.
  $: if (log) {
    if (contextOpen) {
      void loadContext();
    } else {
      contextKey = '';
      context = { kind: 'idle' };
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
  onDestroy(() => {
    if (toastTimer) clearTimeout(toastTimer);
  });

  async function copy(text: string, label: string): Promise<void> {
    try {
      await navigator.clipboard?.writeText(text);
      flash(`✓ ${label} copiado`);
    } catch {
      // Fallback: prompt.
      window.prompt('Copia este texto:', text);
    }
  }

  async function copyAsJson(): Promise<void> {
    if (!log) return;
    await copy(JSON.stringify(log, null, 2), 'JSON del log');
  }

  async function copyBody(): Promise<void> {
    if (!log) return;
    await copy(log.body ?? '', 'mensaje');
  }

  async function shareLink(): Promise<void> {
    if (!log) return;
    const url = new URL(window.location.href);
    url.searchParams.set('selected', log.timestamp);
    try {
      await navigator.clipboard?.writeText(url.toString());
      flash('✓ Enlace copiado');
      window.history.replaceState(null, '', url.toString());
    } catch {
      window.prompt('Copia este enlace:', url.toString());
    }
  }

  function applyFilter(key: 'service' | 'query' | 'trace_id', value: string): void {
    if (!value) return;
    dispatch('filter', { key, value });
    flash('✓ Filtro aplicado');
  }

  // ---------- Vista de atributos ----------

  type AttrEntry = { key: string; value: string; source: 'attr' | 'resource' };

  $: attrs = (() => {
    const out: AttrEntry[] = [];
    if (!log) return out;
    for (const [k, v] of Object.entries(log.attributes ?? {})) {
      out.push({ key: k, value: v, source: 'attr' });
    }
    for (const [k, v] of Object.entries(log.resource_attributes ?? {})) {
      out.push({ key: k, value: v, source: 'resource' });
    }
    out.sort((a, b) => a.key.localeCompare(b.key));
    return out;
  })();

  // Lee resource attribute con nombres comunes (host, k8s pod, environment) para el subtítulo.
  function pick(log: LogRow, ...keys: string[]): string {
    for (const k of keys) {
      const v = log.resource_attributes?.[k] ?? log.attributes?.[k];
      if (v) return v;
    }
    return '';
  }
  $: env  = log ? pick(log, 'deployment.environment', 'env', 'environment') : '';
  $: host = log ? pick(log, 'host.name', 'host', 'k8s.pod.name', 'k8s.node.name') : '';

  /**
   * Intenta interpretar `body` como JSON. Si es un objeto/array, lo serializa
   * indentado para mostrarlo formateado. Si no, devuelve la línea cruda.
   */
  function prettyBody(body: string): string {
    if (!body) return '';
    const trimmed = body.trim();
    if (!trimmed.startsWith('{') && !trimmed.startsWith('[')) return body;
    try {
      const v = JSON.parse(trimmed);
      if (typeof v === 'object' && v !== null) return JSON.stringify(v, null, 2);
    } catch {
      /* no es JSON: cae al return */
    }
    return body;
  }

  $: stackTrace = log ? (log.attributes?.['exception.stacktrace'] ?? log.attributes?.['stack_trace'] ?? '') : '';
  let stackOpen = false;
</script>

<svelte:window on:keydown={onKeydown} />

{#if log}
  <!-- svelte-ignore a11y-click-events-have-key-events a11y-no-static-element-interactions -->
  <aside
    bind:this={panelEl}
    class="ld"
    class:resizing
    style="width: {width}px"
    role="dialog"
    aria-label="Detalle del log"
  >
    <!-- Handle de resize -->
    <!-- svelte-ignore a11y-no-static-element-interactions -->
    <div
      class="ld-resize"
      role="separator"
      aria-orientation="vertical"
      aria-label="Redimensionar"
      on:mousedown={onResizeStart}
    ></div>

    <header class="ld-header">
      <div class="ld-header-top">
        <SeverityBadge severity={log.severity_text} />
        <span class="ld-ts mono">{formatTimestamp(log.timestamp)}</span>
        <div class="ld-header-actions">
          {#if position}
            <span class="muted" style="font-size: 11.5px;">
              {position.index + 1} de {position.total}
              <span class="kbd-hint mono"><kbd>j</kbd><kbd>k</kbd></span>
            </span>
          {/if}
          <button class="ld-icon-btn" on:click={() => dispatch('close')} title="Cerrar (Esc)" aria-label="Cerrar">×</button>
        </div>
      </div>
      <div class="ld-header-sub mono">
        <button class="ld-link" on:click={() => applyFilter('service', log.service_name)} title="Filtrar por este servicio">
          {log.service_name}
        </button>
        {#if env}<span class="ld-sep">·</span><span>{env}</span>{/if}
        {#if host}<span class="ld-sep">·</span><span>{host}</span>{/if}
      </div>
    </header>

    <div class="ld-body">
      <!-- Mensaje principal -->
      <section class="ld-section">
        <h3>Mensaje</h3>
        <pre class="ld-msg">{prettyBody(log.body)}</pre>
      </section>

      <!-- Stack trace si hay -->
      {#if stackTrace}
        <section class="ld-section">
          <h3>
            <button class="ld-collapse" on:click={() => (stackOpen = !stackOpen)}>
              <span>{stackOpen ? '▾' : '▸'}</span>
              <span>Stack trace</span>
              <span class="muted" style="font-size: 11px; font-weight: normal;">
                ({stackTrace.split('\n').length} líneas)
              </span>
            </button>
          </h3>
          {#if stackOpen}
            <pre class="ld-stack">{stackTrace}</pre>
          {/if}
        </section>
      {/if}

      <!-- Atributos -->
      {#if attrs.length > 0}
        <section class="ld-section">
          <h3>Atributos ({attrs.length})</h3>
          <table class="ld-attrs">
            <tbody>
              {#each attrs as a (a.source + ':' + a.key)}
                <tr>
                  <td class="ld-attr-key mono" title={a.source === 'resource' ? 'resource attribute' : 'log attribute'}>
                    {#if a.source === 'resource'}<span class="ld-attr-flag" title="resource">R</span>{/if}
                    {a.key}
                  </td>
                  <td class="ld-attr-val mono">{a.value}</td>
                  <td class="ld-attr-actions">
                    <button
                      class="ld-icon-btn small"
                      title="Filtrar por este valor"
                      on:click={() => applyFilter('query', a.value)}
                    >▾</button>
                    <button
                      class="ld-icon-btn small"
                      title="Copiar valor"
                      on:click={() => copy(a.value, a.key)}
                    >⎘</button>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </section>
      {/if}

      <!-- Navegación contextual -->
      <section class="ld-section">
        <h3>Contexto</h3>
        <div class="ld-shortcuts">
          {#if log.trace_id}
            <a href={`/traces/${log.trace_id}`} class="ld-shortcut">
              <span class="ld-shortcut-icon">⤳</span>
              <span class="ld-shortcut-label">Ver traza</span>
              <span class="ld-shortcut-sub mono">{log.trace_id.slice(0, 16)}…</span>
            </a>
          {/if}
          <button
            class="ld-shortcut"
            on:click={async () => { contextOpen = !contextOpen; if (contextOpen) await loadContext(); }}
          >
            <span class="ld-shortcut-icon">📋</span>
            <span class="ld-shortcut-label">Logs ±2 min alrededor</span>
            <span class="ld-shortcut-sub">{contextOpen ? '▾ ocultar' : '▸ mostrar'}</span>
          </button>
          <button class="ld-shortcut" on:click={() => applyFilter('service', log.service_name)}>
            <span class="ld-shortcut-icon">🔍</span>
            <span class="ld-shortcut-label">Más logs de {log.service_name}</span>
          </button>
          {#if host}
            <button class="ld-shortcut" on:click={() => applyFilter('query', host)}>
              <span class="ld-shortcut-icon">📋</span>
              <span class="ld-shortcut-label">Otros logs en {host}</span>
            </button>
          {/if}
        </div>

        {#if contextOpen}
          <div class="ld-context">
            <div class="ld-context-bar">
              <label>
                <input type="checkbox" bind:checked={contextSameService} on:change={loadContext} />
                Solo del mismo servicio
              </label>
              <button class="ld-icon-btn small" on:click={loadContext} title="Recargar">↻</button>
            </div>

            {#if context.kind === 'loading'}
              <div class="muted" style="padding: 16px; text-align: center;">
                <span class="spinner"></span> cargando ventana…
              </div>
            {:else if context.kind === 'err'}
              <div style="color: var(--danger); padding: 8px;">Error: {context.message}</div>
            {:else if context.kind === 'ok'}
              {#if context.rows.length === 0}
                <div class="muted" style="padding: 12px; text-align: center;">Sin otros logs en la ventana.</div>
              {:else}
                <div class="ld-context-list">
                  {#each context.rows as r (r.timestamp + r.body)}
                    <!-- svelte-ignore a11y-click-events-have-key-events -->
                    <div
                      class="ld-context-row"
                      class:focused={isFocusedRow(r)}
                      role="button"
                      tabindex="0"
                      on:click={() => dispatch('jump', { timestamp: r.timestamp })}
                    >
                      <span class="ld-context-ts mono">{r.timestamp.slice(11, 23)}</span>
                      <SeverityBadge severity={r.severity_text} />
                      <span class="ld-context-svc muted mono">{r.service_name}</span>
                      <span class="ld-context-body">{r.body}</span>
                    </div>
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/if}
      </section>
    </div>

    <!-- Acciones globales -->
    <footer class="ld-footer">
      <button on:click={copyAsJson} title="Copiar el log completo como JSON (Cmd+C)">
        <span class="mono">⎘</span> Copiar JSON
      </button>
      <button on:click={copyBody} title="Copiar solo el mensaje">
        <span class="mono">⎘</span> Copiar mensaje
      </button>
      <button on:click={shareLink} title="Copiar enlace a este log">
        🔗 Compartir
      </button>
    </footer>

    {#if toastMsg}
      <div class="ld-toast">{toastMsg}</div>
    {/if}
  </aside>
{/if}

<style>
  .ld {
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
  .ld.resizing { transition: none; user-select: none; }

  .ld-resize {
    position: absolute;
    top: 0;
    left: -3px;
    width: 6px;
    height: 100%;
    cursor: col-resize;
    background: transparent;
    z-index: 2;
  }
  .ld-resize:hover { background: var(--accent); opacity: 0.35; }

  .ld-header {
    padding: 12px 16px 8px;
    border-bottom: 1px solid var(--border);
    flex-shrink: 0;
  }
  .ld-header-top {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .ld-ts {
    font-size: 12px;
    color: var(--text-muted);
  }
  .ld-header-actions {
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
  .ld-header-sub {
    margin-top: 6px;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    color: var(--text-muted);
    flex-wrap: wrap;
  }
  .ld-sep { color: var(--text-muted); }
  .ld-link {
    background: transparent;
    border: 0;
    padding: 0;
    color: var(--text);
    font-family: inherit;
    font-size: inherit;
    cursor: pointer;
    border-bottom: 1px dashed var(--border);
  }
  .ld-link:hover { color: var(--accent); border-bottom-color: var(--accent); }

  .ld-body {
    flex: 1;
    overflow-y: auto;
    padding: 12px 16px;
  }
  .ld-section { margin-bottom: 18px; }
  .ld-section h3 {
    margin: 0 0 6px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
  }
  .ld-collapse {
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
  .ld-msg {
    margin: 0;
    padding: 10px 12px;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 12.5px;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 320px;
    overflow: auto;
  }
  .ld-stack {
    margin: 6px 0 0;
    padding: 10px 12px;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    font-size: 11.5px;
    max-height: 360px;
    overflow: auto;
  }

  .ld-attrs {
    width: 100%;
    border-collapse: separate;
    border-spacing: 0;
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
    table-layout: fixed;
  }
  .ld-attrs td {
    padding: 4px 8px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    vertical-align: top;
    background: transparent;
  }
  .ld-attrs tr:last-child td { border-bottom: 0; }
  .ld-attrs tr:hover td { background: var(--bg-hover); }
  .ld-attr-key {
    width: 38%;
    color: var(--text-muted);
    word-break: break-all;
    position: relative;
    padding-left: 8px;
  }
  .ld-attr-flag {
    display: inline-block;
    font-size: 9px;
    background: var(--bg);
    border: 1px solid var(--border);
    color: var(--text-muted);
    padding: 0 3px;
    border-radius: 2px;
    margin-right: 4px;
  }
  .ld-attr-val {
    word-break: break-all;
    white-space: pre-wrap;
  }
  .ld-attr-actions {
    width: 60px;
    text-align: right;
    white-space: nowrap;
  }

  .ld-icon-btn {
    background: transparent;
    border: 1px solid transparent;
    border-radius: 4px;
    padding: 2px 8px;
    cursor: pointer;
    color: var(--text-muted);
    font-size: 13px;
    line-height: 1.2;
  }
  .ld-icon-btn:hover { background: var(--bg-hover); color: var(--text); border-color: var(--border); }
  .ld-icon-btn.small { padding: 0 5px; font-size: 11.5px; }

  .ld-shortcuts {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .ld-shortcut {
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
  .ld-shortcut:hover { background: var(--bg-hover); text-decoration: none; }
  .ld-shortcut-icon {
    width: 18px;
    text-align: center;
    flex-shrink: 0;
  }
  .ld-shortcut-label { flex: 1; font-size: 13px; }
  .ld-shortcut-sub {
    color: var(--text-muted);
    font-size: 11.5px;
  }

  .ld-context {
    margin-top: 8px;
    border: 1px solid var(--border);
    border-radius: 4px;
    overflow: hidden;
  }
  .ld-context-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 10px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .ld-context-bar label { display: inline-flex; align-items: center; gap: 6px; cursor: pointer; }
  .ld-context-list {
    max-height: 380px;
    overflow-y: auto;
  }
  .ld-context-row {
    display: grid;
    grid-template-columns: 90px 70px 110px 1fr;
    gap: 8px;
    align-items: center;
    padding: 4px 10px;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    cursor: pointer;
  }
  .ld-context-row:last-child { border-bottom: 0; }
  .ld-context-row:hover { background: var(--bg-hover); }
  .ld-context-row.focused {
    background: var(--bg-hover);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .ld-context-ts { color: var(--text-muted); white-space: nowrap; font-size: 11.5px; }
  .ld-context-svc {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11.5px;
  }
  .ld-context-body {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: "JetBrains Mono", Menlo, monospace;
    font-size: 11.5px;
  }

  .ld-footer {
    display: flex;
    gap: 6px;
    padding: 8px 12px;
    border-top: 1px solid var(--border);
    background: var(--bg);
    flex-shrink: 0;
  }
  .ld-footer button {
    font-size: 12px;
    padding: 4px 10px;
  }

  .ld-toast {
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
    .ld {
      width: 100vw !important;
    }
    .ld-resize { display: none; }
  }
</style>
