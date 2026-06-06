<script lang="ts">
  /**
   * Pestaña `/settings/appearance` — preferencias personales de visualización.
   *
   * Permite elegir el tema (claro/oscuro/sistema) y el rango temporal por defecto,
   * y los persiste como preferencias del usuario en el backend (`savePreferences`)
   * para que viajen entre dispositivos.
   */
  import { onMount } from 'svelte';
  import {
    fetchPreferences,
    fetchProjects,
    savePreferences,
    type Project,
    type TimeRangePref
  } from '$lib/api';
  import { selectedProject, timeRange, type RangePreset } from '$lib/stores';
  import { themeChoice, resolvedTheme, setTheme, type ThemeChoice } from '$lib/theme';
  import { toast } from '$lib/toasts';

  type Option = { value: ThemeChoice; label: string; icon: string; description: string };

  const themeOptions: Option[] = [
    { value: 'light', label: 'Claro', icon: '☀', description: 'Para entornos iluminados.' },
    { value: 'dark', label: 'Oscuro', icon: '☾', description: 'Reduce la fatiga en sesiones largas.' },
    { value: 'system', label: 'Sistema', icon: '◐', description: 'Sigue tu preferencia del SO.' }
  ];

  const rangeOptions: { value: TimeRangePref; label: string }[] = [
    { value: '5m',  label: 'Últimos 5 minutos' },
    { value: '15m', label: 'Últimos 15 minutos' },
    { value: '1h',  label: 'Última hora' },
    { value: '6h',  label: 'Últimas 6 horas' },
    { value: '24h', label: 'Últimas 24 horas' },
    { value: '7d',  label: 'Últimos 7 días' }
  ];

  let saving = false;
  let savedAt: Date | null = null;
  let saveError = '';

  let projects: Project[] = [];
  let defaultProject = '';
  let defaultRange: TimeRangePref = '1h';
  let initialDefaultProject = '';
  let initialDefaultRange: TimeRangePref = '1h';

  onMount(async () => {
    // Carga proyectos + preferencias en paralelo. Si una falla, la otra
    // sigue funcionando, así el panel no queda inutilizable por un solo
    // endpoint caído.
    const [prefsResult, projectsResult] = await Promise.allSettled([
      fetchPreferences(),
      fetchProjects()
    ]);
    if (prefsResult.status === 'fulfilled') {
      defaultProject = prefsResult.value.default_project ?? '';
      defaultRange = (prefsResult.value.default_time_range ?? '1h') as TimeRangePref;
      initialDefaultProject = defaultProject;
      initialDefaultRange = defaultRange;
    }
    if (projectsResult.status === 'fulfilled') {
      projects = projectsResult.value;
    }
  });

  async function chooseTheme(v: ThemeChoice): Promise<void> {
    if (v === $themeChoice) return;
    saving = true;
    saveError = '';
    try {
      await setTheme(v);
      savedAt = new Date();
      toast.success(`Tema cambiado a ${v === 'system' ? 'automático del sistema' : v === 'dark' ? 'oscuro' : 'claro'}`);
    } catch (e: unknown) {
      saveError = e instanceof Error ? e.message : String(e);
      toast.fromError('No se pudo guardar el tema', e);
    } finally {
      saving = false;
    }
  }

  $: defaultsDirty =
    defaultProject !== initialDefaultProject || defaultRange !== initialDefaultRange;

  async function saveDefaults(): Promise<void> {
    saving = true;
    saveError = '';
    try {
      const prefs = await savePreferences({
        default_project: defaultProject,
        default_time_range: defaultRange
      });
      initialDefaultProject = prefs.default_project;
      initialDefaultRange = prefs.default_time_range;
      savedAt = new Date();
      toast.success('Defaults guardados', {
        description: 'Se aplicarán en la próxima sesión sin ?project= ni ?range=.'
      });
    } catch (e: unknown) {
      saveError = e instanceof Error ? e.message : String(e);
      toast.fromError('No se pudieron guardar los defaults', e);
    } finally {
      saving = false;
    }
  }

  function applyDefaultsToCurrentSession(): void {
    // Útil para "ver el cambio sin esperar al próximo login".
    selectedProject.set(defaultProject);
    timeRange.set(defaultRange as RangePreset);
  }
</script>

<div class="page-header">
  <h1 class="page-title">Apariencia y defaults</h1>
</div>

