<script lang="ts">
  /**
   * Pestaña `/settings/alerts` — reglas de alerta e incidentes.
   *
   * CRUD de reglas de alerta (`AlertRule`) con sus destinatarios (`targets`), más
   * la lista de incidentes recientes (`fetchAlertIncidents`). Las reglas evalúan
   * métricas/monitores y, al dispararse, notifican por los canales configurados en
   * Integraciones.
   */
  import { onMount } from 'svelte';
  import {
    fetchAlertRules,
    fetchAlertIncidents,
    createAlertRule,
    updateAlertRule,
    deleteAlertRule,
    fetchProjects,
    type AlertRule,
    type AlertIncident,
    type Project
  } from '$lib/api';
  import { formatTimestamp, selectedProject } from '$lib/stores';
  import { toast } from '$lib/toasts';
  import SkeletonTable from '$lib/components/SkeletonTable.svelte';

  let rules: AlertRule[] = [];
  let incidents: AlertIncident[] = [];
  let projects: Project[] = [];
  let editing: (Partial<AlertRule> & { project?: string }) | null = null;
  let targetsText = '';
  let error = '';
  let loading = true;

  async function load(): Promise<void> {
    loading = true;
    try {
      [rules, incidents, projects] = await Promise.all([
        fetchAlertRules({ project: $selectedProject || undefined }),
        fetchAlertIncidents({ last_minutes: 1440, project: $selectedProject || undefined }),
        fetchProjects()
      ]);
    } finally {
      loading = false;
    }
  }
  onMount(load);
  $: $selectedProject, load();

  function newRule(): void {
    editing = {
      name: '',
      description: '',
      source: 'logs',
      query: "(SELECT countIf(severity_number >= 17) FROM faro.logs WHERE timestamp > now() - INTERVAL :window_seconds SECOND)",
      condition: 'gt',
      threshold: 10,
      window_seconds: 300,
      interval_seconds: 60,
      severity: 'warn',
      notification_targets: [],
      enabled: 1,
      project: $selectedProject || (projects[0]?.slug ?? 'default')
    };
    targetsText = '';
  }

  function editRule(r: AlertRule): void {
    editing = { ...r };
    targetsText = (r.notification_targets || []).join('\n');
  }

  async function save(): Promise<void> {
    if (!editing) return;
    error = '';
    editing.notification_targets = targetsText
      .split('\n')
      .map((s) => s.trim())
      .filter(Boolean);
    const wasEdit = !!editing.id;
    const name = editing.name ?? '';
    try {
      if (editing.id) {
        await updateAlertRule(editing.id, editing);
      } else {
        await createAlertRule(editing);
      }
      editing = null;
      await load();
      toast.success(wasEdit ? `Regla "${name}" actualizada` : `Regla "${name}" creada`);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      toast.fromError(wasEdit ? 'No se pudo actualizar la regla' : 'No se pudo crear la regla', e);
    }
  }

  async function remove(id: string): Promise<void> {
    const rule = rules.find((r) => r.id === id);
    if (!confirm(`¿Eliminar la regla "${rule?.name ?? id}"?`)) return;
    try {
      await deleteAlertRule(id);
      await load();
      toast.success(`Regla "${rule?.name ?? id}" eliminada`);
    } catch (e: unknown) {
      toast.fromError('No se pudo eliminar la regla', e);
    }
  }

  $: firing = incidents.filter((i) => i.status === 'firing');

  const sevLabel: Record<string, string> = {
    info: 'info', warn: 'aviso', error: 'error', critical: 'crítico'
  };
</script>

<div class="page-header">
  <h1 class="page-title">Alertas</h1>
  <button class="primary" on:click={newRule}>+ Nueva regla</button>
</div>

