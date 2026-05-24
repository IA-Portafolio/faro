import { writable } from 'svelte/store';

/**
 * Devuelve `true` si el evento ocurre mientras el usuario está escribiendo en
 * un input/textarea/select/contenteditable. Los atajos globales (g+x, /, ?)
 * deben respetar esto para no robarle teclas al usuario.
 */
export function isTyping(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  if (el.isContentEditable) return true;
  const tag = el.tagName;
  if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') return true;
  // Cualquier descendiente de un contenedor `role="textbox"`.
  return Boolean(el.closest?.('[role="textbox"]'));
}

/** Selector que la página puede poner en el input que debe enfocar la tecla `/`. */
export const SEARCH_INPUT_ATTR = 'data-search-input';

/** Enfoca el primer elemento marcado con `[data-search-input]` en el documento. */
export function focusPageSearch(): boolean {
  const el = document.querySelector<HTMLInputElement>(`[${SEARCH_INPUT_ATTR}]`);
  if (!el) return false;
  el.focus();
  // Si tiene contenido, lo seleccionamos para reemplazar rápido.
  if (typeof el.select === 'function') el.select();
  return true;
}

/** Stores globales para overlays de teclado. */
export const paletteOpen = writable(false);
export const helpOpen = writable(false);

/**
 * Suscribe un handler a una secuencia con tecla líder. Por ejemplo:
 *   sequence({ leader: 'g', map: { l: () => goto('/logs') } })
 * permite presionar `g` seguido de `l` dentro de `windowMs` ms.
 *
 * Devuelve una función `dispose()` para desregistrar.
 */
export function sequence(opts: {
  leader: string;
  map: Record<string, () => void>;
  windowMs?: number;
  /** Llamado cuando entramos en estado armado (tras pulsar la tecla líder). */
  onArm?: () => void;
  onDisarm?: () => void;
}): () => void {
  const windowMs = opts.windowMs ?? 1200;
  let armed = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  function disarm(): void {
    if (!armed) return;
    armed = false;
    if (timer) clearTimeout(timer);
    timer = null;
    opts.onDisarm?.();
  }

  function onKey(e: KeyboardEvent): void {
    if (e.defaultPrevented) return;
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    if (isTyping(e.target)) return;

    if (!armed) {
      if (e.key === opts.leader) {
        armed = true;
        opts.onArm?.();
        timer = setTimeout(disarm, windowMs);
        e.preventDefault();
      }
      return;
    }

    // Estado armado: cualquier tecla resuelve o cancela la secuencia.
    const handler = opts.map[e.key];
    disarm();
    if (handler) {
      e.preventDefault();
      handler();
    }
  }

  window.addEventListener('keydown', onKey);
  return () => window.removeEventListener('keydown', onKey);
}
