/**
 * Helpers para sincronizar filtros de página con el query string.
 *
 * Cada página de exploración (logs, traces, errors, metrics) llama a:
 *   - `readFilters({...})` al montar para hidratar sus locales con los valores
 *     que vinieron en la URL (los que no estén, mantienen el default local).
 *   - `writeFilters({...})` cuando cambia un filtro, para que un refresh o un
 *     paste-del-enlace reconstruyan exactamente la misma vista.
 *
 * Convenciones:
 *   - Los valores vacíos (`''`, `0`, `undefined`, `null`) se omiten del query
 *     string para que las URLs queden limpias.
 *   - El proyecto y el rango global se llaman `project` y `range` en la URL,
 *     **compartidos** entre páginas para que la barra lateral los respete.
 */

import { browser } from '$app/environment';

export type FilterValue = string | number | boolean | undefined | null;

export function buildSearch(values: Record<string, FilterValue>): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(values)) {
    if (v === undefined || v === null) continue;
    if (typeof v === 'string') {
      if (v === '') continue;
      u.set(k, v);
    } else if (typeof v === 'number') {
      if (!Number.isFinite(v) || v === 0) continue;
      u.set(k, String(v));
    } else if (typeof v === 'boolean') {
      if (!v) continue;
      u.set(k, '1');
    }
  }
  return u.toString();
}

/**
 * Sustituye el query string actual por el resultado de serializar `values`,
 * conservando `pathname` y `hash`. No emite navegación, solo `replaceState`,
 * para no romper el back-stack del navegador al teclear en un input.
 */
export function writeFilters(values: Record<string, FilterValue>): void {
  if (!browser) return;
  const qs = buildSearch(values);
  const url = window.location.pathname + (qs ? `?${qs}` : '') + window.location.hash;
  try {
    window.history.replaceState(null, '', url);
  } catch {
    /* no bloqueante: algunos navegadores rechazan replaceState bajo carga */
  }
}

/** Lee del query string y devuelve un objeto con los valores presentes. */
export function readFilters(keys: readonly string[]): Record<string, string> {
  if (!browser) return {};
  const p = new URLSearchParams(window.location.search);
  const out: Record<string, string> = {};
  for (const k of keys) {
    const v = p.get(k);
    if (v !== null) out[k] = v;
  }
  return out;
}

/** Convierte un valor de query string a number con default seguro. */
export function asNumber(v: string | undefined, def = 0): number {
  if (v === undefined) return def;
  const n = Number(v);
  return Number.isFinite(n) ? n : def;
}