{#if error}<div style="color: var(--danger);">{error}</div>{/if}

{#if firing.length > 0}
  <h2 style="font-size: 16px; margin-top: 8px;">Activas ({firing.length})</h2>
  <div style="background: var(--bg-elev); border: 1px solid var(--danger); border-radius: 6px; overflow: hidden; margin-bottom: 16px;">
    <table>
      <thead><tr><th>Regla</th><th>Severidad</th><th>Valor</th><th>Umbral</th><th>Desde</th></tr></thead>
      <tbody>
        {#each firing as i}
          <tr>
            <td><strong>{i.rule_name}</strong></td>
            <td><span class="badge {i.severity}">{sevLabel[i.severity] ?? i.severity}</span></td>
            <td class="tabular">{i.value.toFixed(2)}</td>
            <td class="tabular">{i.threshold.toFixed(2)}</td>
            <td class="muted mono">{formatTimestamp(i.started_at)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </div>
{/if}

<h2 style="font-size: 16px;">Reglas</h2>
<div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead><tr><th>Nombre</th><th>Fuente</th><th>Condición</th><th>Severidad</th><th>Intervalo</th><th>Estado</th><th></th></tr></thead>
    <tbody>
      {#if loading && rules.length === 0}
        <SkeletonTable rows={4} cols={7} widths={['28%', '10%', '14%', '12%', '10%', '10%', '16%']} />
      {/if}
      {#each rules as r}
        <tr>
          <td>
            <strong>{r.name}</strong>
            <div class="muted" style="max-width: 480px;">{r.description}</div>
          </td>
          <td>{r.source}</td>
          <td class="mono">{r.condition} {r.threshold}</td>
          <td><span class="badge {r.severity}">{sevLabel[r.severity] ?? r.severity}</span></td>
          <td class="tabular">{r.interval_seconds}s</td>
          <td><span class="badge {r.enabled ? 'ok' : 'debug'}">{r.enabled ? 'ON' : 'OFF'}</span></td>
          <td>
            <button on:click={() => editRule(r)}>Editar</button>
            <button class="danger" on:click={() => remove(r.id)}>Eliminar</button>
          </td>
        </tr>
      {/each}
      {#if rules.length === 0}
        <tr><td colspan="7" class="empty">Sin reglas de alerta.</td></tr>
      {/if}
    </tbody>
  </table>
</div>

<h2 style="font-size: 16px; margin-top: 24px;">Incidentes recientes</h2>
<div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead><tr><th>Regla</th><th>Severidad</th><th>Estado</th><th>Valor</th><th>Inicio</th><th>Resuelto</th></tr></thead>
    <tbody>
      {#if loading && incidents.length === 0}
        <SkeletonTable rows={3} cols={6} widths={['28%', '12%', '12%', '14%', '18%', '16%']} />
      {/if}
      {#each incidents as i}
        <tr>
          <td>{i.rule_name}</td>
          <td><span class="badge {i.severity}">{sevLabel[i.severity] ?? i.severity}</span></td>
          <td><span class="badge {i.status}">{i.status === 'firing' ? 'activa' : 'resuelta'}</span></td>
          <td class="tabular">{i.value.toFixed(2)} / {i.threshold.toFixed(2)}</td>
          <td class="muted mono">{formatTimestamp(i.started_at)}</td>
          <td class="muted mono">{i.resolved_at ? formatTimestamp(i.resolved_at) : ''}</td>
        </tr>
      {/each}
      {#if incidents.length === 0}
        <tr><td colspan="6" class="empty">Sin incidentes en las últimas 24 h.</td></tr>
      {/if}
    </tbody>
  </table>
</div>

{#if editing}
  <div class="drawer">
    <button class="close" on:click={() => (editing = null)}>Cerrar</button>
    <h2 style="margin-top: 0;">{editing.id ? 'Editar regla' : 'Nueva regla de alerta'}</h2>

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
    <div class="field"><label>Descripción</label><input bind:value={editing.description} /></div>
    <div class="flex gap-8">
      <div class="field" style="width: 140px;"><label>Fuente</label>
        <select bind:value={editing.source}>
          <option>logs</option><option>spans</option><option>metrics</option><option>monitors</option>
        </select>
      </div>
      <div class="field" style="width: 140px;"><label>Severidad</label>
        <select bind:value={editing.severity}>
          <option value="info">info</option><option value="warn">aviso</option>
          <option value="error">error</option><option value="critical">crítico</option>
        </select>
      </div>
    </div>
    <div class="field"><label>Consulta SQL (devuelve un Float64; usa :window_seconds para el rango)</label>
      <textarea bind:value={editing.query} rows="4" class="mono"></textarea>
    </div>
    <div class="flex gap-8">
      <div class="field" style="width: 110px;"><label>Condición</label>
        <select bind:value={editing.condition}>
          <option value="gt">&gt;</option><option value="gte">&gt;=</option>
          <option value="lt">&lt;</option><option value="lte">&lt;=</option>
          <option value="eq">=</option>
        </select>
      </div>
      <div class="field grow"><label>Umbral</label><input type="number" bind:value={editing.threshold} step="any" /></div>
    </div>
    <div class="flex gap-8">
      <div class="field"><label>Ventana (s)</label><input type="number" bind:value={editing.window_seconds} /></div>
      <div class="field"><label>Intervalo eval (s)</label><input type="number" bind:value={editing.interval_seconds} /></div>
    </div>
    <div class="field"><label>Destinos de notificación (uno por línea)</label>
      <textarea bind:value={targetsText} rows="4" class="mono" placeholder={'https://discord.com/api/webhooks/...\ntg://-1001234567890\ntg://@mi_canal\ntg://-1001234567890@123456:ABC-bot-token'}></textarea>
      <small class="muted">
        Acepta <code>https://</code> (webhook JSON tipo Slack/Discord) y <code>tg://&lt;chat_id&gt;</code> para Telegram nativo.
        El bot se configura en <a href="/settings/integrations">Integraciones</a>; también puedes incluir un token por destino con <code>tg://&lt;chat_id&gt;@&lt;token&gt;</code>.
      </small>
    </div>
    <div class="field"><label><input type="checkbox" checked={editing.enabled === 1} on:change={(e) => (editing!.enabled = (e.currentTarget).checked ? 1 : 0)} /> Activa</label></div>

    <button class="primary" on:click={save}>Guardar</button>
  </div>
{/if}
