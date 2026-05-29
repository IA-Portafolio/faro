<script lang="ts">
  /**
   * Empty state con onboarding. Se monta cuando una página de exploración
   * (logs, traces, errors, metrics) o el resumen no tiene datos para mostrar.
   *
   * En vez de un "vacío" genérico, da al usuario lo que necesita para que
   * la sesión sea útil:
   *   - Snippet de instalación del SDK del proyecto seleccionado (o el primero
   *     si no hay ninguno seleccionado), con su token de ingesta ya inyectado.
   *   - Comando `curl` listo para copiar-pegar y verificar que el endpoint
   *     responde sin tocar ningún SDK.
   *   - Link a la API docs (/docs) para explorar el resto del contrato.
   *   - Si no hay ningún proyecto creado, ofrece el CTA para crearlo.
   *
   * La prop `kind` solo cambia el copy ("Sin logs aún" vs "Sin trazas aún"…),
   * el resto es común — porque la ingesta es la misma para todos los signals.
   */
  import { onMount } from 'svelte';
  import { fetchProjects, type Project } from '$lib/api';
  import { selectedProject } from '$lib/stores';
  import {
    curlProbe,
    groupLabels,
    groupOrder,
    otlpCurlProbe,
    otlpSnippetsFor,
    snippetsFor,
    type Snippet
  } from '$lib/sdk-snippets';

  type EmptyKind = 'logs' | 'traces' | 'errors' | 'metrics' | 'events' | 'summary';
  export let kind: EmptyKind = 'logs';
  /** Si la página puede distinguir "sin datos *aún*" vs "sin datos *en este rango*",
   *  lo pasa aquí. En el segundo caso el copy se suaviza. */
  export let filteredOut = false;

  let projects: Project[] = [];
  let loadingProjects = true;
  let loadError = '';
  /** Override manual del proyecto activo desde el dropdown del propio empty state. */
  let manualSlug = '';

  /** Métricas y trazas SOLO entran por OTLP — los SDKs `@iaportafolio/*`
   *  no las cubren. En el resto de signals usamos los snippets nativos. */
  $: useOtlp = kind === 'metrics' || kind === 'traces';
  /** El tab por defecto cambia según el set de snippets activo. */
  let activeTab = 'node';
  $: activeTab = useOtlp ? 'otel-node' : 'node';

  onMount(async () => {
    try {
      projects = await fetchProjects();
    } catch (e: unknown) {
      loadError = e instanceof Error ? e.message : String(e);
    } finally {
      loadingProjects = false;
    }
  });

  /** El proyecto que protagoniza los snippets: el seleccionado en el sidebar,
   *  si no, el del dropdown propio, si no, el primero disponible. */
  $: focusProject = (() => {
    if (manualSlug) return projects.find((p) => p.slug === manualSlug) ?? null;
    const sel = $selectedProject;
    if (sel) return projects.find((p) => p.slug === sel) ?? null;
    return projects[0] ?? null;
  })();

  $: snippets = focusProject
    ? useOtlp
      ? otlpSnippetsFor(focusProject, kind as 'metrics' | 'traces')
      : snippetsFor(focusProject)
    : ([] as Snippet[]);
  $: curl = focusProject
    ? useOtlp
      ? otlpCurlProbe(focusProject, kind as 'metrics' | 'traces')
      : curlProbe(focusProject)
    : '';

  // ---------- Copy helpers ----------

  let toastMsg = '';
  let toastTimer: ReturnType<typeof setTimeout> | null = null;
  function flash(msg: string): void {
    toastMsg = msg;
    if (toastTimer) clearTimeout(toastTimer);
    toastTimer = setTimeout(() => { toastMsg = ''; }, 1600);
  }
  async function copy(text: string, label: string): Promise<void> {
    try {
      await navigator.clipboard?.writeText(text);
      flash(`✓ ${label} copiado`);
    } catch {
      window.prompt('Copia este texto:', text);
    }
  }

  // ---------- Copy por kind ----------

  const headlines: Record<EmptyKind, { idle: string; filtered: string }> = {
    logs:    { idle: 'Aún no llegan logs', filtered: 'Sin logs en este rango' },
    traces:  { idle: 'Aún no llegan trazas', filtered: 'Sin trazas en este rango' },
    errors:  { idle: 'Aún no hay errores capturados', filtered: 'Sin errores en este rango' },
    metrics: { idle: 'Aún no llegan métricas', filtered: 'Sin métricas en este rango' },
    events:  { idle: 'Aún no llegan eventos de producto', filtered: 'Sin eventos en este rango' },
    summary: { idle: '¡Bienvenido a Faro!', filtered: 'Sin actividad en el rango actual' }
  };
  const blurbs: Record<EmptyKind, string> = {
    logs:    'Envía tu primer log desde cualquier servicio en menos de un minuto.',
    traces:  'Las trazas entran por OTLP — los SDKs `@iaportafolio/*` sólo envían logs y errores. Usa el SDK oficial de OpenTelemetry de tu lenguaje y apúntalo a `/v1/traces`.',
    errors:  'Llama a `captureException(err)` desde tu SDK para que se agrupe aquí por fingerprint.',
    metrics: 'Las métricas entran por OTLP — los SDKs `@iaportafolio/*` sólo envían logs y errores. Usa el SDK oficial de OpenTelemetry de tu lenguaje y apúntalo a `/v1/metrics`.',
    events:  'Llama a `track(eventName, properties)` desde el SDK para registrar acciones del usuario.',
    summary: 'Empieza enviando tu primer log: el resumen se llena en cuanto llegue algo.'
  };
