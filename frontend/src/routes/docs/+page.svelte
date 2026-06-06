<script lang="ts">
  /**
   * Página `/docs` — referencia pública de los SDKs de Faro (acceso anónimo).
   *
   * Renderiza la documentación de métodos por SDK desde `$lib/sdk-docs` (la fuente
   * de verdad), con selector de SDK y buscador que filtra por firma o resumen del
   * método. Es pública: el layout raíz la deja ver sin sesión. Sus variantes
   * `/docs.md` y `/llms.txt` sirven la misma info en texto plano.
   */
  import {
    sdks,
    profileDefaults,
    commonOptions,
    severities,
    productMatrix,
    totalMethods,
    type SdkDoc,
    type SdkMethodGroup
  } from '$lib/sdk-docs';
  import { toast } from '$lib/toasts';

  let activeId = sdks[0].id;
  let query = '';

  $: active = sdks.find((s) => s.id === activeId) ?? sdks[0];
  $: q = query.trim().toLowerCase();

  // Cuando hay búsqueda, filtra los métodos del SDK activo. Devuelve los
  // grupos con al menos un método que matchee firma o resumen.
  function filterGroups(sdk: SdkDoc, needle: string): SdkMethodGroup[] {
    if (!needle) return sdk.groups;
    return sdk.groups
      .map((g) => ({
        ...g,
        methods: g.methods.filter(
          (m) =>
            m.signature.toLowerCase().includes(needle) ||
            m.summary.toLowerCase().includes(needle)
        )
      }))
      .filter((g) => g.methods.length > 0);
  }

  // Cuántos métodos del SDK matchean la búsqueda — alimenta el badge del pill.
  function matchCount(sdk: SdkDoc, needle: string): number {
    if (!needle) return sdk.groups.reduce((n, g) => n + g.methods.length, 0);
    return sdk.groups.reduce(
      (n, g) =>
        n +
        g.methods.filter(
          (m) =>
            m.signature.toLowerCase().includes(needle) ||
            m.summary.toLowerCase().includes(needle)
        ).length,
      0
    );
  }

  $: groups = filterGroups(active, q);

  async function copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      toast.success('Copiado al portapapeles');
    } catch {
      toast.error('No se pudo acceder al portapapeles');
    }
  }

  // Si la búsqueda deja sin resultados al SDK activo, salta al primero que sí
  // tenga matches para que el usuario no vea una página vacía. Va en el handler
  // del input (no en un `$:`) para no crear un ciclo reactivo con `active`.
  function onSearch(): void {
    const needle = query.trim().toLowerCase();
    if (needle && matchCount(active, needle) === 0) {
      const firstWithMatch = sdks.find((s) => matchCount(s, needle) > 0);
      if (firstWithMatch) activeId = firstWithMatch.id;
    }
  }
</script>

<svelte:head>
  <title>SDKs &amp; referencia de API · Faro</title>
  <meta
    name="description"
    content="Referencia pública de los SDKs de Faro (Node.js, Next.js, Expo, Python, Go, Flutter, Kotlin) y todos sus métodos. Versión en texto para LLMs en /docs.md y /llms.txt."
  />
  <meta name="robots" content="index, follow" />
  <link rel="alternate" type="text/markdown" href="/docs.md" title="Referencia completa en Markdown" />
</svelte:head>

<div class="page-header">
  <div>
    <h1 class="page-title">SDKs &amp; referencia de API</h1>
    <div class="muted" style="font-size: 13px; margin-top: 2px;">
      {sdks.length} SDKs · {totalMethods()} métodos documentados · misma API conceptual en todos
    </div>
    <div class="llm-links">
      <span class="muted">¿LLM o agente?</span>
      <a href="/docs.md" target="_blank" rel="noopener">/docs.md</a>
      <span class="muted">·</span>
      <a href="/llms.txt" target="_blank" rel="noopener">/llms.txt</a>
      <span class="muted">— texto plano, sin login</span>
    </div>
  </div>
  <input
    type="search"
    placeholder="Buscar método… (track, captureException, flush)"
    bind:value={query}
    on:input={onSearch}
    style="min-width: 280px;"
    aria-label="Buscar método"
  />
</div>

