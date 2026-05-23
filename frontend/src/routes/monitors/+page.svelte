<script lang="ts">
  import { onMount } from 'svelte';
  import {
    fetchMonitors,
    fetchMonitorUptime,
    createMonitor,
    updateMonitor,
    deleteMonitor,
    fetchProjects,
    type Monitor,
    type Project
  } from '$lib/api';
  import { selectedProject } from '$lib/stores';

  let monitors: Monitor[] = [];
  let projects: Project[] = [];
  let uptime: Record<string, { uptime_pct: number; avg_duration_ms: number; total: number }> = {};
  // El formulario de edición replica Monitor más un campo `project` (slug) para el flujo de creación.
  let editing: (Partial<Monitor> & { project?: string }) | null = null;
  let error = '';

  async function load(): Promise<void> {
    try {
      [monitors, projects] = await Promise.all([
        fetchMonitors({ project: $selectedProject || undefined }),
        fetchProjects()
      ]);
      for (const m of monitors) {
        try {
          const u = await fetchMonitorUptime(m.id, { last_minutes: 60 });
          uptime[m.id] = u;
        } catch (_e) {
          // ignore
        }
      }
      uptime = uptime;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }
  onMount(load);
  $: $selectedProject, load();

  function newMonitor(): void {
    editing = {
      name: '',
      method: 'GET',
      url: '',
      headers: {},
      body: '',
      interval_seconds: 60,
      timeout_seconds: 30,
      expected_status_min: 200,
      expected_status_max: 299,
      expected_body_regex: '',
      enabled: 1,
      project: $selectedProject || (projects[0]?.slug ?? 'default')
    };
  }

  async function save(): Promise<void> {
    if (!editing) return;
    error = '';
    try {
      if (editing.id) {
        await updateMonitor(editing.id, editing);
      } else {
        await createMonitor(editing);
      }
      editing = null;
      await load();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function remove(id: string): Promise<void> {
    if (!confirm('¿Eliminar este monitor?')) return;
    await deleteMonitor(id);
    await load();
  }
</script>

<div class="page-header">
  <h1 class="page-title">Monitores de API</h1>
  <button class="primary" on:click={newMonitor}>+ Nuevo monitor</button>
</div>

{#if error}<div style="color: var(--danger);">{error}</div>{/if}

<div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead>
      <tr><th>Nombre</th><th>Endpoint</th><th>Intervalo</th><th>Uptime 1h</th><th>Latencia media</th><th>Estado</th><th></th></tr>
    </thead>
    <tbody>
      {#each monitors as m}
        <tr>
          <td><strong>{m.name}</strong></td>
          <td class="mono" style="max-width: 320px; overflow: hidden; text-overflow: ellipsis;">{m.method} {m.url}</td>
          <td class="tabular">{m.interval_seconds}s</td>
          <td class="tabular" style:color={uptime[m.id]?.uptime_pct < 99 ? 'var(--danger)' : 'var(--success)'}>
            {uptime[m.id] ? uptime[m.id].uptime_pct.toFixed(2) + '%' : '–'}
          </td>
          <td class="tabular">{uptime[m.id] ? uptime[m.id].avg_duration_ms.toFixed(0) + 'ms' : '–'}</td>
          <td><span class="badge {m.enabled ? 'ok' : 'debug'}">{m.enabled ? 'ACTIVO' : 'INACTIVO'}</span></td>
          <td>
            <button on:click={() => (editing = { ...m })}>Editar</button>
            <button class="danger" on:click={() => remove(m.id)}>Eliminar</button>
          </td>
        </tr>
      {/each}
      {#if monitors.length === 0}
        <tr><td colspan="7" class="empty">Sin monitores configurados.</td></tr>
      {/if}
    </tbody>
  </table>
</div>

{#if editing}
  <div class="drawer">
    <button class="close" on:click={() => (editing = null)}>Cerrar</button>
    <h2 style="margin-top: 0;">{editing.id ? 'Editar monitor' : 'Nuevo monitor'}</h2>

    {#if !editing.id}
      <div class="field">
        <label>Proyecto</label>
        <select bind:value={editing.project}>
          {#each projects as p}
            <option value={p.slug}>{p.name}</option>
          {/each}
        </select>
      </div>
    {/if}
    <div class="field"><label>Nombre</label><input bind:value={editing.name} /></div>
    <div class="flex gap-8">
      <div class="field" style="width: 110px;"><label>Método</label>
        <select bind:value={editing.method}>
          <option>GET</option><option>POST</option><option>PUT</option>
          <option>DELETE</option><option>HEAD</option><option>PATCH</option>
        </select>
      </div>
      <div class="field grow"><label>URL</label><input bind:value={editing.url} class="mono" /></div>
    </div>
    <div class="flex gap-8">
      <div class="field"><label>Intervalo (s)</label><input type="number" bind:value={editing.interval_seconds} /></div>
      <div class="field"><label>Timeout (s)</label><input type="number" bind:value={editing.timeout_seconds} /></div>
      <div class="field"><label>Status mín.</label><input type="number" bind:value={editing.expected_status_min} /></div>
      <div class="field"><label>Status máx.</label><input type="number" bind:value={editing.expected_status_max} /></div>
    </div>
    <div class="field"><label>Regex del cuerpo (opcional)</label><input bind:value={editing.expected_body_regex} class="mono" placeholder=".*ok.*" /></div>
    <div class="field"><label>Cuerpo</label><textarea bind:value={editing.body} rows="4" class="mono"></textarea></div>
    <div class="field"><label><input type="checkbox" checked={editing.enabled === 1} on:change={(e) => (editing.enabled = (e.currentTarget).checked ? 1 : 0)} /> Activo</label></div>

    <button class="primary" on:click={save}>Guardar</button>
  </div>
{/if}
