import { writable } from 'svelte/store';
import { browser } from '$app/environment';

import type { AuthUser } from './api';
export const currentUser = writable<AuthUser | null>(null);

export type RangePreset = '5m' | '15m' | '1h' | '6h' | '24h' | '7d';

const PROJECT_KEY = 'faro:selectedProject';

function loadProject(): string {
  if (!browser) return '';
  try {
    return window.localStorage.getItem(PROJECT_KEY) ?? '';
  } catch {
    return '';
  }
}

export const selectedProject = writable<string>(loadProject());

if (browser) {
  selectedProject.subscribe((v) => {
    try {
      if (v) window.localStorage.setItem(PROJECT_KEY, v);
      else window.localStorage.removeItem(PROJECT_KEY);
    } catch {
      // ignora errores de cuota
    }
  });
}

const presetMinutes: Record<RangePreset, number> = {
  '5m': 5,
  '15m': 15,
  '1h': 60,
  '6h': 360,
  '24h': 1440,
  '7d': 10080
};

export const timeRange = writable<RangePreset>('1h');

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