<!-- Selector de SDK -->
<div class="pills">
  {#each sdks as s}
    {@const count = matchCount(s, q)}
    <button
      type="button"
      class="pill"
      class:active={s.id === activeId}
      class:dim={q !== '' && count === 0}
      on:click={() => (activeId = s.id)}
    >
      <span>{s.name}</span>
      <span class="pill-count">{count}</span>
    </button>
  {/each}
</div>

<!-- Cabecera del SDK activo -->
<div class="card sdk-head">
  <div class="sdk-head-top">
    <div>
      <h2 class="sdk-name">{active.name}</h2>
      <div class="muted" style="font-size: 13px;">{active.language}</div>
    </div>
    <span class="badge profile-{active.profile}">Perfil {profileDefaults[active.profile].label}</span>
  </div>

  <p style="margin: 4px 0 0; color: var(--text-muted);">{active.blurb}</p>

  <div class="caps">
    {#each active.capabilities as c}
      <span class="cap">{c}</span>
    {/each}
  </div>

  <div class="install-row">
    <code class="install-code mono">{active.install}</code>
    <button type="button" on:click={() => copy(active.install)} title="Copiar comando">Copiar</button>
  </div>

  <div class="defaults muted mono">
    paquete <strong>{active.pkg}</strong> · defaults: flush {profileDefaults[active.profile].flushMs}ms ·
    batch {profileDefaults[active.profile].batch} · cola {profileDefaults[active.profile].queue.toLocaleString()}
  </div>

  <div class="code-block">
    <button type="button" class="copy-fab" on:click={() => copy(active.initExample)} title="Copiar snippet">Copiar</button>
    <pre><code>{active.initExample}</code></pre>
  </div>
</div>

<!-- Métodos del SDK activo -->
{#if groups.length === 0}
  <div class="empty">Ningún método de {active.name} coincide con «{query}».</div>
{:else}
  {#each groups as group}
    <section class="method-group">
      <div class="group-head">
        <h3 class="group-title">{group.title}</h3>
        {#if group.note}<span class="muted" style="font-size: 12px;">{group.note}</span>{/if}
      </div>
      <div class="method-table">
        {#each group.methods as m}
          <div class="method-row">
            <div class="method-sig">
              <code class="mono">{m.signature}</code>
              {#if m.returns}<span class="returns mono">→ {m.returns}</span>{/if}
            </div>
            <div class="method-desc">{m.summary}</div>
          </div>
        {/each}
      </div>
    </section>
  {/each}
{/if}

<!-- Referencia común a todos los SDKs -->
<h2 class="section-title">Referencia común</h2>
<p class="muted" style="margin-top: -4px;">
  Estas piezas se comportan igual en los 7 SDKs: si vienes de uno, esperas lo mismo en otro.
</p>

<div class="ref-grid">
  <section class="card">
    <h3 class="group-title">Opciones de <code class="mono">init()</code></h3>
    <div class="method-table" style="margin-top: 8px;">
      {#each commonOptions as o}
        <div class="method-row">
          <div class="method-sig">
            <code class="mono">{o.name}</code>
            <span class="returns mono">{o.type}</span>
          </div>
          <div class="method-desc">
            {o.desc}
            {#if o.default !== '—'}<span class="muted"> · default: {o.default}</span>{/if}
          </div>
        </div>
      {/each}
    </div>
  </section>

  <div class="ref-side">
    <section class="card">
      <h3 class="group-title">Severidades</h3>
      <div style="margin-top: 10px; display: flex; flex-direction: column; gap: 6px;">
        {#each severities as s}
          <div class="flex center between">
            <span class="badge {s.cls}">{s.text}</span>
            <span class="muted mono">OTel {s.num}</span>
          </div>
        {/each}
      </div>
    </section>

    <section class="card" style="margin-top: 16px;">
      <h3 class="group-title">Endpoint nativo</h3>
      <pre style="margin-top: 8px;"><code>POST /api/v1/ingest/logs
Authorization: Bearer &lt;token&gt;
POST /api/v1/ingest/events</code></pre>
      <p class="muted" style="font-size: 12px; margin: 8px 0 0;">
        ¿Prefieres OpenTelemetry estándar? Apunta tu OTLP exporter a
        <code class="mono">/v1/logs · /v1/traces · /v1/metrics</code>.
      </p>
    </section>
  </div>
</div>

<section class="card mt-16">
  <h3 class="group-title">Disponibilidad de la API de producto</h3>
  <p class="muted" style="font-size: 12px; margin: 4px 0 10px;">
    <code class="mono">page</code> solo donde hay routing de cliente; <code class="mono">screen</code> solo en móvil.
  </p>
  <div style="overflow-x: auto;">
    <table>
      <thead>
        <tr>
          <th>SDK</th><th>track</th><th>identify</th><th>page</th><th>screen</th><th>alias</th>
        </tr>
      </thead>
      <tbody>
        {#each productMatrix as r}
          <tr>
            <td>{r.sdk}</td>
            <td class="cell">{r.track ? '✔' : '–'}</td>
            <td class="cell">{r.identify ? '✔' : '–'}</td>
            <td class="cell">{r.page ? '✔' : '–'}</td>
            <td class="cell">{r.screen ? '✔' : '–'}</td>
            <td class="cell">{r.alias ? '✔' : '–'}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
</section>

<style>
  .llm-links {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 6px;
    font-size: 12.5px;
  }
  .llm-links a {
    font-family: "JetBrains Mono", "Fira Code", "SF Mono", Menlo, Consolas, monospace;
    font-size: 12px;
  }

  .pills {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-bottom: 16px;
  }
  .pill {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 6px 12px;
    border-radius: 999px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    color: var(--text);
    cursor: pointer;
    font-size: 13px;
  }
  .pill:hover { background: var(--bg-hover); }
  .pill.active {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-fg);
    font-weight: 600;
  }
  .pill.dim { opacity: 0.4; }
  .pill-count {
    font-size: 11px;
    background: color-mix(in srgb, var(--text-muted) 22%, transparent);
    color: var(--text-muted);
    border-radius: 999px;
    padding: 0 6px;
    line-height: 16px;
    font-variant-numeric: tabular-nums;
  }
  .pill.active .pill-count {
    background: color-mix(in srgb, var(--accent-fg) 25%, transparent);
    color: var(--accent-fg);
  }

  .sdk-head { margin-bottom: 20px; }
  .sdk-head-top {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 12px;
  }
  .sdk-name { font-size: 18px; font-weight: 600; margin: 0; }

  .caps { display: flex; flex-wrap: wrap; gap: 6px; margin: 12px 0; }
  .cap {
    font-size: 11px;
    padding: 2px 9px;
    border-radius: 999px;
    background: var(--badge-info-bg);
    color: var(--info);
    font-weight: 600;
  }

  .install-row {
    display: flex;
    align-items: stretch;
    gap: 8px;
    margin: 6px 0 8px;
  }
  .install-code {
    flex: 1;
    background: var(--code-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 8px 10px;
    overflow-x: auto;
    white-space: nowrap;
    display: flex;
    align-items: center;
  }

  .defaults { font-size: 12px; margin-bottom: 12px; }
  .defaults strong { color: var(--text); font-weight: 600; }

  .code-block { position: relative; }
  .code-block pre { margin: 0; }
  .copy-fab {
    position: absolute;
    top: 8px;
    right: 8px;
    font-size: 11px;
    padding: 3px 8px;
    z-index: 1;
    opacity: 0.85;
  }
  .copy-fab:hover { opacity: 1; }

  .profile-server { background: var(--badge-info-bg); color: var(--info); }
  .profile-mobile { background: var(--badge-ok-bg); color: var(--success); }
  .profile-browser { background: var(--badge-warn-bg); color: var(--warn); }

  .method-group { margin-bottom: 18px; }
  .group-head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    margin-bottom: 8px;
    flex-wrap: wrap;
  }
  .group-title { font-size: 14px; font-weight: 600; margin: 0; }

  .method-table {
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    background: var(--bg-elev);
  }
  .method-row {
    display: grid;
    grid-template-columns: minmax(220px, 40%) 1fr;
    gap: 16px;
    padding: 9px 14px;
    border-bottom: 1px solid var(--border);
    align-items: baseline;
  }
  .method-row:last-child { border-bottom: 0; }
  .method-row:hover { background: var(--bg-hover); }
  .method-sig {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .method-sig code {
    color: var(--accent);
    word-break: break-word;
  }
  .returns { font-size: 11px; color: var(--text-muted); }
  .method-desc { color: var(--text); font-size: 13px; }

  .section-title {
    font-size: 16px;
    font-weight: 600;
    margin: 28px 0 6px;
  }

  .ref-grid {
    display: grid;
    grid-template-columns: 1.4fr 1fr;
    gap: 16px;
    align-items: start;
  }
  .cell { text-align: center; font-variant-numeric: tabular-nums; }

  @media (max-width: 860px) {
    .ref-grid { grid-template-columns: 1fr; }
    .method-row { grid-template-columns: 1fr; gap: 4px; }
  }
</style>
