<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { page } from '$app/stores';
  import { fetchReplay, type ReplayPayload } from '$lib/api';
  import { formatTimestamp } from '$lib/stores';

  // rrweb-player se carga desde CDN para no añadir deps de build al dashboard.
  // En entornos sin CDN (Cloudflare blocked, on-prem aislado) habría que
  // bundlear local — TODO si llega el caso.
  const PLAYER_JS = 'https://cdn.jsdelivr.net/npm/rrweb-player@1.0.0-alpha.4/dist/index.js';
  const PLAYER_CSS = 'https://cdn.jsdelivr.net/npm/rrweb-player@1.0.0-alpha.4/dist/style.css';

  // El módulo expone `Replayer` (default) — type-erased aquí porque cargamos por URL.
  type RrwebPlayerCtor = new (opts: {
    target: HTMLElement;
    props: { events: unknown[]; autoPlay?: boolean; showController?: boolean; width?: number; height?: number };
  }) => { $destroy: () => void };

  let replay: ReplayPayload | null = null;
  let loading = true;
  let error = '';
  let playerEl: HTMLDivElement;
  let playerInstance: { $destroy: () => void } | null = null;

  $: sessionId = $page.params.session_id;

  async function loadPlayerScript(): Promise<RrwebPlayerCtor> {
    // CSS — idempotente.
    if (!document.querySelector(`link[data-faro-rrweb]`)) {
      const link = document.createElement('link');
      link.rel = 'stylesheet';
      link.href = PLAYER_CSS;
      link.dataset.faroRrweb = '1';
      document.head.appendChild(link);
    }
    // Script — esperar al load del existente si ya hay uno en vuelo.
    type W = Window & { rrwebPlayer?: RrwebPlayerCtor };
    const w = window as W;
    if (w.rrwebPlayer) return w.rrwebPlayer;
    const existing = document.querySelector<HTMLScriptElement>('script[data-faro-rrweb]');
    if (existing) {
      await new Promise<void>((resolve, reject) => {
        existing.addEventListener('load', () => resolve(), { once: true });
        existing.addEventListener('error', () => reject(new Error('rrweb-player script load failed')), { once: true });
      });
      if (!w.rrwebPlayer) throw new Error('rrweb-player no se registró en window');
      return w.rrwebPlayer;
    }
    await new Promise<void>((resolve, reject) => {
      const s = document.createElement('script');
      s.src = PLAYER_JS;
      s.async = true;
      s.dataset.faroRrweb = '1';
      s.addEventListener('load', () => resolve(), { once: true });
      s.addEventListener('error', () => reject(new Error('rrweb-player script load failed')), { once: true });
      document.head.appendChild(s);
    });
    if (!w.rrwebPlayer) throw new Error('rrweb-player no se registró en window');
    return w.rrwebPlayer;
  }

  function clearChildren(el: HTMLElement): void {
    while (el.firstChild) el.removeChild(el.firstChild);
  }

  async function load(): Promise<void> {
    const sid = sessionId;
    if (!sid) return;
    loading = true;
    error = '';
    try {
      replay = await fetchReplay(sid);
    } catch (e: unknown) {
      error = e instanceof Error ? e.message : String(e);
      loading = false;
      return;
    }
    if (!replay || replay.events.length === 0) {
      error = 'No hay eventos para esta sesión (puede haber caído del TTL de 7 días)';
      loading = false;
      return;
    }
    try {
      const Player = await loadPlayerScript();
      // Limpia un player previo antes de montar otro (HMR / re-navegación).
      if (playerInstance) {
        try { playerInstance.$destroy(); } catch { /* ignora */ }
        playerInstance = null;
      }
      clearChildren(playerEl);
      const width = Math.min(1200, Math.max(600, playerEl.clientWidth - 16));
      const height = Math.round(width * 0.6);
      playerInstance = new Player({
        target: playerEl,
        props: {
          events: replay.events,
          autoPlay: true,
          showController: true,
          width,
          height,
        },
      });
    } catch (e: unknown) {
      error = `No se pudo cargar el player: ${e instanceof Error ? e.message : String(e)}`;
    } finally {
      loading = false;
    }
  }

  onMount(load);
  onDestroy(() => {
    if (playerInstance) {
      try { playerInstance.$destroy(); } catch { /* ignora */ }
    }
  });

  function copySessionId(): void {
    if (!navigator.clipboard || !sessionId) return;
    void navigator.clipboard.writeText(sessionId);
  }
</script>

<div class="page-header">
  <h1 class="page-title">Session replay</h1>
  <div class="flex gap-12 center">
    <button on:click={load} disabled={loading}>{loading ? 'Cargando…' : '↻ Recargar'}</button>
  </div>
</div>

<div class="meta-row">
  <div>
    <span class="muted">Sesión</span>
    <code class="mono">{sessionId}</code>
    <button class="link-btn" on:click={copySessionId} title="Copiar al portapapeles">📋</button>
  </div>
  {#if replay}
    <div><span class="muted">Servicio</span> <strong>{replay.service_name}</strong></div>
    <div><span class="muted">Eventos</span> <strong>{replay.event_count.toLocaleString()}</strong></div>
    <div><span class="muted">Inicio</span> <span class="mono">{formatTimestamp(replay.start_ts)}</span></div>
    <div><span class="muted">Fin</span> <span class="mono">{formatTimestamp(replay.end_ts)}</span></div>
    {#if replay.user_id}
      <div><span class="muted">Usuario</span> <code class="mono">{replay.user_id}</code></div>
    {/if}
    {#if replay.page_url}
      <div class="url" title={replay.page_url}>
        <span class="muted">URL</span>
        <a href={replay.page_url} target="_blank" rel="noopener noreferrer">{replay.page_url}</a>
      </div>
    {/if}
  {/if}
</div>

{#if error}
  <div class="error-box">Error: {error}</div>
{/if}

<div class="player-frame">
  <div bind:this={playerEl} class="player-host"></div>
  {#if loading}
    <div class="player-loading"><span class="spinner"></span> Cargando reproducción…</div>
  {/if}
</div>

<style>
  .meta-row {
    display: flex;
    flex-wrap: wrap;
    gap: 18px;
    padding: 10px 14px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    margin-bottom: 12px;
    font-size: 13px;
    align-items: center;
  }
  .meta-row code {
    background: var(--bg);
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 12px;
  }
  .meta-row .url {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta-row .url a {
    max-width: 360px;
    display: inline-block;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    vertical-align: bottom;
  }
  .link-btn {
    background: transparent;
    border: 0;
    padding: 2px 4px;
    cursor: pointer;
    font-size: 12px;
  }

  .player-frame {
    position: relative;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
    min-height: 480px;
    padding: 8px;
  }
  .player-host {
    display: flex;
    justify-content: center;
    align-items: center;
    min-height: 460px;
  }
  /* rrweb-player monta su propio shadow DOM con tema claro; lo dejamos respirar */
  .player-host :global(.rr-player) {
    background: #ffffff;
    border-radius: 6px;
  }

  .player-loading {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    color: var(--text-muted);
    background: var(--bg-elev);
  }

  .error-box {
    color: var(--danger);
    padding: 12px 14px;
    border: 1px solid var(--danger);
    border-radius: 6px;
    margin-bottom: 12px;
    font-size: 13px;
  }
</style>
