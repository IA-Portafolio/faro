<script lang="ts">
  import Skeleton from '$lib/components/Skeleton.svelte';
  import {
    fmtCount,
    fmtPct,
    fmtSeconds,
    fmtSecondsRange,
    lookaheadPresets,
    timingMaxPresets,
    windowPresets,
    type StepView
  } from '$lib/funnels';
  import type { DropOffResult, FunnelResult, TimeToConvertResult } from '$lib/api';

  export let result: FunnelResult | null;
  export let loading: boolean;
  export let error: string;
  export let funnelLength: number;
  export let expandedStep: number;
  export let expandedView: StepView;
  export let lookaheadSecs: number;
  export let timingMaxSecs: number;
  export let dropOffByStep: Record<number, DropOffResult>;
  export let dropOffErrors: Record<number, string>;
  export let dropOffLoading: Record<number, boolean>;
  export let timingByStep: Record<number, TimeToConvertResult>;
  export let timingErrors: Record<number, string>;
  export let timingLoading: Record<number, boolean>;

  export let onTogglePanel: (i: number, v: StepView) => void;
  export let onSetLookahead: (s: number) => void;
  export let onSetTimingMax: (s: number) => void;

  $: maxStepUsers = result ? Math.max(1, ...result.steps.map((s) => s.users)) : 1;
</script>

