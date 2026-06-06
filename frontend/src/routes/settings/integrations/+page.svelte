<script lang="ts">
  /**
   * Pestaña `/settings/integrations` — canales de notificación.
   *
   * Configura la integración con Telegram (bot token, chat por defecto, envío de
   * prueba) y el CRUD de canales de notificación (`NotificationChannel`) que luego
   * usan las reglas de alerta para avisar cuando se dispara un incidente.
   */
  import { onMount } from 'svelte';
  import {
    fetchTelegramIntegration,
    saveTelegramIntegration,
    deleteTelegramIntegration,
    testTelegramIntegration,
    listChannels,
    createChannel,
    updateChannel,
    deleteChannel,
    testChannel,
    type TelegramIntegration,
    type NotificationChannel,
    type ChannelKind,
    type ChannelInput
  } from '$lib/api';
  import { formatTimestamp } from '$lib/stores';
  import { toast } from '$lib/toasts';

  let tg: TelegramIntegration | null = null;
  let loading = true;
  let saving = false;
  let testing = false;
  let removing = false;

  let botTokenInput = '';
  let defaultChatId = '';
  let enabled = true;
  let testChatId = '';
  let testText = '';
  let testFeedback: { kind: 'ok' | 'err'; message: string } | null = null;
  let saveError = '';

  // -------- Canales de notificación (multi-instancia) --------
  type FormField = {
    key: string;
    label: string;
    type: 'text' | 'password' | 'textarea' | 'json' | 'checkbox';
    placeholder?: string;
    help?: string;
  };

  const KIND_LABEL: Record<ChannelKind, string> = {
    webhook: 'Webhook genérico',
    slack: 'Slack',
    discord: 'Discord',
    pagerduty: 'PagerDuty',
    opsgenie: 'OpsGenie',
    email_resend: 'Email (Resend)',
    telegram: 'Telegram (por canal)'
  };

  // Esquema de campos por kind. Los campos `password` se devuelven enmascarados
  // por el backend; al editar, dejarlos vacíos conserva el valor previo.
  const KIND_FIELDS: Record<ChannelKind, FormField[]> = {
    webhook: [
      { key: 'url', label: 'URL', type: 'password', placeholder: 'https://...', help: 'POST JSON. Compatible con Slack/Discord/custom.' },
      { key: 'body_template', label: 'Body template JSON (opcional)', type: 'textarea', help: 'Vacío = body estructurado por defecto. Placeholders: {rule_name} {severity} {status} {value} {threshold} {project_id} {text}.' },
      { key: 'headers', label: 'Headers extra (JSON)', type: 'json', help: 'Ej.: {"Authorization":"Bearer ..."}' }
    ],
    slack: [
      { key: 'webhook_url', label: 'Incoming Webhook URL', type: 'password', placeholder: 'https://hooks.slack.com/services/...' },
      { key: 'channel', label: 'Canal (opcional)', type: 'text', placeholder: '#alerts' },
      { key: 'username', label: 'Username (opcional)', type: 'text', placeholder: 'Faro' }
    ],
    discord: [
      { key: 'webhook_url', label: 'Webhook URL', type: 'password', placeholder: 'https://discord.com/api/webhooks/...' },
      { key: 'username', label: 'Username (opcional)', type: 'text' },
      { key: 'avatar_url', label: 'Avatar URL (opcional)', type: 'text' }
    ],
    pagerduty: [
      { key: 'integration_key', label: 'Integration Key (Events API v2)', type: 'password', help: 'En PagerDuty: Service → Integrations → Events API v2 → Integration Key.' }
    ],
    opsgenie: [
      { key: 'api_key', label: 'API Key', type: 'password', help: 'Settings → API key management → Add new API key (con permiso "Create and Update Access").' },
      { key: 'api_base', label: 'API base (opcional)', type: 'text', placeholder: 'https://api.opsgenie.com', help: 'Para cuentas EU: https://api.eu.opsgenie.com.' },
      { key: 'responders', label: 'Responders (JSON array)', type: 'json', help: 'Ej.: ["team-ops","user@example.com"]. Si vacío, se usa el routing del account.' },
      { key: 'tags', label: 'Tags (JSON array)', type: 'json', help: 'Ej.: ["faro","prod"].' }
    ],
    email_resend: [
      { key: 'api_key', label: 'Resend API Key', type: 'password', placeholder: 're_...' },
      { key: 'from', label: 'From', type: 'text', placeholder: 'alerts@tudominio.com', help: 'El dominio debe estar verificado en Resend.' },
      { key: 'to', label: 'Destinatarios (JSON array)', type: 'json', placeholder: '["ops@tudominio.com"]' },
      { key: 'subject_prefix', label: 'Prefijo del subject (opcional)', type: 'text', placeholder: '[PROD]' }
    ],
    telegram: [
      { key: 'bot_token', label: 'Bot token', type: 'password', placeholder: '123456:ABC...' },
      { key: 'chat_id', label: 'Chat ID', type: 'text', placeholder: '-1001234567890 o @canal' }
    ]
  };

  let channels: NotificationChannel[] = [];
  let channelsLoading = false;
  let channelsError = '';

  // Form en edición/creación. Si `editing.id === ''`, es un nuevo canal.
  let editing: {
    id: string;
    originalId: string; // '' si es nuevo, si no el id existente (para PUT vs POST)
    name: string;
    kind: ChannelKind;
    enabled: boolean;
    config: Record<string, string>;
  } | null = null;
  let channelSaving = false;
  let channelTestFeedback: Record<string, { kind: 'ok' | 'err'; message: string } | undefined> = {};

  async function loadChannels(): Promise<void> {
    channelsLoading = true;
    channelsError = '';
    try {
      channels = await listChannels();
    } catch (e: unknown) {
      channelsError = e instanceof Error ? e.message : String(e);
    } finally {
      channelsLoading = false;
    }
  }

  function startNew(): void {
    editing = {
      id: '',
      originalId: '',
      name: '',
      kind: 'webhook',
      enabled: true,
      config: {}
    };
  }

  function startEdit(ch: NotificationChannel): void {
    // Convierte la config (que llega con campos arbitrarios) a Record<string,string>
    // para que los inputs siempre tengan un value definido.
    const cfg: Record<string, string> = {};
    for (const f of KIND_FIELDS[ch.kind] ?? []) {
      const v = ch.config?.[f.key];
      if (f.type === 'json') {
        cfg[f.key] = v === undefined ? '' : JSON.stringify(v, null, 2);
      } else if (typeof v === 'string') {
        cfg[f.key] = v;
      } else if (v !== undefined && v !== null) {
        cfg[f.key] = JSON.stringify(v);
      } else {
        cfg[f.key] = '';
      }
    }
    editing = {
      id: ch.id,
      originalId: ch.id,
      name: ch.name,
      kind: ch.kind,
      enabled: ch.enabled,
      config: cfg
    };
  }

  function cancelEdit(): void {
    editing = null;
  }

  function onKindChange(): void {
    if (!editing) return;
    // Al cambiar el kind, vaciar campos para no enviar basura del kind anterior.
    editing.config = {};
  }

  /** Convierte el form en payload listo para el backend. Detecta enmascarados
   * (empiezan con "****" o contienen "://****") y los envía vacíos para
   * conservar el valor previo. */
  function buildPayload(): ChannelInput | { error: string } {
    if (!editing) return { error: 'no editing' };
    if (!editing.name.trim()) return { error: 'Nombre obligatorio' };
    const fields = KIND_FIELDS[editing.kind] ?? [];
    const config: Record<string, unknown> = {};
    for (const f of fields) {
      const raw = (editing.config[f.key] ?? '').trim();
      if (f.type === 'json') {
        if (raw === '') continue;
        try {
          config[f.key] = JSON.parse(raw);
        } catch {
          return { error: `Campo "${f.label}" no es JSON válido` };
        }
      } else if (f.type === 'password') {
        // Si parece enmascarado, omitirlo (el backend conserva el valor previo).
        if (raw === '' || raw.startsWith('****') || /:\*{4}/.test(raw) || raw.includes('://****')) {
          continue;
        }
        config[f.key] = raw;
      } else if (raw !== '') {
        config[f.key] = raw;
      }
    }
    const payload: ChannelInput = {
      name: editing.name.trim(),
      kind: editing.kind,
      enabled: editing.enabled,
      config
    };
    if (editing.originalId === '' && editing.id.trim()) {
      payload.id = editing.id.trim();
    }
    return payload;
  }

  async function saveChannel(): Promise<void> {
    if (!editing) return;
    const payload = buildPayload();
    if ('error' in payload) {
      channelsError = payload.error;
      toast.warning(payload.error);
      return;
    }
    channelSaving = true;
    channelsError = '';
    const wasEdit = editing.originalId !== '';
    const id = wasEdit ? editing.originalId : (payload.id ?? editing.id);
    try {
      if (editing.originalId === '') {
        await createChannel(payload);
      } else {
        await updateChannel(editing.originalId, payload);
      }
      editing = null;
      await loadChannels();
      toast.success(wasEdit ? `Canal "${id}" actualizado` : `Canal "${id}" creado`);
    } catch (e: unknown) {
      channelsError = e instanceof Error ? e.message : String(e);
      toast.fromError(wasEdit ? 'No se pudo actualizar el canal' : 'No se pudo crear el canal', e);
    } finally {
      channelSaving = false;
    }
  }

  async function removeChannel(id: string): Promise<void> {
    if (!confirm(`¿Eliminar el canal "${id}"? Las reglas que usen channel://${id} dejarán de notificar.`)) return;
    try {
      await deleteChannel(id);
      await loadChannels();
      toast.success(`Canal "${id}" eliminado`, {
        description: `Las reglas que usaban channel://${id} dejaron de notificar.`
      });
    } catch (e: unknown) {
      channelsError = e instanceof Error ? e.message : String(e);
      toast.fromError('No se pudo eliminar el canal', e);
    }
  }

  async function runChannelTest(id: string): Promise<void> {
    channelTestFeedback[id] = undefined;
    try {
      await testChannel(id, 'Notificación de prueba desde Settings → Integraciones');
      channelTestFeedback[id] = { kind: 'ok', message: '✓ Enviado' };
      toast.success(`Test enviado a "${id}"`, { description: 'Revisa el canal de destino.' });
    } catch (e: unknown) {
      channelTestFeedback[id] = {
        kind: 'err',
        message: e instanceof Error ? e.message : String(e)
      };
      toast.fromError(`El canal "${id}" no respondió correctamente`, e);
    }
    channelTestFeedback = { ...channelTestFeedback };
  }

  async function load(): Promise<void> {
    loading = true;
    try {
      tg = await fetchTelegramIntegration();
      defaultChatId = tg.default_chat_id;
      enabled = tg.enabled;
      // Pre-rellena el chat_id de prueba con el por defecto, si existe.
      if (!testChatId) testChatId = tg.default_chat_id;
    } catch (e: unknown) {
      saveError = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
    await loadChannels();
  }
  onMount(load);

  async function save(): Promise<void> {
    saving = true;
    saveError = '';
    try {
      tg = await saveTelegramIntegration({
        bot_token: botTokenInput,
        default_chat_id: defaultChatId,
        enabled
      });
      botTokenInput = '';
      defaultChatId = tg.default_chat_id;
      enabled = tg.enabled;
      toast.success('Integración de Telegram guardada');
    } catch (e: unknown) {
      saveError = e instanceof Error ? e.message : String(e);
      toast.fromError('No se pudo guardar la integración', e);
    } finally {
      saving = false;
    }
  }

  async function remove(): Promise<void> {
    if (!confirm('¿Eliminar la integración de Telegram? El token guardado se borrará.')) return;
    removing = true;
    saveError = '';
    try {
      tg = await deleteTelegramIntegration();
      botTokenInput = '';
      defaultChatId = '';
      enabled = false;
      toast.success('Integración de Telegram eliminada');
    } catch (e: unknown) {
      saveError = e instanceof Error ? e.message : String(e);
      toast.fromError('No se pudo eliminar la integración', e);
    } finally {
      removing = false;
    }
  }

  async function runTest(): Promise<void> {
    if (!testChatId.trim()) {
      testFeedback = { kind: 'err', message: 'chat_id obligatorio para la prueba' };
      toast.warning('Necesitas un chat_id para enviar el test');
      return;
    }
    testing = true;
    testFeedback = null;
    try {
      await testTelegramIntegration(testChatId.trim(), testText);
      testFeedback = { kind: 'ok', message: '✓ Mensaje enviado. Revisa el chat.' };
      toast.success('Mensaje de prueba enviado', { description: `Chat ID ${testChatId.trim()}` });
    } catch (e: unknown) {
      testFeedback = {
        kind: 'err',
        message: e instanceof Error ? e.message : String(e)
      };
      toast.fromError('El test de Telegram falló', e);
    } finally {
      testing = false;
    }
  }
</script>

<div class="page-header">
  <h1 class="page-title">Integraciones</h1>
</div>

<p class="muted" style="max-width: 720px; margin-bottom: 20px;">
  Configura aquí los servicios externos a los que Faro puede enviar notificaciones.
  Una vez configurada Telegram, las reglas de alerta podrán usar destinos con la
  forma <code>tg://&lt;chat_id&gt;</code>.
</p>

{#if loading}
  <div class="muted">Cargando…</div>
{:else}
  <section style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px; padding: 20px; max-width: 720px;">
    <div style="display: flex; align-items: center; gap: 12px;">
      <svg
        viewBox="0 0 240 240"
        width="32"
        height="32"
        aria-label="Telegram"
        role="img"
        style="flex-shrink: 0;"
      >
        <defs>
          <linearGradient id="tg-gradient" x1="0.667" x2="0.417" y1="0.167" y2="0.75">
            <stop offset="0" stop-color="#37aee2" />
            <stop offset="1" stop-color="#1e96c8" />
          </linearGradient>
        </defs>
        <circle cx="120" cy="120" r="120" fill="url(#tg-gradient)" />
        <path
          fill="#c8daea"
          d="M98 175c-3.888 0-3.227-1.468-4.568-5.17L82 132.207 170 80z"
        />
        <path fill="#a9c9dd" d="M98 175c3 0 4.325-1.372 6-3l16-15.558-19.958-12.035z" />
        <path
          fill="#fff"
          d="M100.04 144.41l48.36 35.729c5.519 3.045 9.501 1.468 10.876-5.123l19.685-92.763c2.015-8.08-3.08-11.746-8.36-9.349l-115.59 44.571c-7.89 3.165-7.843 7.567-1.438 9.528l29.663 9.259 68.673-43.325c3.242-1.966 6.218-.91 3.776 1.258z"
        />
      </svg>
      <div style="flex: 1;">
        <h2 style="margin: 0; font-size: 16px;">Telegram</h2>
        <div class="muted" style="font-size: 12px;">Bot API · notificaciones por chat, grupo o canal</div>
      </div>
      {#if tg?.configured}
        <span class="badge ok">Activa</span>
      {:else if tg?.bot_token_masked}
        <span class="badge debug">Inactiva</span>
      {:else}
        <span class="badge debug">Sin configurar</span>
      {/if}
    </div>

    {#if tg?.bot_token_masked}
      <div style="margin-top: 16px; font-size: 12px;" class="muted">
        Token actual: <code>{tg.bot_token_masked}</code>
        {#if tg.updated_at}
          · actualizado {formatTimestamp(tg.updated_at)}{tg.updated_by ? ` por ${tg.updated_by}` : ''}
        {/if}
      </div>
    {/if}

    <div class="field" style="margin-top: 16px;">
      <label for="bot-token">Token del bot</label>
      <input
        id="bot-token"
        type="password"
        autocomplete="off"
        bind:value={botTokenInput}
        placeholder={tg?.bot_token_masked
          ? 'Deja vacío para conservar el token actual'
          : '123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11'}
      />
      <small class="muted">
        Crea un bot con <a href="https://t.me/BotFather" target="_blank" rel="noreferrer">@BotFather</a>
        y pega aquí el token que te devuelve.
      </small>
    </div>

    <div class="field">
      <label for="default-chat">Chat ID por defecto (opcional)</label>
      <input
        id="default-chat"
        bind:value={defaultChatId}
        placeholder="-1001234567890 o @mi_canal"
      />
      <small class="muted">
        Se usa solo para pre-rellenar la prueba. Las reglas siguen definiendo sus propios destinos.
      </small>
    </div>

    <div class="field">
      <label>
        <input type="checkbox" bind:checked={enabled} />
        Habilitada (las reglas pueden enviar notificaciones)
      </label>
    </div>

    {#if saveError}
      <div style="color: var(--danger); margin-bottom: 8px;">{saveError}</div>
    {/if}

    <div style="display: flex; gap: 8px;">
      <button class="primary" on:click={save} disabled={saving}>
        {saving ? 'Guardando…' : 'Guardar'}
      </button>
      {#if tg?.bot_token_masked}
        <button class="danger" on:click={remove} disabled={removing}>
          {removing ? 'Eliminando…' : 'Eliminar token'}
        </button>
      {/if}
    </div>

    {#if tg?.configured}
      <hr style="margin: 24px 0; border: none; border-top: 1px solid var(--border);" />

      <h3 style="margin: 0 0 8px; font-size: 14px;">Enviar mensaje de prueba</h3>
      <div class="flex gap-8">
        <div class="field grow">
          <label for="test-chat">Chat ID</label>
          <input id="test-chat" bind:value={testChatId} placeholder="-1001234567890" />
        </div>
      </div>
      <div class="field">
        <label for="test-text">Texto (opcional, HTML permitido)</label>
        <input id="test-text" bind:value={testText} placeholder="🧪 Prueba desde Faro" />
      </div>
      <button on:click={runTest} disabled={testing}>
        {testing ? 'Enviando…' : 'Enviar prueba'}
      </button>
      {#if testFeedback}
        <div
          style="margin-top: 8px; color: {testFeedback.kind === 'ok'
            ? 'var(--ok)'
            : 'var(--danger)'};"
        >
          {testFeedback.message}
        </div>
      {/if}
    {/if}

    <hr style="margin: 24px 0; border: none; border-top: 1px solid var(--border);" />
    <details>
      <summary style="cursor: pointer; font-size: 13px;">¿Cómo obtengo el chat_id?</summary>
      <ol style="margin-top: 8px; font-size: 13px; line-height: 1.6; padding-left: 20px;">
        <li>Añade el bot al chat / grupo / canal.</li>
        <li>Envíale un mensaje cualquiera.</li>
        <li>
          Visita <code>https://api.telegram.org/bot&lt;TOKEN&gt;/getUpdates</code> y busca
          <code>chat.id</code>.
        </li>
        <li>Para canales públicos puedes usar directamente <code>@nombre_canal</code>.</li>
      </ol>
    </details>
  </section>

  <!-- ============ Canales de notificación (multi-instancia) ============ -->
  <section
    style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 6px;
           padding: 20px; max-width: 720px; margin-top: 24px;"
  >
    <div style="display: flex; align-items: center; justify-content: space-between; gap: 12px;">
      <div>
        <h2 style="margin: 0; font-size: 16px;">Canales de notificación</h2>
        <div class="muted" style="font-size: 12px; margin-top: 2px;">
          Webhooks, Slack, Discord, PagerDuty, OpsGenie y email — referenciables desde las
          reglas con <code>channel://&lt;id&gt;</code>.
        </div>
      </div>
      {#if !editing}
        <button class="primary" on:click={startNew}>+ Añadir canal</button>
      {/if}
    </div>

    {#if channelsError}
      <div style="color: var(--danger); margin-top: 12px;">{channelsError}</div>
    {/if}

    {#if channelsLoading}
      <div class="muted" style="margin-top: 12px;">Cargando…</div>
    {:else if channels.length === 0 && !editing}
      <div class="muted" style="margin-top: 16px; font-size: 13px;">
        No hay canales todavía. Crea uno para que las reglas puedan notificar a Slack/Discord/PagerDuty/etc.
      </div>
    {:else if !editing}
      <table style="width: 100%; margin-top: 16px; border-collapse: collapse; font-size: 13px;">
        <thead>
          <tr style="text-align: left; color: var(--muted);">
            <th style="padding: 6px 0;">ID</th>
            <th style="padding: 6px 0;">Nombre</th>
            <th style="padding: 6px 0;">Tipo</th>
            <th style="padding: 6px 0;">Estado</th>
            <th style="padding: 6px 0; text-align: right;">Acciones</th>
          </tr>
        </thead>
        <tbody>
          {#each channels as c (c.id)}
            <tr style="border-top: 1px solid var(--border);">
              <td style="padding: 8px 0;"><code>{c.id}</code></td>
              <td style="padding: 8px 0;">{c.name || '—'}</td>
              <td style="padding: 8px 0;">
                <span class="badge debug">{KIND_LABEL[c.kind] ?? c.kind}</span>
              </td>
              <td style="padding: 8px 0;">
                {#if c.enabled}
                  <span class="badge ok">Activo</span>
                {:else}
                  <span class="badge debug">Inactivo</span>
                {/if}
              </td>
              <td style="padding: 8px 0; text-align: right;">
                <button on:click={() => runChannelTest(c.id)} disabled={!c.enabled}
                  >Probar</button
                >
                <button on:click={() => startEdit(c)}>Editar</button>
                <button class="danger" on:click={() => removeChannel(c.id)}>Eliminar</button>
                {#if channelTestFeedback[c.id]}
                  <div
                    style="margin-top: 4px; font-size: 11px; color: {channelTestFeedback[c.id]
                      ?.kind === 'ok'
                      ? 'var(--ok)'
                      : 'var(--danger)'};"
                  >
                    {channelTestFeedback[c.id]?.message}
                  </div>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    {#if editing}
      <div
        style="margin-top: 16px; padding: 16px; border: 1px solid var(--border); border-radius: 6px;"
      >
        <h3 style="margin: 0 0 12px; font-size: 14px;">
          {editing.originalId === '' ? 'Nuevo canal' : `Editando: ${editing.originalId}`}
        </h3>

        <div class="field">
          <label for="ch-name">Nombre</label>
          <input id="ch-name" bind:value={editing.name} placeholder="Ops PagerDuty" />
        </div>

        {#if editing.originalId === ''}
          <div class="field">
            <label for="ch-id">ID (opcional)</label>
            <input
              id="ch-id"
              bind:value={editing.id}
              placeholder="ops-pagerduty — sólo [a-z0-9-], se autogenera del nombre si lo dejas vacío"
            />
          </div>
        {/if}

        <div class="field">
          <label for="ch-kind">Tipo</label>
          <select id="ch-kind" bind:value={editing.kind} on:change={onKindChange}>
            {#each Object.entries(KIND_LABEL) as [k, label]}
              <option value={k}>{label}</option>
            {/each}
          </select>
        </div>

        {#each KIND_FIELDS[editing.kind] ?? [] as f (f.key)}
          <div class="field">
            <label for={`ch-cfg-${f.key}`}>{f.label}</label>
            {#if f.type === 'textarea' || f.type === 'json'}
              <textarea
                id={`ch-cfg-${f.key}`}
                rows="4"
                bind:value={editing.config[f.key]}
                placeholder={f.placeholder ?? ''}
                style="font-family: monospace; font-size: 12px;"
              ></textarea>
            {:else if f.type === 'password'}
              <input
                id={`ch-cfg-${f.key}`}
                type="password"
                autocomplete="off"
                bind:value={editing.config[f.key]}
                placeholder={editing.originalId !== ''
                  ? 'Deja vacío para conservar el actual'
                  : (f.placeholder ?? '')}
              />
            {:else}
              <input
                id={`ch-cfg-${f.key}`}
                bind:value={editing.config[f.key]}
                placeholder={f.placeholder ?? ''}
              />
            {/if}
            {#if f.help}
              <small class="muted">{f.help}</small>
            {/if}
          </div>
        {/each}

        <div class="field">
          <label>
            <input type="checkbox" bind:checked={editing.enabled} />
            Habilitado (las reglas pueden disparar este canal)
          </label>
        </div>

        <div style="display: flex; gap: 8px;">
          <button class="primary" on:click={saveChannel} disabled={channelSaving}>
            {channelSaving ? 'Guardando…' : 'Guardar'}
          </button>
          <button on:click={cancelEdit} disabled={channelSaving}>Cancelar</button>
        </div>
      </div>
    {/if}

    <details style="margin-top: 16px;">
      <summary style="cursor: pointer; font-size: 13px;">¿Cómo usar un canal en una regla?</summary>
      <div style="margin-top: 8px; font-size: 13px; line-height: 1.6;">
        En el campo <code>notification_targets</code> de una regla, añade
        <code>channel://&lt;id&gt;</code>. Por ejemplo, si el canal se llama
        <code>ops-pagerduty</code>, el target es <code>channel://ops-pagerduty</code>.
        <br /><br />
        Sigues pudiendo usar los formatos viejos: <code>tg://&lt;chat_id&gt;</code> y
        <code>https://...</code> (webhook directo). El nuevo formato sólo añade flexibilidad.
      </div>
    </details>
  </section>
{/if}
