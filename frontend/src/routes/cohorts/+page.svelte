<script lang="ts">
  /**
   * Cohorts: segmentación de usuarios sobre `faro.product_events`.
   *
   * Layout 3-paneles:
   *   - Izquierda: lista de cohorts guardados + "Nuevo".
   *   - Centro: builder de la definition (event_name, op, count, last_days, filtros)
   *     con "Preview" en vivo (debounce 250ms) y "Guardar".
   *   - Derecha: detalle del cohort seleccionado — tamaño actual, sample,
   *     retención (curva), overlap con otro cohort.
   *
   * Race conditions: `previewSeq` y `detailSeq` descartan respuestas viejas
   * cuando el usuario edita rápido o cambia de cohort durante un fetch.
   */
  import { onMount } from 'svelte';
  import {
    createCohort,
    deleteCohort,
    fetchCohortOverlap,
    fetchCohortRetention,
    fetchCohortUsers,
    listCohorts,
    parseCohortDefinition,
    previewCohort,
    updateCohort,
    type Cohort,
    type CohortDefinition,
    type CohortFilter,
    type CohortOverlap,
    type CohortPreview,
    type CohortRetention
  } from '$lib/api';
  import { selectedProject } from '$lib/stores';
  import { toast } from '$lib/toasts';
  import Skeleton from '$lib/components/Skeleton.svelte';

  // ---------- Lista de cohorts ----------
  let cohorts: Cohort[] = [];
  let listLoading = true;
  let listError = '';

  async function loadList(): Promise<void> {
    listLoading = true;
    listError = '';
    try {
      cohorts = await listCohorts({ project: $selectedProject || undefined });
    } catch (e: unknown) {
      listError = e instanceof Error ? e.message : String(e);
      cohorts = [];
    } finally {
      listLoading = false;
    }
  }

  // ---------- Builder ----------
  /** Cohort en edición. `null` => modo creación. */
  let editing: Cohort | null = null;

  // Form fields.
  let formName = '';
  let formDescription = '';
  let formEvent = 'checkout_completed';
  let formOp: CohortDefinition['op'] = '>=';
  let formCount = 3;
  let formLastDays = 30;
  let formFilters: CohortFilter[] = [];
  let formError = '';

  function loadEditing(c: Cohort): void {
    editing = c;
    formName = c.name;
    formDescription = c.description;
    const def = parseCohortDefinition(c.definition);
    if (def) {
      formEvent = def.event;
      formOp = def.op;
      formCount = def.count;
      formLastDays = def.last_days;
      formFilters = (def.filters ?? []).map((f) => ({ ...f }));
    } else {
      formError = 'definition guardada inválida — corregí y guardá de nuevo.';
    }
    void schedulePreview();
    void loadDetail(c);
  }

  function newCohort(): void {
    editing = null;
    formName = '';
    formDescription = '';
    formEvent = 'checkout_completed';
    formOp = '>=';
    formCount = 3;
    formLastDays = 30;
    formFilters = [];
    formError = '';
    detail = null;
    detailError = '';
    retention = null;
    retentionError = '';
    overlap = null;
    overlapError = '';
    overlapAgainstId = '';
    void schedulePreview();
  }

  function addFilter(): void {
    if (formFilters.length >= 3) return;
    formFilters = [...formFilters, { key: '', value: '' }];
  }
  function removeFilter(i: number): void {
    formFilters = formFilters.filter((_, j) => j !== i);
    void schedulePreview();
  }

  function buildDefinition(): CohortDefinition {
    return {
      event: formEvent.trim(),
      op: formOp,
      count: Math.max(1, Math.floor(formCount || 0)),
      last_days: Math.max(1, Math.floor(formLastDays || 0)),
      filters: formFilters.filter((f) => f.key.trim() && f.value.trim())
    };
  }

  // ---------- Preview en vivo ----------
  let preview: CohortPreview | null = null;
  let previewBusy = false;
  let previewError = '';
  let previewSeq = 0;
  let previewTimer: ReturnType<typeof setTimeout> | null = null;

  async function schedulePreview(): Promise<void> {
    if (previewTimer) clearTimeout(previewTimer);
    if (!formEvent.trim() || !formCount || !formLastDays) {
      preview = null;
      previewError = '';
      return;
    }
    previewTimer = setTimeout(runPreview, 250);
  }

  async function runPreview(): Promise<void> {
    const seq = ++previewSeq;
    previewBusy = true;
    previewError = '';
    try {
      const r = await previewCohort({
        project: $selectedProject || undefined,
        definition: buildDefinition(),
        sample_limit: 20
      });
      if (seq !== previewSeq) return;
      preview = r;
    } catch (e: unknown) {
      if (seq !== previewSeq) return;
      previewError = e instanceof Error ? e.message : String(e);
      preview = null;
    } finally {
      if (seq === previewSeq) previewBusy = false;
    }
  }

  // ---------- Guardar / eliminar ----------
  let saving = false;
  async function saveCohort(): Promise<void> {
    formError = '';
    const def = buildDefinition();
    if (!formName.trim()) {
      formError = 'name es obligatorio';
      return;
    }
    if (!def.event) {
      formError = 'event es obligatorio';
      return;
    }
    saving = true;
    try {
      const payload = {
        name: formName.trim(),
        description: formDescription,
        project: $selectedProject || 'default',
        definition: def
      };
      const saved = editing
        ? await updateCohort(editing.id, payload)
        : await createCohort(payload);
      toast.success(editing ? 'Cohort actualizado' : 'Cohort creado');
      await loadList();
      // Volvé a poner el guardado como editing para que el detail muestre size/retention/overlap.
      const fresh = cohorts.find((c) => c.id === saved.id) ?? saved;
      loadEditing(fresh);
    } catch (e: unknown) {
      formError = e instanceof Error ? e.message : String(e);
    } finally {
      saving = false;
    }
  }

  async function removeCohort(): Promise<void> {
    if (!editing) return;
    if (!window.confirm(`¿Eliminar el cohort "${editing.name}"?`)) return;
    try {
      await deleteCohort(editing.id);
      toast.success('Cohort eliminado');
      await loadList();
      newCohort();
    } catch (e: unknown) {
      toast.error(e instanceof Error ? e.message : String(e));
    }
  }

  // ---------- Detalle del cohort seleccionado ----------
  let detail: CohortPreview | null = null;
  let detailBusy = false;
  let detailError = '';
  let detailSeq = 0;

  let retention: CohortRetention | null = null;
  let retentionBusy = false;
  let retentionError = '';
  let retentionHorizon = 30;

  let overlap: CohortOverlap | null = null;
  let overlapBusy = false;
  let overlapError = '';
  let overlapAgainstId = '';

  async function loadDetail(c: Cohort): Promise<void> {
    const seq = ++detailSeq;
    detailBusy = true;
    detailError = '';
    detail = null;
    retention = null;
    retentionError = '';
    overlap = null;
    overlapError = '';
    try {
      const [users, ret] = await Promise.all([
        fetchCohortUsers(c.id, { limit: 50 }),
        fetchCohortRetention(c.id, { horizon_days: retentionHorizon })
      ]);
      if (seq !== detailSeq) return;
      detail = users;
      retention = ret;
    } catch (e: unknown) {
      if (seq !== detailSeq) return;
      detailError = e instanceof Error ? e.message : String(e);
    } finally {
      if (seq === detailSeq) detailBusy = false;
    }
  }

  async function reloadRetention(): Promise<void> {
    if (!editing) return;
    retentionBusy = true;
    retentionError = '';
    try {
      retention = await fetchCohortRetention(editing.id, { horizon_days: retentionHorizon });
    } catch (e: unknown) {
      retentionError = e instanceof Error ? e.message : String(e);
    } finally {
      retentionBusy = false;
    }
  }

  async function computeOverlap(): Promise<void> {
    if (!editing || !overlapAgainstId) return;
    overlapBusy = true;
    overlapError = '';
    overlap = null;
    try {
      overlap = await fetchCohortOverlap(editing.id, overlapAgainstId);
    } catch (e: unknown) {
      overlapError = e instanceof Error ? e.message : String(e);
    } finally {
      overlapBusy = false;
    }
  }

  // ---------- Reactividad ----------
  let prevProject = $selectedProject;
  $: if (prevProject !== $selectedProject) {
    prevProject = $selectedProject;
    void loadList();
    newCohort();
  }

  // Recompute preview cuando cambien los inputs del builder.
  $: formEvent, formOp, formCount, formLastDays, formFilters, void schedulePreview();

  onMount(loadList);

  // ---------- Helpers ----------
  function fmtCount(n: number): string {
    if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
    return n.toLocaleString();
  }
  function fmtPct(p: number): string {
    if (!isFinite(p)) return '–';
    return `${(p * 100).toFixed(1)}%`;
  }
  function describe(c: Cohort): string {
    const def = parseCohortDefinition(c.definition);
    if (!def) return 'definition inválida';
    const f = def.filters && def.filters.length > 0
      ? ` · ${def.filters.length} filtro${def.filters.length === 1 ? '' : 's'}`
      : '';
    return `${def.event} ${def.op} ${def.count} en ${def.last_days}d${f}`;
  }

  // ---------- Histogramita de retención ----------
  $: retentionMax = retention
    ? Math.max(1, ...retention.points.map((p) => p.active_users))
    : 1;

  /** Map de bucket día_back → punto, para rellenar huecos en el render. */
  $: retentionByDay = (() => {
    const m = new Map<number, number>();
    if (!retention) return m;
    for (const p of retention.points) m.set(p.day_back, p.active_users);
    return m;
  })();

  $: retentionDaysToRender = retention
    ? Array.from({ length: retention.horizon_days + 1 }, (_, i) => i).reverse()
    : [];

  $: otherCohorts = cohorts.filter((c) => editing && c.id !== editing.id);
  $: overlapAgainst = overlapAgainstId
    ? cohorts.find((c) => c.id === overlapAgainstId) ?? null
    : null;
