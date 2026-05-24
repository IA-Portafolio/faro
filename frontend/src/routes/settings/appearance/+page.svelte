<script lang="ts">
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
    { value: 'light', label: 'Claro', icon: '☀', description: 'Pensado para entornos bien iluminados.' },
    { value: 'dark', label: 'Oscuro', icon: '☾', description: 'Reduce la fatiga ocular en sesiones largas o nocturnas.' },
    { value: 'system', label: 'Sistema', icon: '◐', description: 'Sigue automáticamente la preferencia de tu sistema operativo.' }
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

<p class="muted" style="max-width: 720px; margin-bottom: 20px;">
  Estas preferencias se guardan en tu cuenta y se aplican automáticamente al
  iniciar sesión en cualquier dispositivo. Un enlace con <code>?project=…</code>
  o <code>?range=…</code> siempre gana sobre el default para que los deep links
  no se vean alterados.
</p>

<section style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; padding: 20px; max-width: 720px;">
  <h2 style="margin: 0 0 6px; font-size: 16px;">Tema</h2>
  <div class="muted" style="font-size: 12px; margin-bottom: 16px;">
    Actualmente se aplica <strong>{$resolvedTheme === 'dark' ? 'oscuro' : 'claro'}</strong>
    {#if $themeChoice === 'system'}(según tu sistema){/if}.
  </div>

  <div role="radiogroup" aria-label="Selección de tema" style="display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: 10px;">
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
        <div class="muted" style="font-size: 12px; text-align: left;">{opt.description}</div>
      </button>
    {/each}
  </div>
</section>

<section style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; padding: 20px; max-width: 720px; margin-top: 16px;">
  <h2 style="margin: 0 0 6px; font-size: 16px;">Defaults de exploración</h2>
  <div class="muted" style="font-size: 12px; margin-bottom: 16px;">
    Cuando entres a Faro sin parámetros en la URL, el panel arrancará con
    estos valores. Si cambias el proyecto o el rango dentro de la app, el
    cambio queda reflejado en el query string (no en estos defaults) hasta
    que pulses "Guardar".
  </div>

  <div class="field" style="max-width: 360px;">
    <label for="default-project">Proyecto por defecto</label>
    <select id="default-project" bind:value={defaultProject}>
      <option value="">Todos los proyectos</option>
      {#each projects as p}
        <option value={p.slug}>{p.name} ({p.slug})</option>
      {/each}
    </select>
  </div>

  <div class="field" style="max-width: 360px;">
    <label for="default-range">Rango temporal por defecto</label>
    <select id="default-range" bind:value={defaultRange}>
      {#each rangeOptions as o}
        <option value={o.value}>{o.label}</option>
      {/each}
    </select>
  </div>

  <div class="flex gap-8 center">
    <button class="primary" on:click={saveDefaults} disabled={!defaultsDirty || saving}>
      {saving ? 'Guardando…' : 'Guardar defaults'}
    </button>
    <button on:click={applyDefaultsToCurrentSession} disabled={saving} title="Aplica los valores al panel sin esperar al próximo login">
      Aplicar a esta sesión
    </button>
    {#if defaultsDirty}
      <span class="muted" style="font-size: 12px;">· cambios sin guardar</span>
    {/if}
  </div>
</section>

{#if saveError}
  <div style="color: var(--danger); margin-top: 12px; max-width: 720px;">{saveError}</div>
{:else if savedAt}
  <div class="muted" style="margin-top: 12px; font-size: 12px; max-width: 720px;">
    ✓ Guardado a las {savedAt.toLocaleTimeString()}.
  </div>
{/if}

<style>
  .theme-card {
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 14px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 8px;
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
  .theme-card-icon {
    font-size: 16px;
    width: 20px;
  }
</style>
