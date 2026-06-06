<script lang="ts">
  /**
   * Pestaña `/settings/projects` — gestión de proyectos.
   *
   * CRUD de proyectos (cada uno identificado por su `slug`), incluida la rotación
   * del token de ingesta (`rotateProjectToken`) y los snippets de instalación por
   * SDK (`$lib/sdk-snippets`). Un proyecto aísla los datos y tiene su propio token.
   */
  import { onMount } from 'svelte';
  import {
    fetchProjects,
    createProject,
    updateProject,
    deleteProject,
    rotateProjectToken,
    type Project
  } from '$lib/api';
  import { groupLabels, groupOrder, snippetsFor } from '$lib/sdk-snippets';
  import { formatTimestamp, selectedProject } from '$lib/stores';
  import { toast } from '$lib/toasts';
  import SkeletonTable from '$lib/components/SkeletonTable.svelte';

  let projects: Project[] = [];
  let creating = false;
  let editing: Project | null = null;
  let detail: Project | null = null;
  let revealed: Record<string, boolean> = {};
  let error = '';
  let loading = true;

  // Estado del formulario del modal de creación / edición
  let formName = '';
  let formSlug = '';
  let formDescription = '';

  async function load(): Promise<void> {
    loading = true;
    error = '';
    try {
      projects = await fetchProjects();
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);

  function openNew(): void {
    creating = true;
    editing = null;
    formName = '';
    formSlug = '';
    formDescription = '';
  }

  function openEdit(p: Project): void {
    creating = false;
    editing = p;
    formName = p.name;
    formSlug = p.slug;
    formDescription = p.description;
  }

  async function save(): Promise<void> {
    error = '';
    try {
      if (creating) {
        const created = await createProject({
          name: formName,
          slug: formSlug || undefined,
          description: formDescription
        });
        detail = created; // open the "DSN" panel right away so user can copy the token
        await load();
        toast.success(`Proyecto "${created.name}" creado`, {
          description: `slug: ${created.slug}`
        });
      } else if (editing) {
        await updateProject(editing.slug, { name: formName, description: formDescription });
        await load();
        toast.success('Proyecto actualizado');
      }
      creating = false;
      editing = null;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      toast.fromError('No se pudo guardar el proyecto', e);
    }
  }

  async function remove(slug: string): Promise<void> {
    if (!confirm(`¿Eliminar el proyecto "${slug}"? La ingesta con su token quedará bloqueada inmediatamente.`)) return;
    try {
      await deleteProject(slug);
      if ($selectedProject === slug) selectedProject.set('');
      await load();
      toast.success(`Proyecto "${slug}" eliminado`, {
        description: 'Las peticiones con su token quedarán bloqueadas.'
      });
    } catch (e: unknown) {
      toast.fromError('No se pudo eliminar el proyecto', e);
    }
  }

  async function rotate(slug: string): Promise<void> {
    if (!confirm(`Rotar el token del proyecto "${slug}"? El token anterior dejará de funcionar.`)) return;
    try {
      const updated = await rotateProjectToken(slug);
      detail = updated;
      revealed[updated.slug] = true;
      await load();
      toast.success(`Token de "${slug}" rotado`, {
        description: 'El token anterior ya no autentica. Actualiza tus SDKs.',
        duration: 8000
      });
    } catch (e: unknown) {
      toast.fromError('No se pudo rotar el token', e);
    }
  }

  async function copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
      toast.success('Copiado al portapapeles');
    } catch {
      toast.error('No se pudo acceder al portapapeles');
    }
  }

  // El catálogo de snippets vive en `$lib/sdk-snippets` para que el empty
  // state de onboarding muestre exactamente los mismos bloques sin duplicar
  // ninguna plantilla. El alias mantiene el call site del template intacto.
  const snippets = snippetsFor;

  let activeTab = 'node';
</script>

<div class="page-header">
  <h1 class="page-title">Proyectos</h1>
  <button class="primary" on:click={openNew}>+ Nuevo proyecto</button>
</div>

<p class="muted" style="max-width: 720px;">
  Cada proyecto agrupa varios servicios y tiene su propio token de ingesta. El SDK envía
  los logs, trazas y métricas a Faro autenticando con el token; los datos quedan ligados
  al proyecto y aparecen filtrados al seleccionarlo en la barra lateral.
</p>