</script>

<div class="page-header">
  <h1 class="page-title">Cohorts</h1>
  <button on:click={newCohort} class="primary">+ Nuevo cohort</button>
</div>

<p class="muted hint">
  Definí un cohort como "usuarios que dispararon <em>evento X</em> <em>op N</em> veces
  en los últimos <em>D días</em>", opcionalmente filtrando por properties. Una vez
  guardado podés ver la retención hacia atrás y el overlap con otros cohorts.
</p>

<div class="layout">
  <!-- ===== Lista ===== -->
  <aside class="pane list" aria-label="Cohorts guardados">
    <h2 class="pane-title">Guardados</h2>
    {#if listLoading}
      <div class="skel-col">
        {#each Array(4) as _}
          <Skeleton width="100%" height="44px" radius="6px" />
        {/each}
      </div>
    {:else if listError}
      <div class="error">{listError}</div>
    {:else if cohorts.length === 0}
      <div class="muted empty small">No hay cohorts guardados todavía.</div>
    {:else}
      <ul class="cohort-list">
        {#each cohorts as c (c.id)}
          <li>
            <button
              type="button"
              class="cohort-item"
              class:active={editing?.id === c.id}
              on:click={() => loadEditing(c)}
              title={describe(c)}
            >
              <span class="cohort-name">{c.name}</span>
              <span class="cohort-desc muted">{describe(c)}</span>
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </aside>

  <!-- ===== Builder ===== -->
  <section class="pane builder">
    <div class="pane-head">
      <h2 class="pane-title">{editing ? 'Editar' : 'Nuevo cohort'}</h2>
      {#if previewBusy}
        <span class="muted" style="font-size: 11px;"><span class="spinner"></span> calculando…</span>
      {/if}
    </div>

    <div class="field">
      <label for="cohort-name">Nombre</label>
      <input id="cohort-name" bind:value={formName} placeholder="Power users (checkout x3)" />
    </div>
    <div class="field">
      <label for="cohort-desc">Descripción <span class="muted">(opcional)</span></label>
      <input id="cohort-desc" bind:value={formDescription} placeholder="Usuarios con alto engagement de checkout" />
    </div>

    <div class="def-row">
      <div class="field grow">
        <label for="cohort-event">Evento</label>
        <input
          id="cohort-event"
          class="mono"
          bind:value={formEvent}
          placeholder="checkout_completed"
        />
      </div>
      <div class="field narrow">
        <label for="cohort-op">Op</label>
        <select id="cohort-op" bind:value={formOp}>
          <option value="==">==</option>
          <option value=">=">≥</option>
          <option value=">">&gt;</option>
          <option value="<=">≤</option>
          <option value="<">&lt;</option>
        </select>
      </div>
      <div class="field narrow">
        <label for="cohort-count">Veces</label>
        <input id="cohort-count" type="number" min="1" bind:value={formCount} />
      </div>
      <div class="field narrow">
        <label for="cohort-days">Últimos N días</label>
        <input id="cohort-days" type="number" min="1" max="365" bind:value={formLastDays} />
      </div>
    </div>

    <div class="filters">
      <div class="filters-head">
        <span>Filtros sobre properties <span class="muted">(opcional, máx 3)</span></span>
        <button type="button" class="ghost small" on:click={addFilter} disabled={formFilters.length >= 3}>
          + Añadir
        </button>
      </div>
      {#if formFilters.length === 0}
        <div class="muted empty small">
          Sin filtros — el cohort solo cuenta por event_name.
        </div>
      {:else}
        {#each formFilters as f, i (i)}
          <div class="filter-row">
            <input class="mono" placeholder="properties.key" bind:value={f.key} on:change={schedulePreview} />
            <span class="muted">=</span>
            <input class="mono" placeholder="value" bind:value={f.value} on:change={schedulePreview} />
            <button type="button" class="ghost icon" on:click={() => removeFilter(i)} title="Quitar">×</button>
          </div>
        {/each}
      {/if}
    </div>

    {#if formError}<div class="error">{formError}</div>{/if}

    <div class="actions">
      <button on:click={saveCohort} class="primary" disabled={saving}>
        {saving ? 'Guardando…' : editing ? 'Actualizar' : 'Crear cohort'}
      </button>
      {#if editing}
        <button on:click={removeCohort} class="danger ghost">Eliminar</button>
      {/if}
    </div>

    <!-- Preview en vivo -->
    <div class="preview-box">
      <h3 class="block-title">Tamaño estimado</h3>
      {#if previewError}
        <div class="error">{previewError}</div>
      {:else if preview}
        <div class="preview-stats">
          <div class="big-number mono">{fmtCount(preview.size)}</div>
          <div class="muted preview-meta">
            usuarios distintos · {preview.took_ms} ms
          </div>
        </div>
        {#if preview.sample.length > 0}
          <details class="preview-sample">
            <summary class="muted">Sample de {preview.sample.length} distinct_id</summary>
            <ul class="sample-list mono">
              {#each preview.sample as id}
                <li>{id}</li>
              {/each}
            </ul>
          </details>
        {/if}
      {:else}
        <div class="muted empty small">Completá los campos para ver el preview.</div>
      {/if}
    </div>
  </section>

  <!-- ===== Detalle: retención + overlap ===== -->
  <section class="pane detail">
    <h2 class="pane-title">Detalle</h2>
    {#if !editing}
      <div class="muted empty">
        Seleccioná un cohort de la lista o guardá uno nuevo para ver retención y overlap.
      </div>
    {:else if detailError}
      <div class="error">{detailError}</div>
    {:else}
      <!-- Tamaño actual -->
      <div class="block">
        <h3 class="block-title">Miembros actuales</h3>
        {#if detailBusy && !detail}
          <Skeleton width="100%" height="48px" radius="6px" />
        {:else if detail}
          <div class="preview-stats">
            <div class="big-number mono">{fmtCount(detail.size)}</div>
            <div class="muted preview-meta">usuarios distintos · {detail.took_ms} ms</div>
          </div>
        {/if}
      </div>

      <!-- Retención -->
      <div class="block">
        <div class="block-head">
          <h3 class="block-title">Retención</h3>
          <label class="muted" style="font-size: 11px;">
            Horizonte
            <select
              bind:value={retentionHorizon}
              on:change={reloadRetention}
              style="margin-left: 4px;"
            >
              <option value={7}>7d</option>
              <option value={14}>14d</option>
              <option value={30}>30d</option>
              <option value={60}>60d</option>
              <option value={90}>90d</option>
            </select>
          </label>
        </div>
        {#if retentionBusy && !retention}
          <Skeleton width="100%" height="120px" radius="6px" />
        {:else if retentionError}
          <div class="error">{retentionError}</div>
        {:else if retention}
          <div class="muted small" style="margin-bottom: 6px;">
            % del cohort ({fmtCount(retention.cohort_size)} usuarios) con al menos un evento ese día.
            <span class="mono">{retention.took_ms} ms</span>
          </div>
          {#if retention.cohort_size === 0}
            <div class="muted empty small">El cohort está vacío.</div>
          {:else}
            <ul class="ret-list">
              {#each retentionDaysToRender as d}
                {@const active = retentionByDay.get(d) ?? 0}
                {@const pct = retention.cohort_size > 0 ? active / retention.cohort_size : 0}
                {@const widthPct = (active / retentionMax) * 100}
                <li class="ret-row">
                  <span class="ret-day mono">{d === 0 ? 'hoy' : `-${d}d`}</span>
                  <div class="ret-track">
                    <div class="ret-fill" style="width: {widthPct}%"></div>
                  </div>
                  <span class="ret-val mono">{fmtPct(pct)}</span>
                  <span class="ret-users muted mono">{fmtCount(active)}</span>
                </li>
              {/each}
            </ul>
          {/if}
        {/if}
      </div>

      <!-- Overlap -->
      <div class="block">
        <h3 class="block-title">Overlap</h3>
        {#if otherCohorts.length === 0}
          <div class="muted empty small">
            Necesitás al menos otro cohort guardado para comparar.
          </div>
        {:else}
          <div class="overlap-controls">
            <select bind:value={overlapAgainstId}>
              <option value="">Comparar con…</option>
              {#each otherCohorts as c}
                <option value={c.id}>{c.name}</option>
              {/each}
            </select>
            <button
              on:click={computeOverlap}
              disabled={!overlapAgainstId || overlapBusy}
            >
              {overlapBusy ? 'Calculando…' : 'Comparar'}
            </button>
          </div>
          {#if overlapError}<div class="error">{overlapError}</div>{/if}
          {#if overlap}
            <div class="overlap-grid">
              <div class="overlap-cell">
                <span class="overlap-label muted">{editing.name}</span>
                <span class="overlap-value mono">{fmtCount(overlap.size_a)}</span>
              </div>
              <div class="overlap-cell highlight">
                <span class="overlap-label muted">Intersección</span>
                <span class="overlap-value mono">{fmtCount(overlap.intersection)}</span>
                <span class="overlap-sub muted mono">Jaccard {fmtPct(overlap.jaccard)}</span>
              </div>
              <div class="overlap-cell">
                <span class="overlap-label muted">{overlapAgainst?.name ?? 'otro'}</span>
                <span class="overlap-value mono">{fmtCount(overlap.size_b)}</span>
              </div>
            </div>
            <div class="muted small overlap-foot mono">{overlap.took_ms} ms</div>
          {/if}
        {/if}
      </div>
    {/if}
  </section>
</div>

<style>
  .hint { margin-bottom: 16px; max-width: 760px; }

  .layout {
    display: grid;
    grid-template-columns: 240px 1fr 1fr;
    gap: 16px;
    align-items: start;
    min-height: 60vh;
  }
  @media (max-width: 1100px) {
    .layout { grid-template-columns: 1fr; }
  }

  .pane {
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }
  .pane-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
  }
  .pane-title {
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin: 0;
  }
  .block-title {
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    color: var(--text-muted);
    margin: 0 0 6px;
  }
  .block { display: flex; flex-direction: column; gap: 6px; }
  .block-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  /* ----- Lista ----- */
  .cohort-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 60vh;
    overflow-y: auto;
  }
  .cohort-item {
    width: 100%;
    text-align: left;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 8px 10px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 2px;
    color: var(--text);
  }
  .cohort-item:hover { background: var(--bg-hover); border-color: var(--accent-dim); }
  .cohort-item.active {
    background: var(--bg-hover);
    border-color: var(--accent);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  .cohort-name { font-size: 13px; font-weight: 600; }
  .cohort-desc {
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-family: "JetBrains Mono", Menlo, monospace;
  }

  /* ----- Builder ----- */
  .field { display: flex; flex-direction: column; gap: 4px; }
  .field label { font-size: 11px; color: var(--text-muted); }
  .field input, .field select { width: 100%; }
  .def-row {
    display: grid;
    grid-template-columns: 1fr 70px 90px 110px;
    gap: 8px;
  }
  .def-row .narrow input, .def-row .narrow select { text-align: right; }

  .filters {
    display: flex;
    flex-direction: column;
    gap: 6px;
    background: var(--bg);
    padding: 8px;
    border-radius: 6px;
    border: 1px solid var(--border);
  }
  .filters-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 12px;
  }
  .filter-row {
    display: grid;
    grid-template-columns: 1fr 14px 1fr auto;
    gap: 6px;
    align-items: center;
  }
  .ghost {
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-muted);
    font-size: 12px;
    padding: 4px 10px;
  }
  .ghost.small { font-size: 11px; padding: 2px 8px; }
  .ghost.icon { padding: 0 6px; font-size: 15px; }
  .ghost:hover { color: var(--text); }
  .ghost.danger:hover { color: var(--danger); border-color: var(--danger); }

  .actions {
    display: flex;
    gap: 8px;
    margin-top: 4px;
  }
  .preview-box {
    margin-top: 8px;
    padding: 10px 12px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
  }
  .preview-stats { display: flex; align-items: baseline; gap: 10px; }
  .big-number { font-size: 28px; font-weight: 700; color: var(--text); }
  .preview-meta { font-size: 11px; }
  .preview-sample { margin-top: 8px; }
  .preview-sample summary { cursor: pointer; font-size: 11.5px; }
  .sample-list {
    list-style: none;
    padding: 6px 0 0;
    margin: 0;
    max-height: 140px;
    overflow-y: auto;
    font-size: 11px;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  /* ----- Retención ----- */
  .ret-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .ret-row {
    display: grid;
    grid-template-columns: 48px 1fr 56px 64px;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
  }
  .ret-day { color: var(--text-muted); text-align: right; padding-right: 4px; }
  .ret-track {
    height: 8px;
    background: var(--bg);
    border-radius: 4px;
    overflow: hidden;
  }
  .ret-fill {
    height: 100%;
    background: linear-gradient(90deg, var(--accent), var(--accent-dim));
    transition: width 150ms ease-out;
  }
  .ret-val { text-align: right; font-size: 11.5px; }
  .ret-users { text-align: right; font-size: 10.5px; }

  /* ----- Overlap ----- */
  .overlap-controls {
    display: flex;
    gap: 6px;
    align-items: center;
  }
  .overlap-controls select { flex: 1; }
  .overlap-grid {
    display: grid;
    grid-template-columns: 1fr 1.2fr 1fr;
    gap: 8px;
    margin-top: 8px;
  }
  .overlap-cell {
    padding: 10px;
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    display: flex;
    flex-direction: column;
    gap: 2px;
    text-align: center;
  }
  .overlap-cell.highlight {
    border-color: var(--accent);
    background: rgba(250, 204, 21, 0.06);
  }
  .overlap-label { font-size: 10.5px; }
  .overlap-value { font-size: 20px; font-weight: 700; }
  .overlap-sub { font-size: 10.5px; }
  .overlap-foot { margin-top: 6px; text-align: right; font-size: 10.5px; }

  /* ----- Genéricos ----- */
  .error {
    color: var(--danger);
    background: var(--badge-error-bg);
    border: 1px solid var(--danger);
    padding: 6px 10px;
    border-radius: 6px;
    font-size: 12px;
  }
  .empty { padding: 16px 6px; text-align: center; font-size: 12px; }
  .empty.small { padding: 8px 6px; }
  .small { font-size: 11.5px; }
  .skel-col { display: flex; flex-direction: column; gap: 6px; }
</style>