<section class="pane results">
  <div class="results-head">
    <h2 class="pane-title">Resultados</h2>
    {#if result}
      <span class="muted timing mono">
        {result.took_ms} ms · {fmtCount(result.total_entered)} usuarios
      </span>
    {/if}
  </div>

  {#if error}
    <div class="error">{error}</div>
  {:else if funnelLength < 2}
    <div class="muted empty">Agregá al menos 2 pasos para ver la conversión.</div>
  {:else if !result && loading}
    <div class="skel-col">
      {#each Array(funnelLength) as _}
        <Skeleton width="100%" height="36px" radius="6px" />
      {/each}
    </div>
  {:else if result}
    {@const r = result}
    <ul class="bars" class:stale={loading}>
      {#each r.steps as s, i}
        {@const widthPct = (s.users / maxStepUsers) * 100}
        {@const isLastStep = i === r.steps.length - 1}
        {@const droppedAfter = isLastStep ? 0 : s.users - r.steps[i + 1].users}
        {@const droppedPct = s.users > 0 ? droppedAfter / s.users : 0}
        {@const isOpen = expandedStep === i}
        {@const isDropOpen = isOpen && expandedView === 'dropoff'}
        {@const isTimingOpen = isOpen && expandedView === 'timing'}
        {@const dropResult = dropOffByStep[i]}
        {@const dropErr = dropOffErrors[i]}
        {@const dropBusy = dropOffLoading[i]}
        {@const timingResult = timingByStep[i]}
        {@const timingErr = timingErrors[i]}
        {@const timingBusy = timingLoading[i]}
        {@const maxBinUsers = timingResult
          ? Math.max(1, ...timingResult.bins.map((b) => b.users))
          : 1}
        <li class="bar-row">
          <div class="bar-meta">
            <span class="bar-index mono">{i + 1}</span>
            <span class="bar-name">{s.event}</span>
            <span class="bar-users mono">{fmtCount(s.users)}</span>
          </div>
          <div class="bar-track" aria-hidden="true">
            <div class="bar-fill" style="width: {widthPct}%"></div>
          </div>
          <div class="bar-conv mono">
            <span title="Conversión desde el primer paso">{fmtPct(s.conversion_from_start)}</span>
            {#if i > 0}
              <span class="muted" title="Conversión vs paso anterior">
                ↳ {fmtPct(s.conversion_from_prev)}
              </span>
            {/if}
            {#if !isLastStep}
              <span class="insight-tabs" role="tablist" aria-label={`Insights para ${s.event}`}>
                <button
                  type="button"
                  role="tab"
                  class="insight-tab"
                  class:active={isDropOpen}
                  on:click={() => onTogglePanel(i, 'dropoff')}
                  title={`${fmtCount(droppedAfter)} usuarios (${fmtPct(droppedPct)}) no llegaron a ${r.steps[i + 1].event}`}
                  aria-selected={isDropOpen}
                >
                  <span aria-hidden="true">{isDropOpen ? '▾' : '▸'}</span>
                  <span>Drop-off · {fmtCount(droppedAfter)}</span>
                </button>
                <button
                  type="button"
                  role="tab"
                  class="insight-tab"
                  class:active={isTimingOpen}
                  on:click={() => onTogglePanel(i, 'timing')}
                  title={`Tiempo entre ${s.event} y ${r.steps[i + 1].event}`}
                  aria-selected={isTimingOpen}
                >
                  <span aria-hidden="true">{isTimingOpen ? '▾' : '▸'}</span>
                  <span>Tiempo</span>
                </button>
              </span>
            {/if}
          </div>

          {#if !isLastStep && isDropOpen}
            <div class="drop-panel">
              <div class="drop-context muted">
                {fmtCount(droppedAfter)} usuarios vieron <strong>{s.event}</strong>
                y NO llegaron a <strong>{r.steps[i + 1].event}</strong>.
                Eventos en los siguientes
                <select
                  class="lookahead"
                  value={lookaheadSecs}
                  on:change={(e) => onSetLookahead(Number((e.currentTarget as HTMLSelectElement).value))}
                >
                  {#each lookaheadPresets as p}
                    <option value={p.seconds}>{p.label}</option>
                  {/each}
                </select>:
              </div>

              {#if dropErr}
                <div class="error">{dropErr}</div>
              {:else if dropBusy && !dropResult}
                <div class="skel-col">
                  {#each Array(5) as _}
                    <Skeleton width="100%" height="22px" radius="4px" />
                  {/each}
                </div>
              {:else if dropResult}
                {#if dropResult.dropped_users === 0}
                  <div class="muted empty small">
                    No hay usuarios en este cohort en el rango actual.
                  </div>
                {:else if dropResult.top_events.length === 0}
                  <div class="muted empty small">
                    Ningún evento posterior dentro del look-ahead.
                    Probablemente cerraron la pestaña.
                  </div>
                {:else}
                  <ul class="drop-list">
                    {#each dropResult.top_events as ev}
                      <li class="drop-item">
                        <div class="drop-item-bar" style="width: {ev.share * 100}%"></div>
                        <span class="drop-item-name">{ev.event_name}</span>
                        <span class="drop-item-pct mono">{fmtPct(ev.share)}</span>
                        <span class="drop-item-users mono muted">
                          {fmtCount(ev.users)} u · {fmtCount(ev.occurrences)} ev
                        </span>
                      </li>
                    {/each}
                  </ul>
                  <div class="drop-foot muted mono">
                    {dropResult.took_ms} ms · cohort: {fmtCount(dropResult.dropped_users)} usuarios
                  </div>
                {/if}
              {/if}
            </div>
          {/if}

          {#if !isLastStep && isTimingOpen}
            <div class="drop-panel">
              <div class="drop-context muted">
                Tiempo entre <strong>{s.event}</strong> y <strong>{r.steps[i + 1].event}</strong>
                para usuarios que convirtieron en hasta
                <select
                  class="lookahead"
                  value={timingMaxSecs}
                  on:change={(e) => onSetTimingMax(Number((e.currentTarget as HTMLSelectElement).value))}
                >
                  {#each timingMaxPresets as p}
                    <option value={p.seconds}>{p.label}</option>
                  {/each}
                </select>:
              </div>

              {#if timingErr}
                <div class="error">{timingErr}</div>
              {:else if timingBusy && !timingResult}
                <div class="skel-col">
                  {#each Array(6) as _}
                    <Skeleton width="100%" height="18px" radius="4px" />
                  {/each}
                </div>
              {:else if timingResult}
                {@const tr = timingResult}
                {@const convRate = tr.total_with_from > 0
                  ? tr.total_converted / tr.total_with_from
                  : 0}
                {#if tr.total_with_from === 0}
                  <div class="muted empty small">
                    Nadie disparó <strong>{s.event}</strong> en este rango.
                  </div>
                {:else if tr.total_converted === 0}
                  <div class="muted empty small">
                    {fmtCount(tr.total_with_from)} usuarios vieron {s.event}
                    pero ninguno llegó a {r.steps[i + 1].event} dentro del tope.
                  </div>
                {:else}
                  <div class="timing-stats">
                    <div class="stat">
                      <span class="stat-label muted">Conversión</span>
                      <span class="stat-value mono">{fmtPct(convRate)}</span>
                      <span class="stat-sub muted mono">
                        {fmtCount(tr.total_converted)}/{fmtCount(tr.total_with_from)}
                      </span>
                    </div>
                    <div class="stat">
                      <span class="stat-label muted">p50</span>
                      <span class="stat-value mono">{fmtSeconds(tr.p50_seconds)}</span>
                    </div>
                    <div class="stat">
                      <span class="stat-label muted">p90</span>
                      <span class="stat-value mono">{fmtSeconds(tr.p90_seconds)}</span>
                    </div>
                    <div class="stat">
                      <span class="stat-label muted">p99</span>
                      <span class="stat-value mono">{fmtSeconds(tr.p99_seconds)}</span>
                    </div>
                  </div>
                  <ul class="hist">
                    {#each tr.bins as b}
                      {@const heightPct = (b.users / maxBinUsers) * 100}
                      <li class="hist-col" title={`${fmtSecondsRange(b.lower_seconds, b.upper_seconds)}: ${fmtCount(b.users)} usuarios`}>
                        <span class="hist-count mono">{b.users > 0 ? fmtCount(b.users) : ''}</span>
                        <span class="hist-bar" style="height: {heightPct}%" aria-hidden="true"></span>
                        <span class="hist-label mono">{fmtSecondsRange(b.lower_seconds, b.upper_seconds)}</span>
                      </li>
                    {/each}
                  </ul>
                  <div class="drop-foot muted mono">
                    {tr.took_ms} ms · max delta observado: {fmtSeconds(tr.max_seconds_observed)}
                  </div>
                {/if}
              {/if}
            </div>
          {/if}
        </li>
      {/each}
    </ul>
    <div class="muted window-info">
      Ventana de conversión: {windowPresets.find((p) => p.seconds === r.window_seconds)?.label ?? `${r.window_seconds}s`}
    </div>
  {/if}
</section>

<style>
  .results-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }
  .timing { font-size: 11px; }
  .bars {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .bars.stale { opacity: 0.6; }
  .bar-row { display: flex; flex-direction: column; gap: 4px; }
  .bar-meta {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }
  .bar-index {
    width: 18px; height: 18px;
    border-radius: 50%;
    background: var(--bg-hover);
    color: var(--text-muted);
    font-size: 10px;
    font-weight: 600;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .bar-name { flex: 1; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .bar-users { color: var(--text); font-size: 12px; }
  .bar-track {
    height: 10px;
    background: var(--bg-hover);
    border-radius: 5px;
    overflow: hidden;
  }
  .bar-fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-dim));
    transition: width 150ms ease-out;
  }
  .bar-conv {
    display: flex;
    gap: 10px;
    font-size: 11px;
    color: var(--text-muted);
    align-items: center;
    flex-wrap: wrap;
  }
  .window-info { font-size: 11px; margin-top: 8px; }

  /* ----- Insight tabs (Drop-off / Tiempo) ----- */
  .insight-tabs {
    display: inline-flex;
    gap: 4px;
    margin-left: auto;
  }
  .insight-tab {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-muted);
    font-size: 11px;
    padding: 2px 8px;
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
  .insight-tab:hover { color: var(--text); border-color: var(--accent-dim); }
  .insight-tab.active { color: var(--text); background: var(--bg-hover); border-color: var(--accent-dim); }

  .drop-panel {
    margin-top: 8px;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .drop-context { font-size: 11.5px; line-height: 1.5; }
  .drop-context strong { color: var(--text); font-weight: 600; }
  .lookahead {
    margin: 0 4px;
    padding: 1px 4px;
    font-size: 11px;
  }
  .drop-list { list-style: none; padding: 0; margin: 0; display: flex; flex-direction: column; gap: 4px; }
  .drop-item {
    position: relative;
    display: grid;
    grid-template-columns: 1fr auto auto;
    align-items: center;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 4px;
    font-size: 12px;
    isolation: isolate;
  }
  .drop-item-bar {
    position: absolute;
    inset: 0 auto 0 0;
    background: var(--badge-info-bg);
    border-radius: 4px;
    z-index: -1;
    transition: width 150ms ease-out;
  }
  .drop-item-name {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .drop-item-pct { font-size: 12px; }
  .drop-item-users { font-size: 10.5px; }
  .drop-foot { font-size: 10.5px; text-align: right; }

  /* ----- Time-to-convert ----- */
  .timing-stats {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
    padding: 4px 0;
  }
  .stat {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: 6px 8px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 4px;
  }
  .stat-label { font-size: 10px; text-transform: uppercase; letter-spacing: 0.5px; }
  .stat-value { font-size: 13px; color: var(--text); }
  .stat-sub { font-size: 10px; }

  .hist {
    list-style: none;
    padding: 0;
    margin: 8px 0 0;
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    gap: 4px;
    align-items: end;
    min-height: 130px;
  }
  .hist-col {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    height: 100%;
    justify-content: flex-end;
  }
  .hist-count {
    font-size: 9.5px;
    color: var(--text-muted);
    min-height: 12px;
    line-height: 1;
  }
  .hist-bar {
    width: 100%;
    background: linear-gradient(180deg, var(--accent), var(--accent-dim));
    border-radius: 2px 2px 0 0;
    min-height: 1px;
    transition: height 200ms ease-out;
  }
  .hist-label {
    font-size: 9px;
    color: var(--text-muted);
    text-align: center;
    line-height: 1.1;
    word-break: break-all;
    margin-top: 2px;
  }
</style>
