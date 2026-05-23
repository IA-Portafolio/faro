<script lang="ts">
  import { onMount } from 'svelte';
  import { goto } from '$app/navigation';
  import { page } from '$app/stores';
  import '../../app.css';
  import { login, me } from '$lib/api';
  import { currentUser } from '$lib/stores';

  let email = '';
  let password = '';
  let error = '';
  let loading = false;

  onMount(async () => {
    try {
      const u = await me();
      currentUser.set(u);
      const next = $page.url.searchParams.get('next') || '/';
      await goto(next, { replaceState: true });
    } catch (_e) {
      // not logged in — show form
    }
  });

  async function submit(e: SubmitEvent): Promise<void> {
    e.preventDefault();
    error = '';
    loading = true;
    try {
      const u = await login({ email, password });
      currentUser.set(u);
      const next = $page.url.searchParams.get('next') || '/';
      await goto(next, { replaceState: true });
    } catch (err: unknown) {
      error = err instanceof Error ? err.message : String(err);
      if (error.includes('401') || error === 'unauthorized') {
        error = 'Email o contraseña incorrectos';
      }
    } finally {
      loading = false;
    }
  }
</script>

<svelte:head><title>Iniciar sesión · Faro</title></svelte:head>

<div style="min-height: 100vh; display: grid; place-items: center; background: var(--bg);">
  <form on:submit={submit} style="background: var(--bg-elev); border: 1px solid var(--border); border-radius: 8px; padding: 32px; width: min(380px, 92vw);">
    <div style="display: flex; align-items: center; gap: 10px; margin-bottom: 4px;">
      <span class="brand-dot"></span>
      <strong style="font-size: 20px; letter-spacing: 0.5px;">Faro</strong>
    </div>
    <p class="muted" style="margin-top: 4px; margin-bottom: 24px; font-size: 13px;">Inicia sesión para acceder al panel.</p>

    <div class="field">
      <label>Email</label>
      <input type="email" bind:value={email} required autocomplete="email" autofocus />
    </div>
    <div class="field">
      <label>Contraseña</label>
      <input type="password" bind:value={password} required autocomplete="current-password" />
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
</div>
