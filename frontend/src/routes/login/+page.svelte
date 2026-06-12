<script lang="ts">
  /**
   * Página `/login` — acceso con email + contraseña y 2FA opcional (TOTP).
   *
   * Fase 1: email/password. Si el backend responde `needs_totp`, la fase 2 pide el
   * código TOTP (o un código de recuperación), usando un `challenge_token` para no
   * re-validar la contraseña. Si ya hay sesión activa, redirige a `?next=` (o "/").
   */
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import '../../app.css';
  import { login, loginTotp, me } from '$lib/api';
  import { currentUser } from '$lib/stores';
  import { safeNext } from '$lib/safe-next';

  // Fase 1: email + password.
  let email = '';
  let password = '';
  let error = '';
  let loading = false;

  // Fase 2: si el backend nos devuelve `needs_totp`, mostramos el form de código.
  // `challenge_token` viaja en cleartext de vuelta al backend para asociarnos al
  // user que pasó password sin tener que re-validar credentials.
  let challengeToken = '';
  let challengeExpiresAt = 0;
  let totpCode = '';
  let useRecovery = false;

  onMount(async () => {
    try {
      const u = await me();
      currentUser.set(u);
      const next = safeNext($page.url.searchParams.get('next'));
      await goto(next, { replaceState: true });
    } catch (_e) {
      // sin sesión — muestra el formulario
    }
  });

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    error = '';
    loading = true;
    try {
      const r = await login({ email, password });
      if ('needs_totp' in r && r.needs_totp) {
        challengeToken = r.challenge_token;
        challengeExpiresAt = Date.now() + r.expires_in_secs * 1000;
        // Limpiamos el password de memoria; ya no lo necesitamos. El challenge
        // alcanza para fase 2.
        password = '';
        return;
      }
      currentUser.set(r);
      const next = safeNext($page.url.searchParams.get('next'));
      await goto(next, { replaceState: true });
    } catch (err: unknown) {
      error = friendlyError(err);
    } finally {
      loading = false;
    }
  }

  async function submitTotp(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    error = '';
    loading = true;
    try {
      const u = await loginTotp({
        challenge_token: challengeToken,
        code: totpCode.trim(),
        recovery: useRecovery,
      });
      currentUser.set(u);
      const next = safeNext($page.url.searchParams.get('next'));
      await goto(next, { replaceState: true });
    } catch (err: unknown) {
      error = friendlyError(err);
      // Si el challenge expiró, volvemos a fase 1.
      if (error.includes('401') || error.includes('unauthorized')) {
        if (Date.now() > challengeExpiresAt) {
          resetToPhase1('La verificación expiró, ingresá tus credenciales de nuevo.');
        } else {
          error = useRecovery ? 'Código de recuperación inválido o ya usado' : 'Código incorrecto';
        }
      } else if (error.includes('429')) {
        error = 'Demasiados intentos. Esperá un minuto.';
      }
    } finally {
      loading = false;
    }
  }

  function friendlyError(err: unknown): string {
    const m = err instanceof Error ? err.message : String(err);
    if (m === 'unauthorized') return 'Email o contraseña incorrectos';
    return m;
  }

  function resetToPhase1(msg = ''): void {
    challengeToken = '';
    challengeExpiresAt = 0;
    totpCode = '';
    useRecovery = false;
    password = '';
    error = msg;
  }
</script>

<svelte:head><title>Iniciar sesión · Faro</title></svelte:head>

<div style="min-height: 100vh; display: grid; place-items: center; background: var(--bg);">
  {#if !challengeToken}
    <form on:submit={submit} style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 8px; padding: 32px; width: min(380px, 92vw);">
      <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 4px;">
        <span class="brand-dot"></span>
        <strong style="font-size: 20px; letter-spacing: 0.5px;">Faro</strong>
      </div>
      <p class="muted" style="margin-top: 4px; margin-bottom: 24px; font-size: 13px;">Inicia sesión para acceder al panel.</p>

      <div class="field">
        <label for="email-input">Email</label>
        <input id="email-input" type="email" bind:value={email} required autocomplete="email" autofocus />
      </div>
      <div class="field">
        <label for="pass-input">Contraseña</label>
        <input id="pass-input" type="password" bind:value={password} required autocomplete="current-password" />
      </div>

      {#if error}
        <div style="color: var(--danger); font-size: 13px; margin-bottom: 12px;">{error}</div>
      {/if}

      <button class="primary" type="submit" disabled={loading} style="width: 100%;">
        {loading ? 'Entrando…' : 'Iniciar sesión'}
      </button>

      <div class="muted" style="font-size: 11px; margin-top: 16px; text-align: center;">
        ¿Es la primera vez? Define <code>FARO_BOOTSTRAP_ADMIN_EMAIL</code> y
        <code>FARO_BOOTSTRAP_ADMIN_PASSWORD</code> en el servidor.
      </div>
    </form>
  {:else}
    <form on:submit={submitTotp} style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 8px; padding: 32px; width: min(380px, 92vw);">
      <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 4px;">
        <span class="brand-dot"></span>
        <strong style="font-size: 20px; letter-spacing: 0.5px;">Verificación en dos pasos</strong>
      </div>
      <p class="muted" style="margin-top: 4px; margin-bottom: 20px; font-size: 13px;">
        {useRecovery
          ? 'Ingresa uno de tus códigos de recuperación.'
          : 'Ingresa el código de 6 dígitos de tu authenticator.'}
      </p>

      <div class="field">
        <label for="totp-input">{useRecovery ? 'Código de recuperación' : 'Código TOTP'}</label>
        <input
          id="totp-input"
          type="text"
          inputmode={useRecovery ? 'text' : 'numeric'}
          autocomplete="one-time-code"
          maxlength={useRecovery ? 16 : 7}
          bind:value={totpCode}
          required
          autofocus
          style="font-family: var(--font-mono); font-size: 18px; letter-spacing: 3px; text-align: center;"
        />
      </div>

      {#if error}
        <div style="color: var(--danger); font-size: 13px; margin-bottom: 12px;">{error}</div>
      {/if}

      <button class="primary" type="submit" disabled={loading || totpCode.trim().length < 6} style="width: 100%;">
        {loading ? 'Verificando…' : 'Verificar'}
      </button>

      <div style="display: flex; justify-content: space-between; margin-top: 12px; font-size: 12px;">
        <button type="button" class="link" on:click={() => { useRecovery = !useRecovery; totpCode = ''; error = ''; }}>
          {useRecovery ? 'Usar código TOTP' : 'Usar código de recuperación'}
        </button>
        <button type="button" class="link" on:click={() => resetToPhase1()}>
          Cancelar
        </button>
      </div>
    </form>
  {/if}
</div>

<style>
  button.link {
    background: none;
    border: none;
    color: var(--accent);
    cursor: pointer;
    padding: 0;
    font-size: inherit;
  }
  button.link:hover { text-decoration: underline; }
</style>
