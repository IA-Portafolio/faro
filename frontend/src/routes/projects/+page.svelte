<script lang="ts">
  import { onMount } from 'svelte';
  import {
    fetchProjects,
    createProject,
    updateProject,
    deleteProject,
    rotateProjectToken,
    apiBase,
    type Project
  } from '$lib/api';
  import { formatTimestamp, selectedProject } from '$lib/stores';

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
      } else if (editing) {
        await updateProject(editing.slug, { name: formName, description: formDescription });
        await load();
      }
      creating = false;
      editing = null;
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function remove(slug: string): Promise<void> {
    if (!confirm(`¿Eliminar el proyecto "${slug}"? La ingesta con su token quedará bloqueada inmediatamente.`)) return;
    await deleteProject(slug);
    if ($selectedProject === slug) selectedProject.set('');
    await load();
  }

  async function rotate(slug: string): Promise<void> {
    if (!confirm(`Rotar el token del proyecto "${slug}"? El token anterior dejará de funcionar.`)) return;
    const updated = await rotateProjectToken(slug);
    detail = updated;
    revealed[updated.slug] = true;
    await load();
  }

  async function copy(text: string): Promise<void> {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      /* ignore */
    }
  }

  type Snippet = { id: string; label: string; install: string; code: string };

  function snippets(p: Project): Snippet[] {
    const base = apiBase();
    const t = p.ingest_token;
    return [
      {
        id: 'node',
        label: 'Node.js',
        install: 'npm install @iaportafolio/node',
        code: `import * as faro from '@iaportafolio/node';

faro.init({
  endpoint: '${base}',
  token: process.env.FARO_TOKEN!,           // ${t.slice(0, 6)}…${t.slice(-4)}
  service: 'mi-servicio',
  environment: 'production',
});

faro.info('arranque ok', { port: 8080 });

try {
  await charge(order);
} catch (err) {
  faro.captureException(err, { tags: { order_id: order.id } });
  throw err;
}`,
      },
      {
        id: 'nextjs',
        label: 'Next.js',
        install: 'npm install @iaportafolio/nextjs @iaportafolio/node',
        code: `// instrumentation.ts
export async function register() {
  const { registerFaro } = await import('@iaportafolio/nextjs/server');
  registerFaro({
    endpoint: '${base}',
    token: process.env.FARO_TOKEN!,
    service: 'mi-next-app',
    environment: process.env.NODE_ENV,
  });
}

// app/faro-client.tsx
'use client';
import { useEffect } from 'react';
import { initFaroClient } from '@iaportafolio/nextjs/client';

export function FaroClient() {
  useEffect(() => {
    initFaroClient({
      endpoint: '${base}',
      token: '${t}',
      service: 'mi-next-app-web',
    });
  }, []);
  return null;
}`,
      },
      {
        id: 'python',
        label: 'Python',
        install: 'pip install faro-sdk',
        code: `import faro_sdk as faro

faro.init(
    endpoint='${base}',
    token='${t}',
    service='mi-servicio',
    environment='production',
)

faro.info('arranque ok', port=8080)

try:
    procesar(archivo)
except Exception as exc:
    faro.capture_exception(exc, tags={'archivo': archivo.name})
    raise`,
      },
      {
        id: 'go',
        label: 'Go',
        install: 'go get github.com/iaportafolio/faro-go',
        code: `import faro "github.com/iaportafolio/faro-go"

faro.Init(faro.Options{
    Endpoint:    "${base}",
    Token:       "${t}",
    Service:     "mi-servicio",
    Environment: "production",
})
defer faro.Close(context.Background())

faro.Info("arranque ok", map[string]any{"port": 8080})

if err := charge(order); err != nil {
    faro.CaptureException(err, map[string]string{"order_id": order.ID})
}`,
      },
      {
        id: 'flutter',
        label: 'Flutter',
        install: 'flutter pub add faro_sdk',
        code: `import 'package:faro_sdk/faro_sdk.dart';

Faro.run(
  options: const FaroOptions(
    endpoint: '${base}',
    token: '${t}',
    service: 'mi-app-mobile',
    environment: 'production',
  ),
  appRunner: () => runApp(const MyApp()),
);

// más adelante…
Faro.instance.info('login ok', {'user_id': 42});

try { await pagar(); } catch (e, st) {
  Faro.instance.captureException(e, stack: st, tags: {'flow': 'checkout'});
  rethrow;
}`,
      },
      {
        id: 'kotlin',
        label: 'Kotlin / Android',
        install: 'implementation("com.iaportafolio:faro:0.1.0")',
        code: `import com.iaportafolio.faro.Faro
import com.iaportafolio.faro.FaroOptions

Faro.init(FaroOptions(
    endpoint = "${base}",
    token = "${t}",
    service = "android-app",
    environment = "production",
    release = BuildConfig.VERSION_NAME,
))

Faro.info("login ok", mapOf("user_id" to user.id))

try { pay() } catch (e: Throwable) {
    Faro.captureException(e, mapOf("flow" to "checkout"))
    throw e
}`,
      },
      {
        id: 'expo',
        label: 'Expo / React Native',
        install: 'npx expo install @iaportafolio/expo',
        code: `import * as faro from '@iaportafolio/expo';

faro.init({
  endpoint: '${base}',
  token: process.env.EXPO_PUBLIC_FARO_TOKEN!,
  service: 'mi-app-mobile',
  environment: __DEV__ ? 'dev' : 'production',
});

faro.info('app montada');

try {
  await pagar();
} catch (err) {
  faro.captureException(err, { tags: { flow: 'checkout' } });
}`,
      },
      {
        id: 'otlp',
        label: 'OpenTelemetry',
        install: '# usa el OTel SDK oficial de tu lenguaje',
        code: `# Configura tu OTel SDK con estas variables:
export OTEL_EXPORTER_OTLP_ENDPOINT=${base}
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer ${t}"
export OTEL_SERVICE_NAME=mi-servicio

# Logs van a /v1/logs, trazas a /v1/traces, métricas a /v1/metrics.`,
      },
      {
        id: 'curl',
        label: 'curl',
        install: '# zero install',
        code: `curl -X POST ${base}/api/v1/ingest/logs \\
  -H "Authorization: Bearer ${t}" \\
  -H "Content-Type: application/json" \\
  -d '{
    "service": "mi-servicio",
    "logs": [
      { "level": "INFO", "message": "hola desde ${p.slug}" }
    ]
  }'`,
      },
    ];
  }

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
      <div style="display: flex; gap: 4px; flex-wrap: wrap; border-bottom: 1px solid var(--border); margin: 16px 0 12px; padding-bottom: 0;">
        {#each snipList as s}
          <button
            on:click={() => (activeTab = s.id)}
            class:primary={activeTab === s.id}
            style="border-radius: 4px 4px 0 0; padding: 6px 12px; font-size: 12.5px;"
          >{s.label}</button>
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
