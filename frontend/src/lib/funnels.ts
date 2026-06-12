/**
 * Helpers compartidos por las subsecciones de `/funnels`.
 *
 * Se mantienen puros y sin estado para que las subsecciones (catalog,
 * builder, results) los importen sin acoplarse entre sí.
 */

import type { EventCandidate } from '$lib/api';

/** Vista del panel expandido por paso. */
export type StepView = 'dropoff' | 'timing';

/** Formatea un ratio ∈ [0, 1] como porcentaje con 1 decimal. */
export function fmtPct(p: number): string {
  if (!isFinite(p)) return '–';
  return `${(p * 100).toFixed(1)}%`;
}

/** Formatea un conteo grande: 1.2k / 3.4M / 1,234. */
export function fmtCount(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return n.toLocaleString();
}

/** Formatea un delta de segundos como 1d / 2h / 30m / 45s. */
export function fmtSeconds(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.round(s / 60)}m`;
  if (s < 86_400) return `${Math.round(s / 3600)}h`;
  if (s < 30 * 86_400) return `${Math.round(s / 86_400)}d`;
  return `${Math.round(s / (30 * 86_400))}mo`;
}

/** Formatea un rango de bins de histograma (lower exclusivo, upper inclusivo). */
export function fmtSecondsRange(lower: number, upper: number | null): string {
  const lo = fmtSeconds(lower);
  if (upper === null) return `> ${lo}`;
  return `${lo} – ${fmtSeconds(upper)}`;
}

/** Filtra el catálogo por substring case-insensitive sobre `name`. */
export function filterCatalog(catalog: EventCandidate[], filter: string): EventCandidate[] {
  const t = filter.trim().toLowerCase();
  return t ? catalog.filter((e) => e.name.toLowerCase().includes(t)) : catalog;
}

/** Presets para la ventana de conversión del funnel. */
export const windowPresets: { label: string; seconds: number }[] = [
  { label: '5 min', seconds: 300 },
  { label: '1 hora', seconds: 3600 },
  { label: '1 día', seconds: 86_400 },
  { label: '7 días', seconds: 604_800 },
  { label: '30 días', seconds: 2_592_000 }
];

/** Presets para el look-ahead de drop-off. */
export const lookaheadPresets: { label: string; seconds: number }[] = [
  { label: '1 min', seconds: 60 },
  { label: '5 min', seconds: 300 },
  { label: '15 min', seconds: 900 },
  { label: '1 hora', seconds: 3600 }
];

/** Presets para el tope de la ventana de time-to-convert. */
export const timingMaxPresets: { label: string; seconds: number }[] = [
  { label: '1 hora', seconds: 3600 },
  { label: '1 día', seconds: 86_400 },
  { label: '7 días', seconds: 604_800 },
  { label: '30 días', seconds: 2_592_000 },
  { label: '90 días', seconds: 90 * 86_400 }
];
