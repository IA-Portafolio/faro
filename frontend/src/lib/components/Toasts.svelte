<script lang="ts">
  /**
   * Stack global de toasts. Lee del store `toasts` y renderiza una pila
   * fija arriba-derecha (abajo-centro en móvil). Cada toast tiene:
   *   - icono según kind
   *   - mensaje + descripción opcional
   *   - acción opcional (botón a la derecha)
   *   - botón ✕ de cerrar
   *
   * Hover pausa el auto-dismiss para que el usuario pueda leer un toast
   * que aparece justo cuando va a cliquearlo.
   */
  import { fly } from 'svelte/transition';
  import {
    dismiss,
    pauseDismiss,
    resumeDismiss,
    toasts,
    type Toast
  } from '$lib/toasts';

  const ICONS: Record<Toast['kind'], string> = {
    success: '✓',
    error: '⚠',
    info: 'i',
    warning: '!'
  };
  const LABELS: Record<Toast['kind'], string> = {
    success: 'Éxito',
    error: 'Error',
    info: 'Información',
    warning: 'Aviso'
  };

  async function runAction(t: Toast): Promise<void> {
    if (!t.action) return;
    try {
      await t.action.run();
    } catch (e) {
      console.error('acción de toast falló:', e);
    } finally {
      dismiss(t.id);
    }
  }
</script>

<div class="ts-stack" aria-live="polite" aria-atomic="false">
  {#each $toasts as t (t.id)}
    <div
      class="ts-item ts-{t.kind}"
      role={t.kind === 'error' ? 'alert' : 'status'}
      transition:fly|local={{ x: 320, duration: 180 }}
      on:mouseenter={() => pauseDismiss(t.id)}
      on:mouseleave={() => resumeDismiss(t.id)}
    >
      <span class="ts-icon" aria-label={LABELS[t.kind]}>{ICONS[t.kind]}</span>
      <div class="ts-body">
        <div class="ts-msg">{t.message}</div>
        {#if t.description}
          <div class="ts-desc">{t.description}</div>
        {/if}
      </div>
      {#if t.action}
        <button type="button" class="ts-action" on:click={() => runAction(t)}>
          {t.action.label}
        </button>
      {/if}
      <button
        type="button"
        class="ts-close"
        aria-label="Cerrar notificación"
        on:click={() => dismiss(t.id)}
      >×</button>
    </div>
  {/each}
</div>

<style>
  .ts-stack {
    position: fixed;
    top: 16px;
    right: 16px;
    z-index: 400;  /* por encima de drawers (100), paleta (300) */
    display: flex;
    flex-direction: column;
    gap: 8px;
    max-width: min(420px, calc(100vw - 32px));
    pointer-events: none;  /* solo los items capturan eventos */
  }
  .ts-item {
    pointer-events: auto;
    display: grid;
    grid-template-columns: 22px 1fr auto auto;
    gap: 10px;
    align-items: start;
    padding: 10px 12px;
    background: var(--bg-elev);
    border: 1px solid var(--border);
    border-left-width: 3px;
    border-radius: 6px;
    box-shadow: 0 10px 30px rgba(0, 0, 0, 0.28);
    font-size: 13px;
    line-height: 1.35;
  }
  .ts-success { border-left-color: var(--success); }
  .ts-error   { border-left-color: var(--danger); }
  .ts-info    { border-left-color: var(--info); }
  .ts-warning { border-left-color: var(--warn); }

  .ts-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    font-weight: 700;
    font-size: 12px;
    line-height: 1;
    font-family: -apple-system, "Segoe UI", sans-serif;
  }
  .ts-success .ts-icon { background: var(--badge-ok-bg); color: var(--success); }
  .ts-error   .ts-icon { background: var(--badge-error-bg); color: var(--danger); }
  .ts-info    .ts-icon { background: var(--badge-info-bg); color: var(--info); font-style: italic; }
  .ts-warning .ts-icon { background: var(--badge-warn-bg); color: var(--warn); }

  .ts-body { min-width: 0; }
  .ts-msg {
    font-weight: 600;
    color: var(--text);
    overflow-wrap: anywhere;
  }
  .ts-desc {
    margin-top: 2px;
    color: var(--text-muted);
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .ts-action {
    align-self: center;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    padding: 3px 10px;
    font-size: 12px;
    color: var(--text);
    cursor: pointer;
  }
  .ts-action:hover { background: var(--bg-hover); }

  .ts-close {
    align-self: start;
    background: transparent;
    border: 0;
    padding: 0 4px;
    font-size: 16px;
    line-height: 1;
    color: var(--text-muted);
    cursor: pointer;
  }
  .ts-close:hover { color: var(--text); }

  @media (max-width: 640px) {
    .ts-stack {
      top: auto;
      bottom: 16px;
      right: 16px;
      left: 16px;
      max-width: none;
    }
  }
</style>
