<script lang="ts">
  import { onMount } from 'svelte';
  import { page } from '$app/stores';
  import {
    fetchRedaction,
    saveRedaction,
    type RedactionConfig,
    type RedactionBuiltinInfo,
  } from '$lib/api';

  // SvelteKit tipa params como `string | undefined`. La ruta /settings/projects/[slug]/...
  // garantiza que `slug` está presente, pero el tipo lo declara opcional.
  // Coercer a '' es suficiente — load() corta temprano si está vacío.
  $: slug = $page.params.slug ?? '';

  let config: RedactionConfig = { enabled: false, builtins: [], custom: [] };
  let builtins: RedactionBuiltinInfo[] = [];
  let loading = true;
  let saving = false;
  let error = '';
  let toast = '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const r = await fetchRedaction(slug);
      config = r.config;
      // Aseguramos arrays vacíos en vez de undefined.
      if (!Array.isArray(config.builtins)) config.builtins = [];
      if (!Array.isArray(config.custom)) config.custom = [];
      builtins = r.available_builtins;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  $: if (slug) load();

  function isBuiltinOn(b: string): boolean {
    return config.builtins.includes(b);
  }

  function toggleBuiltin(b: string): void {
    if (isBuiltinOn(b)) {
      config.builtins = config.builtins.filter((x) => x !== b);
    } else {
      config.builtins = [...config.builtins, b];
    }
  }

  function addCustom(): void {
    config.custom = [
      ...config.custom,
      { name: '', pattern: '', replacement: '[REDACTED]' }
    ];
  }

  function removeCustom(i: number): void {
    config.custom = config.custom.filter((_, idx) => idx !== i);
  }

  async function save(): Promise<void> {
    saving = true;
    error = '';
    try {
      const r = await saveRedaction(slug, config);
      config = r.config;
      toast = config.enabled
        ? `Guardado. ${activeRulesCount(config)} reglas activas.`
        : 'Guardado (redacción desactivada).';
      setTimeout(() => (toast = ''), 3000);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  function activeRulesCount(c: RedactionConfig): number {
    return (c.enabled ? 1 : 0) * (c.builtins.length + c.custom.filter((x) => x.pattern).length);
  }
</script>

<div class="page-header">
  <h1 class="page-title">Redacción PII · {slug}</h1>
  <a href="/settings/projects"><button type="button">← Volver</button></a>
</div>

<p class="muted" style="max-width: 720px;">
  Las reglas se aplican en el servidor <strong>antes</strong> de escribir en ClickHouse —
  los datos crudos nunca tocan disco. Las reglas se aplican al <code>body</code> de los
  logs, valores de atributos, stack traces, y span/event attributes. Las <em>keys</em>
  de atributos NO se redactan (romperían los filtros del dashboard).
</p>

{#if error}<div class="card mt-8" style="color: var(--danger);">{error}</div>{/if}
{#if toast}<div class="card mt-8">{toast}</div>{/if}

{#if loading}
  <div class="empty mt-16"><span class="spinner"></span></div>
{:else}
  <!-- Master switch -->
  <div class="card mt-16">
    <label style="display: flex; align-items: center; gap: 8px; cursor: pointer;">
      <input type="checkbox" bind:checked={config.enabled} />
      <strong>Activar redacción para este proyecto</strong>
    </label>
    <div class="muted" style="font-size: 12px; margin-top: 4px;">
      Si se desactiva, los logs entran tal cual. Las reglas individuales abajo se
      conservan pero no se aplican hasta reactivar el master switch.
    </div>
  </div>

  <!-- Built-ins -->
  <h2 style="font-size: 16px; margin-top: 24px;">Reglas predefinidas</h2>
  <div class="card mt-8">
    {#each builtins as b}
      <label style="display: block; margin: 8px 0; cursor: pointer;">
        <input
          type="checkbox"
          checked={isBuiltinOn(b.slug)}
          on:change={() => toggleBuiltin(b.slug)}
          disabled={!config.enabled}
        />
        <strong style="margin-left: 6px;">{b.label}</strong>
        <span class="muted" style="font-size: 12px; margin-left: 6px;">
          ({b.slug}) — {b.description}
        </span>
      </label>
    {/each}
  </div>

  <!-- Custom -->
  <h2 style="font-size: 16px; margin-top: 24px;">Reglas custom</h2>
  <div class="card mt-8">
    <div class="muted" style="font-size: 12px; margin-bottom: 12px;">
      Regex compatible con el crate <code>regex</code> de Rust (sin lookaround ni
      backreferences). El backend valida cada patrón antes de guardar.
      Si tu replacement contiene <code>$1</code>, <code>$2</code>, … se sustituyen
      por los grupos capturados.
    </div>

    {#if config.custom.length === 0}
      <div class="muted">Sin reglas custom.</div>
    {:else}
      {#each config.custom as rule, i}
        <div class="rule">
          <div class="rule-row">
            <label for={`name-${i}`}>Nombre</label>
            <input id={`name-${i}`} bind:value={rule.name} placeholder="ssn" />
          </div>
          <div class="rule-row">
            <label for={`pat-${i}`}>Patrón</label>
            <input id={`pat-${i}`} class="mono" bind:value={rule.pattern} placeholder="\b\d{3}-\d{2}-\d{4}\b" />
          </div>
          <div class="rule-row">
            <label for={`rep-${i}`}>Reemplazo</label>
            <input id={`rep-${i}`} class="mono" bind:value={rule.replacement} />
          </div>
          <div style="margin-top: 6px;">
            <button type="button" class="danger" on:click={() => removeCustom(i)}>Eliminar</button>
          </div>
        </div>
      {/each}
    {/if}

    <div style="margin-top: 12px;">
      <button type="button" on:click={addCustom} disabled={!config.enabled}>
        + Añadir regla custom
      </button>
    </div>
  </div>

  <div style="margin-top: 24px;">
    <button class="primary" on:click={save} disabled={saving}>
      {saving ? 'Guardando…' : 'Guardar configuración'}
    </button>
  </div>
{/if}

<style>
  .rule {
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 12px;
    margin-bottom: 12px;
  }
  .rule-row {
    display: grid;
    grid-template-columns: 100px 1fr;
    gap: 8px;
    align-items: center;
    margin-bottom: 6px;
  }
  .rule-row label {
    font-size: 12px;
    color: var(--text-muted);
  }
  .rule-row input {
    width: 100%;
  }
</style>
