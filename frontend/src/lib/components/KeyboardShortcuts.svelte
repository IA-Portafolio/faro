<script lang="ts">
  /**
   * Manejador global de teclado (componente sin UI; se monta una vez en el layout).
   *
   * Registra la secuencia estilo Vim `g`+tecla para navegar (g l → /logs, g t →
   * /traces…), el toggle de la paleta con ⌘K/Ctrl+K, `?` para la ayuda y Escape
   * para cerrar overlays globales. La maquinaria de secuencias y el guard de
   * "estás escribiendo en un input" viven en `$lib/keyboard`.
   */
  import { onMount, onDestroy } from 'svelte';
  import { goto } from '$app/navigation';
  import {
    focusPageSearch,
    helpOpen,
    isTyping,
    paletteOpen,
    sequence
  } from '$lib/keyboard';

  let armed = false;
  let disposers: Array<() => void> = [];

  onMount(() => {
    // Secuencia `g X` para navegación rápida.
    disposers.push(
      sequence({
        leader: 'g',
        map: {
          r: () => goto('/'),
          l: () => goto('/logs'),
          t: () => goto('/traces'),
          m: () => goto('/metrics'),
          e: () => goto('/errors'),
          o: () => goto('/monitors'),
          s: () => goto('/settings'),
          a: () => goto('/settings/alerts'),
          p: () => goto('/settings/projects'),
          u: () => goto('/settings/users'),
          i: () => goto('/settings/integrations')
        },
        onArm: () => (armed = true),
        onDisarm: () => (armed = false)
      })
    );

    const onKey = (e: KeyboardEvent): void => {
      // Cmd+K / Ctrl+K abre la paleta — incluso desde dentro de un input,
      // porque es un atajo "absoluto" con modificador.
      if ((e.metaKey || e.ctrlKey) && (e.key === 'k' || e.key === 'K')) {
        e.preventDefault();
        paletteOpen.update((v) => !v);
        return;
      }

      // Escape cierra paleta y ayuda. Los drawers/modals locales suelen
      // tener su propio handler — esto solo se ocupa de los globales.
      if (e.key === 'Escape') {
        let consumed = false;
        paletteOpen.update((v) => {
          if (v) consumed = true;
          return false;
        });
        helpOpen.update((v) => {
          if (v) consumed = true;
          return false;
        });
        if (consumed) e.preventDefault();
        return;
      }

      // Los atajos sin modificador no deben dispararse mientras se escribe.
      if (e.ctrlKey || e.metaKey || e.altKey) return;
      if (isTyping(e.target)) return;

      // `?` (shift+/) abre la ayuda de teclado.
      if (e.key === '?') {
        e.preventDefault();
        helpOpen.set(true);
        return;
      }

      // `/` enfoca el campo de búsqueda de la página actual, si existe.
      if (e.key === '/') {
        if (focusPageSearch()) {
          e.preventDefault();
        }
        return;
      }
    };

    window.addEventListener('keydown', onKey);
    disposers.push(() => window.removeEventListener('keydown', onKey));
  });

  onDestroy(() => {
    for (const d of disposers) d();
    disposers = [];
  });
</script>

{#if armed}
  <div class="leader-hint mono" role="status" aria-live="polite">
    <span><kbd>g</kbd> · espera tecla…</span>
  </div>
{/if}

<style>
  .leader-hint {
    position: fixed;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    background: var(--bg-elev);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: 999px;
    padding: 6px 12px;
    font-size: 12px;
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.3);
    z-index: 250;
    display: inline-flex;
    align-items: center;
    gap: 6px;
  }
  .leader-hint kbd {
    font-family: inherit;
    border: 1px solid var(--border);
    background: var(--bg);
    padding: 1px 6px;
    border-radius: 4px;
    font-size: 11px;
  }
</style>
