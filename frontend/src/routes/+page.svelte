<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { fetchDashboard, fetchServices, fetchLogStats, type Dashboard, type Service } from '$lib/api';
  import { timeRange, rangeMinutes, formatTimestamp, selectedProject } from '$lib/stores';
  import TimeRangePicker from '$lib/components/TimeRangePicker.svelte';
  import Chart from '$lib/components/Chart.svelte';
  import OnboardingEmpty from '$lib/components/OnboardingEmpty.svelte';
  import Skeleton from '$lib/components/Skeleton.svelte';
  import SkeletonCards from '$lib/components/SkeletonCards.svelte';
  import SkeletonTable from '$lib/components/SkeletonTable.svelte';

  let summary: Dashboard | null = null;
  let services: Service[] = [];
  let series: { ts: string; value: number }[] = [];
  let loading = true;
  let error = '';
  let intervalId: ReturnType<typeof setInterval>;

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      const m = rangeMinutes($timeRange);
      const project = $selectedProject || undefined;
      const [s, svc, stats] = await Promise.all([
        fetchDashboard({ last_minutes: m, project }),
        fetchServices({ last_minutes: m, project }),
        fetchLogStats({ last_minutes: m, bucket_seconds: 60, project })
      ]);
      summary = s;
      services = svc;
      const byTs: Record<string, number> = {};
      for (const row of stats) {
        byTs[row.ts] = (byTs[row.ts] ?? 0) + row.count;
      }
      series = Object.entries(byTs)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([ts, value]) => ({ ts, value }));
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    void load();
    intervalId = setInterval(load, 15000);
  });
  onDestroy(() => clearInterval(intervalId));
  $: $timeRange, $selectedProject, load();
</script>

<div class="page-header">
  <h1 class="page-title">Resumen</h1>
  <TimeRangePicker />
</div>

{#if error}<div style="color: var(--danger); padding: 12px; background: #2a1010; border-radius: 4px;">Error: {error}</div>{/if}

{#if !loading && summary && summary.log_count === 0 && summary.service_count === 0}
  <OnboardingEmpty kind="summary" />
{/if}

{#if loading && !summary}
  <SkeletonCards count={7} />
{:else}
  <div class="cards">
    <div class="card"><div class="label">Logs</div><div class="value">{summary?.log_count?.toLocaleString() ?? '–'}</div></div>
    <div class="card"><div class="label">Errores</div><div class="value" style="color: var(--danger);">{summary?.error_count?.toLocaleString() ?? '–'}</div></div>
    <div class="card"><div class="label">Servicios</div><div class="value">{summary?.service_count ?? '–'}</div></div>
    <div class="card"><div class="label">Trazas</div><div class="value">{summary?.trace_count?.toLocaleString() ?? '–'}</div></div>
    <div class="card"><div class="label">Problemas abiertos</div><div class="value" style="color: var(--warn);">{summary?.open_issue_count ?? '–'}</div></div>
    <div class="card"><div class="label">Alertas activas</div><div class="value" style="color: var(--danger);">{summary?.firing_incident_count ?? '–'}</div></div>
    <div class="card"><div class="label">Monitores caídos</div><div class="value" style="color: var(--danger);">{summary?.monitors_down ?? 0}/{summary?.monitors_total ?? 0}</div></div>
  </div>
{/if}

<div class="card">
  {#if loading && series.length === 0}
    <Skeleton width="100%" height="220px" radius="4px" />
  {:else}
    <Chart points={series} label="Logs por minuto" height={220} />
  {/if}
</div>

<h2 style="font-size: 16px; font-weight: 600; margin: 24px 0 12px;">Servicios</h2>
<div style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead>
      <tr><th>Servicio</th><th>Logs</th><th>Errores</th><th>Visto por última vez</th></tr>
    </thead>
    <tbody>
      {#if loading && services.length === 0}
        <SkeletonTable rows={6} cols={4} widths={['40%', '12%', '12%', '28%']} />
      {/if}
      {#each services as s}
        <tr>
          <td><a href="/logs?service={encodeURIComponent(s.service_name)}">{s.service_name}</a></td>
          <td class="tabular">{s.log_count.toLocaleString()}</td>
          <td class="tabular" style:color={s.error_count > 0 ? 'var(--danger)' : 'inherit'}>{s.error_count.toLocaleString()}</td>
          <td class="muted mono">{formatTimestamp(s.last_seen)}</td>
        </tr>
      {/each}
      {#if !loading && services.length === 0}
        <tr><td colspan="4" class="empty">
          {#if $selectedProject}
            Sin actividad para este proyecto en el rango seleccionado.
          {:else}
            Aún no hay datos. Crea un <a href="/settings/projects">proyecto</a> y envía logs con el SDK.
          {/if}
        </td></tr>
      {/if}
    </tbody>
  </table>
</div>