</script>

<section class="oe">
  {#if loadingProjects}
    <div class="oe-loading">
      <span class="spinner"></span> cargando proyectos…
    </div>
  {:else if loadError}
    <div class="oe-error">No se pudo cargar la lista de proyectos: {loadError}</div>
  {:else if projects.length === 0}
    <div class="oe-hero">
      <h2>Empieza creando un proyecto</h2>
      <p class="muted">
        Un proyecto agrupa tus servicios y tiene su propio token de ingesta.
        Sin proyecto no hay forma de autenticar la entrada de datos.
      </p>
      <div class="oe-actions">
        <a class="oe-cta" href="/settings/projects">+ Crear primer proyecto</a>
        <a class="oe-link" href="/docs" target="_blank" rel="noreferrer">Ver documentación de la API ↗</a>
      </div>
    </div>
  {:else}
    <header class="oe-head">
      <h2>{filteredOut ? headlines[kind].filtered : headlines[kind].idle}</h2>
      <p class="muted oe-blurb">{blurbs[kind]}</p>
    </header>

    <div class="oe-project-bar">
      <label for="oe-project">Snippets para</label>
      <select id="oe-project" bind:value={manualSlug}>
        {#each projects as p}
          <option value={p.slug}>{p.name} ({p.slug})</option>
        {/each}
      </select>
      {#if focusProject}
        <span class="muted oe-token mono" title="Token de ingesta (oculto)">
          token: {focusProject.ingest_token.slice(0, 6)}…{focusProject.ingest_token.slice(-4)}
        </span>
      {/if}
    </div>

    {#if focusProject}
      <!-- ▸ SDK snippets -->
      <div class="oe-card">
        <div class="oe-card-head">
          <h3>1. {useOtlp ? 'Instrumenta con OpenTelemetry' : 'Instala el SDK'}</h3>
          {#if !useOtlp}
            <a class="oe-link" href="/settings/projects">Ver todos los lenguajes →</a>
          {/if}
        </div>
        <div class="oe-tabs" role="tablist">
          {#each groupOrder as g}
            {@const items = snippets.filter((s) => s.group === g)}
            {#if items.length > 0}
              <div class="oe-tab-group">
                <span class="oe-tab-group-label">{groupLabels[g]}</span>
                <div class="oe-tab-row">
                  {#each items as s}
                    <button
                      type="button"
                      role="tab"
                      aria-selected={activeTab === s.id}
                      class="oe-tab"
                      class:active={activeTab === s.id}
                      on:click={() => (activeTab = s.id)}
                    >{s.label}</button>
                  {/each}
                </div>
              </div>
            {/if}
          {/each}
        </div>

        {#each snippets as s}
          {#if activeTab === s.id}
            <div class="muted mono oe-install">$ {s.install}</div>
            <pre class="oe-code">{s.code}</pre>
            <div class="oe-copy-row">
              <button on:click={() => copy(s.code, s.label)}>Copiar código</button>
            </div>
          {/if}
        {/each}
      </div>

      <!-- ▸ Verifica con curl -->
      <div class="oe-card">
        <div class="oe-card-head">
          <h3>2. Verifica desde la terminal</h3>
          <span class="muted" style="font-size: 12px;">cero dependencias</span>
        </div>
        <p class="muted" style="margin: 0 0 8px;">
          Antes de instrumentar nada, comprueba que el endpoint acepta tu token:
        </p>
        <pre class="oe-code">{curl}</pre>
        <div class="oe-copy-row">
          <button on:click={() => copy(curl, 'comando curl')}>Copiar comando</button>
          <a class="oe-link" href="/docs" target="_blank" rel="noreferrer">Documentación completa ↗</a>
        </div>
      </div>

      <!-- ▸ Próximos pasos -->
      <div class="oe-hints">
        <div class="oe-hint">
          <strong>Refresca pasados unos segundos.</strong>
          <span class="muted">Los datos aparecen en cuanto el SDK haga el primer envío.</span>
        </div>
        <div class="oe-hint">
          <strong>¿Ya enviaste algo?</strong>
          <span class="muted">Comprueba que el rango temporal arriba a la derecha cubre los últimos minutos.</span>
        </div>
      </div>
    {/if}
  {/if}

  {#if toastMsg}
    <div class="oe-toast">{toastMsg}</div>
  {/if}
</section>

<style>
  .oe {
    position: relative;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 24px;
    max-width: 900px;
    margin: 12px auto;
  }
  .oe-loading {
    display: flex;
    align-items: center;
    gap: 10px;
    justify-content: center;
    padding: 32px;
    color: var(--text-muted);
  }
  .oe-error {
    padding: 12px;
    color: var(--danger);
    text-align: center;
  }

  .oe-hero {
    text-align: center;
    padding: 24px 8px;
  }
  .oe-hero h2 {
    margin: 0 0 8px;
    font-size: 22px;
  }
  .oe-actions {
    margin-top: 18px;
    display: flex;
    gap: 12px;
    justify-content: center;
    flex-wrap: wrap;
  }
  .oe-cta {
    background: var(--accent);
    color: var(--accent-fg);
    padding: 10px 18px;
    border-radius: 6px;
    font-weight: 600;
    text-decoration: none;
  }
  .oe-cta:hover {
    background: var(--accent-dim);
    text-decoration: none;
  }
  .oe-link {
    color: var(--accent);
    text-decoration: none;
    font-size: 13px;
  }
  .oe-link:hover { text-decoration: underline; }

  .oe-head h2 {
    margin: 0 0 6px;
    font-size: 20px;
  }
  .oe-blurb {
    margin: 0 0 20px;
    font-size: 13px;
    max-width: 640px;
  }

  .oe-project-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-bottom: 16px;
    flex-wrap: wrap;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .oe-project-bar label {
    font-size: 12px;
    color: var(--text-muted);
  }
  .oe-token { font-size: 11.5px; }

  .oe-card {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 16px;
    margin-bottom: 14px;
  }
  .oe-card-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    margin-bottom: 12px;
  }
  .oe-card-head h3 {
    margin: 0;
    font-size: 14px;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    font-weight: 600;
  }

  .oe-tabs {
    display: flex;
    flex-direction: column;
    gap: 8px;
    border-bottom: 1px solid var(--border);
    padding-bottom: 10px;
    margin-bottom: 10px;
  }
  .oe-tab-group {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .oe-tab-group-label {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-muted);
    min-width: 64px;
  }
  .oe-tab-row {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .oe-tab {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 4px 10px;
    font-size: 12px;
    cursor: pointer;
    color: var(--text-muted);
  }
  .oe-tab:hover { background: var(--bg-hover); color: var(--text); }
  .oe-tab.active {
    background: var(--accent);
    color: var(--accent-fg);
    border-color: var(--accent);
    font-weight: 600;
  }

  .oe-install {
    font-size: 12px;
    margin-bottom: 6px;
  }
  .oe-code {
    margin: 0;
    padding: 12px;
    max-height: 320px;
    overflow: auto;
  }
  .oe-copy-row {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 10px;
  }

  .oe-hints {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 10px;
    margin-top: 16px;
  }
  .oe-hint {
    padding: 10px 12px;
    border: 1px solid var(--border);
    background: var(--bg);
    border-radius: 4px;
    font-size: 12.5px;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .oe-toast {
    position: absolute;
    bottom: 12px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 5px 12px;
    font-size: 12px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
    pointer-events: none;
  }
</style>