{#if error}<div style="color: var(--danger); margin-top: 12px;">{error}</div>{/if}

<div class="mt-16" style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; overflow: hidden;">
  <table>
    <thead>
      <tr>
        <th>Nombre</th>
        <th>Slug</th>
        <th>Token de ingesta</th>
        <th>Creado</th>
        <th></th>
      </tr>
    </thead>
    <tbody>
      {#if loading && projects.length === 0}
        <SkeletonTable rows={4} cols={5} widths={['28%', '14%', '24%', '14%', '20%']} />
      {/if}
      {#each projects as p}
        <tr>
          <td>
            <strong>{p.name}</strong>
            <div class="muted" style="font-size: 12px;">{p.description}</div>
          </td>
          <td class="mono">{p.slug}</td>
          <td class="mono">
            {#if revealed[p.slug]}
              <span style="user-select: all;">{p.ingest_token}</span>
              <button on:click={() => copy(p.ingest_token)} style="margin-left: 6px; padding: 2px 8px;">Copiar</button>
              <button on:click={() => (revealed[p.slug] = false)} style="padding: 2px 8px;">Ocultar</button>
            {:else}
              <span class="muted">••••••••</span>
              <button on:click={() => (revealed[p.slug] = true)} style="margin-left: 6px; padding: 2px 8px;">Mostrar</button>
            {/if}
          </td>
          <td class="muted mono">{formatTimestamp(p.created_at)}</td>
          <td>
            <button on:click={() => (detail = p)}>SDK</button>
            <a href="/settings/projects/{p.slug}/redaction" data-sveltekit-preload-data="hover">
              <button type="button">Redacción PII</button>
            </a>
            <a href="/settings/projects/{p.slug}/origins" data-sveltekit-preload-data="hover">
              <button type="button">Orígenes RUM</button>
            </a>
            <button on:click={() => openEdit(p)}>Editar</button>
            <button on:click={() => rotate(p.slug)}>Rotar token</button>
            <button class="danger" on:click={() => remove(p.slug)}>Eliminar</button>
          </td>
        </tr>
      {/each}
      {#if !loading && projects.length === 0}
        <tr><td colspan="5" class="empty">
          Todavía no hay proyectos. Crea el primero para empezar a ingerir.
        </td></tr>
      {/if}
    </tbody>
  </table>
</div>

{#if creating || editing}
  <div class="drawer">
    <button class="close" on:click={() => { creating = false; editing = null; }}>Cerrar</button>
    <h2 style="margin-top: 0;">{creating ? 'Nuevo proyecto' : `Editar ${editing?.name ?? ''}`}</h2>

    <div class="field">
      <label>Nombre</label>
      <input bind:value={formName} placeholder="Mi App" />
    </div>
    {#if creating}
      <div class="field">
        <label>Slug (opcional, se genera del nombre si lo dejas vacío)</label>
        <input bind:value={formSlug} class="mono" placeholder="mi-app" />
      </div>
    {:else}
      <div class="field">
        <label>Slug</label>
        <input value={formSlug} class="mono" disabled />
      </div>
    {/if}
    <div class="field">
      <label>Descripción</label>
      <textarea bind:value={formDescription} rows="3"></textarea>
    </div>

    <button class="primary" on:click={save}>{creating ? 'Crear' : 'Guardar'}</button>
  </div>
{/if}

{#if detail}
  <div class="drawer">
    <button class="close" on:click={() => (detail = null)}>Cerrar</button>
    <h2 style="margin-top: 0;">SDK · {detail.name}</h2>
    <div class="muted" style="margin-bottom: 16px;">
      Elige tu lenguaje. El token autentica la ingesta; los datos quedan automáticamente
      ligados al proyecto <strong>{detail.slug}</strong>.
    </div>

    <div class="field">
      <label>Token de ingesta</label>
      <div class="flex gap-8">
        <input value={detail.ingest_token} readonly class="mono" style="flex: 1;" />
        <button on:click={() => copy(detail!.ingest_token)}>Copiar</button>
      </div>
    </div>

    {#each [snippets(detail)] as snipList}
      <div class="sdk-tabs">
        {#each groupOrder as g}
          {@const items = snipList.filter((s) => s.group === g)}
          {#if items.length > 0}
            <div class="sdk-tab-group">
              <span class="sdk-tab-group-label">{groupLabels[g]}</span>
              <div class="sdk-tab-row">
                {#each items as s}
                  <button
                    on:click={() => (activeTab = s.id)}
                    class:primary={activeTab === s.id}
                    class="sdk-tab"
                  >{s.label}</button>
                {/each}
              </div>
            </div>
          {/if}
        {/each}
      </div>

      {#each snipList as s}
        {#if activeTab === s.id}
          <div class="muted mono" style="font-size: 12px; margin-bottom: 6px;">$ {s.install}</div>
          <pre>{s.code}</pre>
          <button on:click={() => copy(s.code)} style="margin-top: 8px;">Copiar código</button>
        {/if}
      {/each}
    {/each}
  </div>
{/if}

<style>
  .sdk-tabs {
    display: flex;
    flex-direction: column;
    gap: 10px;
    border-bottom: 1px solid var(--border);
    margin: 16px 0 12px;
    padding-bottom: 8px;
  }
  .sdk-tab-group {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }
  .sdk-tab-group-label {
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.6px;
    color: var(--text-muted);
    min-width: 64px;
  }
  .sdk-tab-row {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }
  .sdk-tab {
    border-radius: 4px;
    padding: 6px 12px;
    font-size: 12.5px;
  }
</style>
