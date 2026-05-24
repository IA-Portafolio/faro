<script lang="ts">
  import { onMount } from 'svelte';
  import { formatTimestamp } from '$lib/stores';
  import {
    fetchSessions,
    revokeOtherSessions,
    fetchTwoFaStatus,
    twoFaSetup,
    twoFaEnable,
    twoFaDisable,
    twoFaRegenRecovery,
    type SessionInfo,
    type TwoFaStatus,
    type TwoFaSetup,
  } from '$lib/api';

  let sessions: SessionInfo[] = [];
  let status: TwoFaStatus = { enabled: false, recovery_codes_remaining: 0 };
  let loading = true;
  let error = '';
  let toast = '';

  // --- Setup wizard state ---
  let setup: TwoFaSetup | null = null;
  let setupCode = '';
  let setupBusy = false;
  let setupError = '';
  // Códigos plaintext que el backend devolvió tras enable o regenerate. SE
  // MUESTRAN UNA SOLA VEZ — al cerrar el panel ya no volvemos a tenerlos.
  let freshRecoveryCodes: string[] = [];

  // --- Disable form state ---
  let disableOpen = false;
  let disablePass = '';
  let disableCode = '';
  let disableRecovery = '';
  let disableBusy = false;
  let disableError = '';

  // --- Regenerate recovery codes form state ---
  let regenOpen = false;
  let regenPass = '';
  let regenCode = '';
  let regenBusy = false;
  let regenError = '';

  async function loadAll(): Promise<void> {
    loading = true;
    error = '';
    try {
      [sessions, status] = await Promise.all([fetchSessions(), fetchTwoFaStatus()]);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  onMount(loadAll);

  function showToast(msg: string): void {
    toast = msg;
    setTimeout(() => (toast = ''), 3000);
  }

  async function onRevokeOthers(): Promise<void> {
    if (!confirm('Cerrar sesión en todos los demás dispositivos. ¿Continuar?')) return;
    try {
      const r = await revokeOtherSessions();
      showToast(`${r.revoked} sesión(es) revocada(s).`);
      await loadAll();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  async function onStartSetup(): Promise<void> {
    setupError = '';
    try {
      setup = await twoFaSetup();
      setupCode = '';
    } catch (e) {
      setupError = e instanceof Error ? e.message : String(e);
    }
  }

  async function onConfirmEnable(): Promise<void> {
    if (!setup) return;
    setupBusy = true;
    setupError = '';
    try {
      const r = await twoFaEnable(setupCode.trim());
      freshRecoveryCodes = r.recovery_codes;
      setup = null;
      setupCode = '';
      await loadAll();
    } catch (e) {
      setupError = e instanceof Error ? e.message : String(e);
    } finally {
      setupBusy = false;
    }
  }

  function onCancelSetup(): void {
    setup = null;
    setupCode = '';
    setupError = '';
  }

  async function onDisable(): Promise<void> {
    disableBusy = true;
    disableError = '';
    try {
      const code = disableCode.trim();
      const recovery = disableRecovery.trim();
      if (!code && !recovery) {
        throw new Error('código TOTP o recovery code obligatorio');
      }
      await twoFaDisable({
        password: disablePass,
        code: code || undefined,
        recovery_code: recovery || undefined,
      });
      disableOpen = false;
      disablePass = '';
      disableCode = '';
      disableRecovery = '';
      showToast('2FA desactivado.');
      await loadAll();
    } catch (e) {
      disableError = e instanceof Error ? e.message : String(e);
    } finally {
      disableBusy = false;
    }
  }

  async function onRegen(): Promise<void> {
    regenBusy = true;
    regenError = '';
    try {
      const r = await twoFaRegenRecovery({ password: regenPass, code: regenCode.trim() });
      freshRecoveryCodes = r.recovery_codes;
      regenOpen = false;
      regenPass = '';
      regenCode = '';
      await loadAll();
    } catch (e) {
      regenError = e instanceof Error ? e.message : String(e);
    } finally {
      regenBusy = false;
    }
  }

  function copyCodes(): void {
    navigator.clipboard
      .writeText(freshRecoveryCodes.join('\n'))
      .then(() => showToast('Códigos copiados al portapapeles.'));
  }

  function downloadCodes(): void {
    const text = `Faro recovery codes — generados ${new Date().toISOString()}\n\n${freshRecoveryCodes.join('\n')}\n`;
    const blob = new Blob([text], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = 'faro-recovery-codes.txt';
    a.click();
    URL.revokeObjectURL(url);
  }
</script>

<div class="page-header">
  <h1 class="page-title">Seguridad</h1>
</div>

{#if error}<div class="card" style="color: var(--danger); margin-top: 12px;">{error}</div>{/if}
{#if toast}<div class="card" style="margin-top: 12px;">{toast}</div>{/if}

<!-- ============ Sessions ============ -->
<h2 style="font-size: 16px; margin-top: 24px;">Sesiones activas</h2>
<div class="card mt-8">
  {#if loading}
    <div class="empty"><span class="spinner"></span></div>
  {:else if sessions.length === 0}
    <div class="muted">No hay sesiones activas.</div>
  {:else}
    <table style="width: 100%; border-collapse: collapse;">
      <thead>
        <tr style="text-align: left; color: var(--text-muted); font-weight: 500;">
          <th style="padding: 6px 8px;">Sesión</th>
          <th style="padding: 6px 8px;">Iniciada</th>
          <th style="padding: 6px 8px;">Expira</th>
        </tr>
      </thead>
      <tbody>
        {#each sessions as s}
          <tr style="border-top: 1px solid var(--border);">
            <td style="padding: 6px 8px;" class="mono">
              {s.token_hash.slice(0, 12)}…
              {#if s.is_current}<span class="badge" style="margin-left: 6px;">actual</span>{/if}
            </td>
            <td style="padding: 6px 8px;">{formatTimestamp(s.created_at)}</td>
            <td style="padding: 6px 8px;">{formatTimestamp(s.expires_at)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
    {#if sessions.some((s) => !s.is_current)}
      <div style="margin-top: 12px;">
        <button on:click={onRevokeOthers}>Cerrar otras sesiones</button>
      </div>
    {/if}
  {/if}
</div>

<!-- ============ 2FA ============ -->
<h2 style="font-size: 16px; margin-top: 24px;">Autenticación en dos pasos (2FA)</h2>
<div class="card mt-8">
  {#if loading}
    <div class="empty"><span class="spinner"></span></div>
  {:else if status.enabled}
    <div>
      <strong>2FA activo.</strong>
      <span class="muted">
        Te quedan {status.recovery_codes_remaining} código(s) de recuperación sin usar.
      </span>
    </div>
    <div class="flex gap-8" style="margin-top: 12px;">
      <button on:click={() => (regenOpen = true)}>Regenerar códigos de recuperación</button>
      <button on:click={() => (disableOpen = true)}>Desactivar 2FA</button>
    </div>
  {:else if setup}
    <div>
      <strong>Configurar 2FA</strong>
      <p class="muted" style="margin-top: 4px;">
        Escanea este QR con Google Authenticator, Authy o 1Password. Luego ingresa el código
        de 6 dígitos que la app te genere para verificar y activar 2FA.
      </p>
      <div style="display: flex; gap: 16px; align-items: flex-start; flex-wrap: wrap; margin-top: 12px;">
        <!-- El SVG viene del backend, mismo origen; CSP `img-src 'self'` lo acepta. -->
        <div style="background: white; padding: 8px; border-radius: 6px; width: 256px; height: 256px;">
          {@html setup.qr_svg}
        </div>
        <div style="flex: 1; min-width: 240px;">
          <div class="muted" style="font-size: 12px;">Entrada manual (si no podés escanear)</div>
          <div class="mono" style="margin-top: 4px; word-break: break-all; user-select: all;">{setup.secret_base32}</div>
          <div style="margin-top: 12px;">
            <label for="setup-code" style="display: block; font-size: 12px; color: var(--text-muted);">
              Código de verificación
            </label>
            <input
              id="setup-code"
              type="text"
              inputmode="numeric"
              autocomplete="one-time-code"
              maxlength="7"
              placeholder="123456"
              bind:value={setupCode}
              style="margin-top: 4px; width: 140px; font-family: var(--font-mono); font-size: 16px; letter-spacing: 2px;"
              on:keydown={(e) => { if (e.key === 'Enter') onConfirmEnable(); }}
            />
          </div>
          {#if setupError}
            <div style="color: var(--danger); margin-top: 8px;">{setupError}</div>
          {/if}
          <div class="flex gap-8" style="margin-top: 12px;">
            <button on:click={onConfirmEnable} disabled={setupBusy || setupCode.trim().length < 6}>
              {setupBusy ? 'Verificando…' : 'Verificar y activar'}
            </button>
            <button on:click={onCancelSetup} type="button">Cancelar</button>
          </div>
        </div>
      </div>
    </div>
  {:else}
    <div>
      <span class="muted">2FA no está activo en esta cuenta.</span>
    </div>
    <div style="margin-top: 12px;">
      <button on:click={onStartSetup}>Activar 2FA</button>
    </div>
    {#if setupError}<div style="color: var(--danger); margin-top: 8px;">{setupError}</div>{/if}
  {/if}
</div>

<!-- ============ Recovery codes (mostrar UNA vez tras enable/regen) ============ -->
{#if freshRecoveryCodes.length > 0}
  <div class="card mt-8" style="border-color: var(--accent);">
    <h3 style="margin: 0 0 8px 0;">Guarda estos códigos de recuperación</h3>
    <p class="muted">
      No volverán a mostrarse. Cada uno funciona <strong>una sola vez</strong> y reemplaza
      al código TOTP si perdés acceso al authenticator.
    </p>
    <pre style="background: var(--card-bg); padding: 12px; border-radius: 4px; user-select: all;">
{freshRecoveryCodes.join('\n')}
    </pre>
    <div class="flex gap-8" style="margin-top: 8px;">
      <button on:click={copyCodes}>Copiar</button>
      <button on:click={downloadCodes}>Descargar .txt</button>
      <button on:click={() => (freshRecoveryCodes = [])}>Cerrar</button>
    </div>
  </div>
{/if}

<!-- ============ Disable modal ============ -->
{#if disableOpen}
  <div class="card mt-8">
    <h3 style="margin: 0 0 8px 0;">Desactivar 2FA</h3>
    <p class="muted">
      Confirma tu contraseña + un código TOTP <em>o</em> un código de recuperación.
      Esto borra el secreto y todos los códigos restantes.
    </p>
    <div class="field"><label for="dis-pass">Contraseña</label>
      <input id="dis-pass" type="password" bind:value={disablePass} autocomplete="current-password" />
    </div>
    <div class="field"><label for="dis-code">Código TOTP (6 dígitos)</label>
      <input id="dis-code" type="text" inputmode="numeric" maxlength="7" bind:value={disableCode} />
    </div>
    <div class="field"><label for="dis-rec">o código de recuperación</label>
      <input id="dis-rec" type="text" bind:value={disableRecovery} />
    </div>
    {#if disableError}<div style="color: var(--danger); margin: 8px 0;">{disableError}</div>{/if}
    <div class="flex gap-8" style="margin-top: 12px;">
      <button on:click={onDisable} disabled={disableBusy || !disablePass}>
        {disableBusy ? 'Procesando…' : 'Desactivar 2FA'}
      </button>
      <button on:click={() => (disableOpen = false)} type="button">Cancelar</button>
    </div>
  </div>
{/if}

<!-- ============ Regen recovery modal ============ -->
{#if regenOpen}
  <div class="card mt-8">
    <h3 style="margin: 0 0 8px 0;">Regenerar códigos de recuperación</h3>
    <p class="muted">
      Los códigos viejos quedan invalidados. Confirma con tu contraseña y un código TOTP actual.
    </p>
    <div class="field"><label for="reg-pass">Contraseña</label>
      <input id="reg-pass" type="password" bind:value={regenPass} autocomplete="current-password" />
    </div>
    <div class="field"><label for="reg-code">Código TOTP (6 dígitos)</label>
      <input id="reg-code" type="text" inputmode="numeric" maxlength="7" bind:value={regenCode} />
    </div>
    {#if regenError}<div style="color: var(--danger); margin: 8px 0;">{regenError}</div>{/if}
    <div class="flex gap-8" style="margin-top: 12px;">
      <button on:click={onRegen} disabled={regenBusy || !regenPass || regenCode.trim().length < 6}>
        {regenBusy ? 'Generando…' : 'Regenerar'}
      </button>
      <button on:click={() => (regenOpen = false)} type="button">Cancelar</button>
    </div>
  </div>
{/if}

<style>
  .field { margin-top: 12px; }
  .field label {
    display: block;
    font-size: 12px;
    color: var(--text-muted);
    margin-bottom: 4px;
  }
  .field input { width: 100%; max-width: 320px; }
  pre {
    font-family: var(--font-mono);
    font-size: 13px;
    line-height: 1.7;
    margin: 0;
    white-space: pre;
  }
</style>
