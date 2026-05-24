/**
 * Sistema global de notificaciones (toasts).
 *
 * Un solo store (`toasts`) con la cola activa y un objeto `toast` con la
 * API ergonómica (`toast.success(...)`, `toast.error(...)`). El componente
 * `Toasts.svelte`, montado en el layout raíz, lee la cola y renderiza el
 * stack — así cualquier handler puede mostrar feedback sin importar nada
 * más que esta API.
 *
 * Decisiones:
 *   - **Auto-dismiss** por defecto a 4 s para info/success/warning,
 *     6 s para errores (que quieres leer con tiempo).
 *   - `duration: 0` deja el toast hasta que el usuario lo cierre.
 *   - **Acción opcional** (`label` + `run`): por ejemplo "Deshacer".
 *     Click ejecuta el callback y descarta el toast.
 *   - **Sin dedupe agresivo**: si el usuario hace click 3 veces, ve 3 toasts.
 *     Si quieres dedupear, llama `toast.dismissAll()` antes.
 */

import { writable } from 'svelte/store';

export type ToastKind = 'success' | 'error' | 'info' | 'warning';

export type ToastAction = {
  label: string;
  run: () => void | Promise<void>;
};

export type Toast = {
  id: number;
  kind: ToastKind;
  message: string;
  /** Texto secundario opcional (detalle, ID afectado, etc.). */
  description?: string;
  /** ms hasta que se descarte solo. `0` = pegajoso. */
  duration: number;
  action?: ToastAction;
  /** Marca de tiempo en que se mostró — usado para sort estable. */
  createdAt: number;
};

export type ShowOptions = {
  message: string;
  description?: string;
  /** ms hasta auto-dismiss. `0` = pegajoso. Si se omite, default por `kind`. */
  duration?: number;
  action?: ToastAction;
  kind?: ToastKind;
};

const DEFAULT_DURATIONS: Record<ToastKind, number> = {
  success: 4000,
  info: 4000,
  warning: 5000,
  error: 6000
};

export const toasts = writable<Toast[]>([]);

let nextId = 1;
const timers = new Map<number, ReturnType<typeof setTimeout>>();

function schedule(t: Toast): void {
  if (t.duration <= 0) return;
  const handle = setTimeout(() => dismiss(t.id), t.duration);
  timers.set(t.id, handle);
}

function clearTimer(id: number): void {
  const h = timers.get(id);
  if (h) {
    clearTimeout(h);
    timers.delete(id);
  }
}

export function dismiss(id: number): void {
  clearTimer(id);
  toasts.update((list) => list.filter((t) => t.id !== id));
}

export function dismissAll(): void {
  for (const id of timers.keys()) clearTimer(id);
  toasts.set([]);
}

function show(input: ShowOptions): number {
  const kind: ToastKind = input.kind ?? 'info';
  const t: Toast = {
    id: nextId++,
    kind,
    message: input.message,
    description: input.description,
    duration: input.duration ?? DEFAULT_DURATIONS[kind],
    action: input.action,
    createdAt: Date.now()
  };
  toasts.update((list) => [t, ...list].slice(0, 8)); // tope: 8 a la vez
  schedule(t);
  return t.id;
}

/**
 * Detiene el auto-dismiss de un toast (p. ej. el usuario pasa el cursor por
 * encima) y permite reprogramarlo cuando se va.
 */
export function pauseDismiss(id: number): void {
  clearTimer(id);
}
export function resumeDismiss(id: number): void {
  // Re-leemos la duración actual del store para que un toast con acción que
  // se resetea el contador funcione predeciblemente.
  let t: Toast | undefined;
  toasts.update((list) => {
    t = list.find((x) => x.id === id);
    return list;
  });
  if (t) schedule(t);
}

/**
 * API pública. Pensada para call sites concisos:
 *   `toast.success('Token rotado')`
 *   `toast.error('No se pudo guardar', { description: e.message })`
 */
export const toast = {
  show,
  dismiss,
  dismissAll,
  success: (message: string, opts: Partial<ShowOptions> = {}) =>
    show({ ...opts, kind: 'success', message }),
  error: (message: string, opts: Partial<ShowOptions> = {}) =>
    show({ ...opts, kind: 'error', message }),
  info: (message: string, opts: Partial<ShowOptions> = {}) =>
    show({ ...opts, kind: 'info', message }),
  warning: (message: string, opts: Partial<ShowOptions> = {}) =>
    show({ ...opts, kind: 'warning', message }),
  /**
   * Helper común: convierte una excepción capturada en un toast de error
   * mostrando su mensaje (o el string si no es Error).
   */
  fromError(prefix: string, err: unknown): number {
    const description = err instanceof Error ? err.message : String(err);
    return show({ kind: 'error', message: prefix, description });
  }
};
