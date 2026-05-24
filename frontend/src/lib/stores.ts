import { writable } from 'svelte/store';
import { browser } from '$app/environment';

import type { AuthUser } from './api';
export const currentUser = writable<AuthUser | null>(null);

export type RangePreset = '5m' | '15m' | '1h' | '6h' | '24h' | '7d';

const VALID_RANGES: readonly RangePreset[] = ['5m', '15m', '1h', '6h', '24h', '7d'];

export function isValidRange(s: string): s is RangePreset {
  return (VALID_RANGES as readonly string[]).includes(s);
}

// ---------- Estado global de exploración ----------
//
// Antes vivían en localStorage. Hoy son simplemente writables en memoria; la
// **persistencia entre máquinas** la da el backend (`faro.user_preferences`) y
// el **deep link** lo da el query string. Cuando una página quiere fijar el
// proyecto o el rango, escribe al store y la propia página/layout sincroniza
// con la URL. Defaults del usuario se hidratan al login en `+layout.svelte`.

/** Slug del proyecto seleccionado, o `''` para "todos". */
export const selectedProject = writable<string>('');

/** Preset de rango temporal activo en exploración. */
export const timeRange = writable<RangePreset>('1h');

/**
 * Lee `?project=` y `?range=` de la URL actual y los aplica a los stores,
 * **solo si la URL los trae**. Devuelve qué claves estaban presentes para
 * que el caller pueda decidir si todavía debe hidratar defaults del backend.
 */
export function applyGlobalUrlParams(): { hasProject: boolean; hasRange: boolean } {
  if (!browser) return { hasProject: false, hasRange: false };
  const p = new URLSearchParams(window.location.search);
  const proj = p.get('project');
  const range = p.get('range');
  if (proj !== null) selectedProject.set(proj);
  if (range && isValidRange(range)) timeRange.set(range);
  return { hasProject: proj !== null, hasRange: range !== null };
}

const presetMinutes: Record<RangePreset, number> = {
  '5m': 5,
  '15m': 15,
  '1h': 60,
  '6h': 360,
  '24h': 1440,
  '7d': 10080
};

export function rangeMinutes(p: RangePreset): number {
  return presetMinutes[p];
}

export function formatTimestamp(s: string): string {
  if (!s) return '';
  const d = new Date(s.includes('T') ? s : s.replace(' ', 'T') + 'Z');
  if (isNaN(d.getTime())) return s;
  return d.toLocaleString(undefined, {
    year: 'numeric', month: '2-digit', day: '2-digit',
    hour: '2-digit', minute: '2-digit', second: '2-digit', fractionalSecondDigits: 3
  } as Intl.DateTimeFormatOptions);
}

export function formatDuration(ns: number): string {
  if (!ns) return '0';
  if (ns < 1000) return `${ns}ns`;
  if (ns < 1_000_000) return `${(ns / 1000).toFixed(1)}µs`;
  if (ns < 1_000_000_000) return `${(ns / 1_000_000).toFixed(2)}ms`;
  return `${(ns / 1_000_000_000).toFixed(2)}s`;
}

export function severityClass(sev: string): string {
  const s = (sev || '').toUpperCase();
  if (s.startsWith('TRACE')) return 'trace';
  if (s.startsWith('DEBUG')) return 'debug';
  if (s.startsWith('INFO')) return 'info';
  if (s.startsWith('WARN')) return 'warn';
  if (s.startsWith('ERROR') || s === 'ERR') return 'error';
  if (s.startsWith('FATAL') || s.startsWith('CRIT')) return 'fatal';
  return 'info';
}