<p class="muted page-lede">
  Estas preferencias se guardan en tu cuenta y se aplican automáticamente al
  iniciar sesión en cualquier dispositivo. Un enlace con <code>?project=…</code>
  o <code>?range=…</code> siempre gana sobre el default para que los deep links
  no se vean alterados.
</p>

<section class="card">
  <header class="card-head">
    <h2>Tema</h2>
    <div class="muted small">
      Actualmente <strong>{$resolvedTheme === 'dark' ? 'oscuro' : 'claro'}</strong>{#if $themeChoice === 'system'} (según tu sistema){/if}.
    </div>
  </header>

  <div role="radiogroup" aria-label="Selección de tema" class="theme-grid">
    {#each themeOptions as opt}
      <button
        type="button"
        role="radio"
        aria-checked={$themeChoice === opt.value}
        on:click={() => chooseTheme(opt.value)}
        disabled={saving}
        class="theme-card"
        class:active={$themeChoice === opt.value}
      >
        <div class="theme-card-head">
          <span class="theme-card-icon mono">{opt.icon}</span>
          <strong>{opt.label}</strong>
          {#if $themeChoice === opt.value}
            <span class="badge ok" style="margin-left: auto;">Activo</span>
          {/if}
        </div>
        <div class="muted small theme-card-desc">{opt.description}</div>
      </button>
    {/each}
  </div>
</section>

<section class="card">
  <header class="card-head">
    <h2>Defaults de exploración</h2>
    <div class="muted small">
      Cuando entres a Faro sin parámetros en la URL, el panel arrancará con
      estos valores. Cambios dentro de la app sólo se persisten al pulsar "Guardar".
    </div>
  </header>

  <div class="defaults-grid">
    <div class="field">
      <label for="default-project">Proyecto por defecto</label>
      <select id="default-project" bind:value={defaultProject}>
        <option value="">Todos los proyectos</option>
        {#each projects as p}
          <option value={p.slug}>{p.name} ({p.slug})</option>
        {/each}
      </select>
    </div>

    <div class="field">
      <label for="default-range">Rango temporal por defecto</label>
      <select id="default-range" bind:value={defaultRange}>
        {#each rangeOptions as o}
          <option value={o.value}>{o.label}</option>
        {/each}
      </select>
    </div>
  </div>

  <div class="flex gap-8 center" style="margin-top: 14px;">
    <button class="primary" on:click={saveDefaults} disabled={!defaultsDirty || saving}>
      {saving ? 'Guardando…' : 'Guardar defaults'}
    </button>
    <button on:click={applyDefaultsToCurrentSession} disabled={saving} title="Aplica los valores al panel sin esperar al próximo login">
      Aplicar a esta sesión
    </button>
    {#if defaultsDirty}
      <span class="muted small">· cambios sin guardar</span>
    {/if}
  </div>
</section>

{#if saveError}
  <div class="error-msg">{saveError}</div>
{:else if savedAt}
  <div class="muted small saved-msg">
    ✓ Guardado a las {savedAt.toLocaleTimeString()}.
  </div>
{/if}

<style>
  /* Constraint global del contenido — los lectores cansan a >900px de prosa. */
  .page-lede { max-width: 760px; margin-bottom: 20px; }

  .card {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 20px;
  }
  .card + .card { margin-top: 16px; }
  .card-head {
    display: flex;
    align-items: baseline;
    gap: 12px;
    flex-wrap: wrap;
    margin-bottom: 16px;
  }
  .card-head h2 { margin: 0; font-size: 15px; font-weight: 600; }
  .small { font-size: 12px; }

  .theme-grid {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 10px;
  }
  @media (max-width: 720px) {
    .theme-grid { grid-template-columns: 1fr; }
  }

  .theme-card {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 12px 14px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 6px;
    text-align: left;
    transition: border-color 0.1s, background 0.1s;
  }
  .theme-card:hover { background: var(--bg-hover); }
  .theme-card.active {
    border-color: var(--accent);
    box-shadow: inset 0 0 0 1px var(--accent);
  }
  .theme-card-head {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .theme-card-icon { font-size: 16px; width: 20px; }
  .theme-card-desc { line-height: 1.35; }

  .defaults-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px 16px;
    max-width: 720px;
  }
  @media (max-width: 600px) {
    .defaults-grid { grid-template-columns: 1fr; }
  }
  /* El field global probablemente trae margin-bottom por defecto; en el grid
     ya hay gap, evitamos doble espaciado. */
  .defaults-grid .field { margin-bottom: 0; }

  .error-msg { color: var(--danger); margin-top: 12px; }
  .saved-msg { margin-top: 12px; }
</style>
