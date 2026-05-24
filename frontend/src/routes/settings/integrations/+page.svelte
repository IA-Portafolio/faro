<script lang="ts">
  import { onMount } from 'svelte';
  import {
    fetchTelegramIntegration,
    saveTelegramIntegration,
    deleteTelegramIntegration,
    testTelegramIntegration,
    type TelegramIntegration
  } from '$lib/api';
  import { formatTimestamp } from '$lib/stores';

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
    } catch (e: unknown) {
      saveError = e instanceof Error ? e.message : String(e);
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
    } catch (e: unknown) {
      saveError = e instanceof Error ? e.message : String(e);
    } finally {
      removing = false;
    }
  }

  async function runTest(): Promise<void> {
    if (!testChatId.trim()) {
      testFeedback = { kind: 'err', message: 'chat_id obligatorio para la prueba' };
      return;
    }
    testing = true;
    testFeedback = null;
    try {
      await testTelegramIntegration(testChatId.trim(), testText);
      testFeedback = { kind: 'ok', message: '✓ Mensaje enviado. Revisa el chat.' };
    } catch (e: unknown) {
      testFeedback = {
        kind: 'err',
        message: e instanceof Error ? e.message : String(e)
      };
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
      <span style="font-size: 22px;">💬</span>
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
{/if}
