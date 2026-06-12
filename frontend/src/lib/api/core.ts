/**
 * Cliente HTTP único contra el backend de Faro.
 *
 * `api<T>()` es el wrapper sobre `fetch`: resuelve la base URL (variable
 * `PUBLIC_API_BASE`, o el host actual en el puerto 8080), manda la cookie de
 * sesión (`credentials: 'include'`) y centraliza el manejo del 401 → redirige a
 * `/login` salvo en rutas públicas (`/login`, `/docs`).
 */
import { env as publicEnv } from '$env/dynamic/public';

function base(): string {
  const fromEnv = publicEnv.PUBLIC_API_BASE;
  if (fromEnv) return fromEnv.replace(/\/$/, '');
  if (typeof window !== 'undefined') {
    return `${window.location.protocol}//${window.location.hostname}:8080`;
  }
  return 'http://localhost:8080';
}

export type RangeArgs = {
  from?: string;
  to?: string;
  last_minutes?: number;
  limit?: number;
  /** Cursor keyset: timestamp del último item de la página anterior. El backend
   *  filtra `WHERE <column> < cursor` antes del LIMIT, sin escanear las páginas
   *  saltadas como hacía el viejo `offset`. */
  cursor?: string;
  project?: string;
};

export function qs(params: Record<string, unknown>): string {
  const u = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v === undefined || v === null || v === '') continue;
    u.set(k, String(v));
  }
  const s = u.toString();
  return s ? `?${s}` : '';
}

export class UnauthorizedError extends Error {
  constructor() {
    super('unauthorized');
  }
}

export async function api<T>(path: string, init?: RequestInit): Promise<T> {
  const res = await fetch(`${base()}${path}`, {
    credentials: 'include',
    ...init,
    headers: { 'Content-Type': 'application/json', ...(init?.headers || {}) }
  });
  if (res.status === 401) {
    // En rutas públicas (login y la doc pública /docs) un 401 es esperado y
    // NO debe forzar el redirect a /login: la página se renderiza en modo
    // anónimo y cada caller maneja el fallo (p. ej. el sidebar muestra vacío).
    const p = typeof window !== 'undefined' ? window.location.pathname : '';
    const onPublic = p.startsWith('/login') || p === '/docs' || p.startsWith('/docs/');
    if (typeof window !== 'undefined' && !onPublic) {
      window.location.assign('/login?next=' + encodeURIComponent(p));
    }
    throw new UnauthorizedError();
  }
  if (!res.ok) {
    const txt = await res.text();
    throw new Error(`HTTP ${res.status}: ${txt}`);
  }
  return res.json() as Promise<T>;
}

export const apiBase = base;
