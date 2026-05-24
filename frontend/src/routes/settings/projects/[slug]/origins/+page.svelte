<script lang="ts">
  import { page } from '$app/stores';
  import { fetchOrigins, saveOrigins, type OriginConfig } from '$lib/api';

  // SvelteKit tipa params como `string | undefined`; la ruta garantiza presencia.
  $: slug = $page.params.slug ?? '';

  let config: OriginConfig = { enabled: false, origins: [] };
  let loading = true;
  let saving = false;
  let error = '';
  let toast = '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const r = await fetchOrigins(slug);
      config = r.config;
      if (!Array.isArray(config.origins)) config.origins = [];
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  $: if (slug) load();

  function add(): void {
    config.origins = [...config.origins, ''];
  }

  function remove(i: number): void {
    config.origins = config.origins.filter((_, idx) => idx !== i);
  }

  async function save(): Promise<void> {
    saving = true;
    error = '';
    try {
      // Filtramos vacíos antes de enviar — el backend los rechazaría con un 400
      // poco útil si quedó un input nuevo sin escribir.
      const trimmed = config.origins.map((s) => s.trim()).filter((s) => s.length > 0);
      const r = await saveOrigins(slug, { enabled: config.enabled, origins: trimmed });
      config = r.config;
      if (!Array.isArray(config.origins)) config.origins = [];
      toast = config.enabled
        ? `Guardado. ${config.origins.length} origen(es) permitido(s).`
        : 'Guardado (verificación de origen desactivada).';
      setTimeout(() => (toast = ''), 3000);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="page-header">
  <h1 class="page-title">Orígenes RUM · {slug}</h1>
  <a href="/settings/projects"><button type="button">← Volver</button></a>
</div>

<p class="muted" style="max-width: 740px;">
  El token de ingesta del SDK browser es <strong>público</strong> — viaja en el bundle JS.
  Sin esta whitelist, cualquiera que lo extraiga puede mandar eventos falsos desde un
  dominio arbitrario. Activala para que el backend acepte requests con cabecera
  <code>Origin</code> sólo desde los dominios que listes acá. Los SDKs server-side
  (Node, Python, Go, …) no mandan <code>Origin</code> y siguen funcionando sin cambios.
</p>

{#if error}<div class="card mt-8" style="color: var(--danger);">{error}</div>{/if}
{#if toast}<div class="card mt-8">{toast}</div>{/if}

{#if loading}
  <div class="empty mt-16"><span class="spinner"></span></div>
{:else}
  <div class="card mt-16">
    <label style="display: flex; align-items: center; gap: 8px; cursor: pointer;">
      <input type="checkbox" bind:checked={config.enabled} />
      <strong>Activar verificación de Origin para este proyecto</strong>
    </label>
    <div class="muted" style="font-size: 12px; margin-top: 4px;">
      Apagado = compat: cualquier origen pasa.
    </div>
  </div>

  <h2 style="font-size: 16px; margin-top: 24px;">Orígenes permitidos</h2>
  <div class="card mt-8">
    <div class="muted" style="font-size: 12px; margin-bottom: 12px;">
      Formato <code>scheme://host[:port]</code>. Wildcard de un solo subdominio
      soportado: <code>https://*.example.com</code> matchea <code>app.example.com</code>
      pero <strong>no</strong> <code>example.com</code> ni
      <code>foo.bar.example.com</code> (evita bypass por wildcard greedy).
    </div>

    {#if config.origins.length === 0}
      <div class="muted">Sin orígenes. Agregá al menos uno antes de activar.</div>
    {:else}
      {#each config.origins as _o, i}
        <div class="row">
          <input
            class="mono"
            bind:value={config.origins[i]}
            placeholder="https://app.example.com"
            disabled={!config.enabled}
          />
          <button type="button" class="danger" on:click={() => remove(i)}>Eliminar</button>
        </div>
      {/each}
    {/if}

    <div style="margin-top: 12px;">
      <button type="button" on:click={add} disabled={!config.enabled}>+ Añadir origen</button>
    </div>
  </div>

  <div style="margin-top: 24px;">
    <button class="primary" on:click={save} disabled={saving}>
      {saving ? 'Guardando…' : 'Guardar'}
    </button>
  </div>
{/if}

<style>
  .row {
    display: flex;
    gap: 8px;
    align-items: center;
    margin-bottom: 8px;
  }
  .row input {
    flex: 1;
    min-width: 0;
  }
</style>
