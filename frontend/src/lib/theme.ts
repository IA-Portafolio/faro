/**
 * Gestión del tema claro/oscuro de la interfaz.
 *
 * `themeChoice` guarda la preferencia del usuario ('light' | 'dark' | 'system');
 * 'system' delega en `prefers-color-scheme` del SO. `resolvedTheme` es el valor
 * efectivo que se aplica al `<html>` (atributos `data-theme` y `color-scheme`).
 * Se persiste en localStorage y, si hay sesión, también en el backend
 * (`savePreferences`) para que el tema viaje entre dispositivos.
 */
import { writable, get } from 'svelte/store';
import { browser } from '$app/environment';
import { savePreferences, type ThemePref } from './api';

const THEME_KEY = 'faro:theme';

/** Preferencia del usuario: 'system' delega en `prefers-color-scheme`. */
export type ThemeChoice = ThemePref;

/** Resultado efectivo aplicado al DOM: siempre 'light' o 'dark'. */
export type ResolvedTheme = 'light' | 'dark';

function loadLocal(): ThemeChoice {
  if (!browser) return 'system';
  try {
    const v = window.localStorage.getItem(THEME_KEY);
    if (v === 'light' || v === 'dark' || v === 'system') return v;
  } catch {
    /* cuota / modo privado */
  }
  return 'system';
}

function systemPreference(): ResolvedTheme {
  if (!browser) return 'light';
  return window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
}

function resolve(choice: ThemeChoice): ResolvedTheme {
  return choice === 'system' ? systemPreference() : choice;
}

/** Aplica el tema al `<html>` y al meta `color-scheme`. */
function apply(choice: ThemeChoice): void {
  if (!browser) return;
  const resolved = resolve(choice);
  const html = document.documentElement;
  html.setAttribute('data-theme', resolved);
  html.style.colorScheme = resolved;
}

export const themeChoice = writable<ThemeChoice>(loadLocal());
export const resolvedTheme = writable<ResolvedTheme>(resolve(loadLocal()));

if (browser) {
  // Sincroniza store ↔ DOM ↔ localStorage.
  themeChoice.subscribe((v) => {
    try {
      window.localStorage.setItem(THEME_KEY, v);
    } catch {
      /* ignora */
    }
    apply(v);
    resolvedTheme.set(resolve(v));
  });

  // Si el usuario eligió 'system', responde a cambios del SO en vivo.
  const mq = window.matchMedia('(prefers-color-scheme: dark)');
  const onChange = (): void => {
    if (get(themeChoice) === 'system') {
      apply('system');
      resolvedTheme.set(systemPreference());
    }
  };
  if (typeof mq.addEventListener === 'function') mq.addEventListener('change', onChange);
  else mq.addListener(onChange);
}

/**
 * Cambia la preferencia y la persiste en backend.
 * Si la llamada al backend falla, el cambio local se mantiene — el server
 * sincronizará en la siguiente sesión cuando vuelva a estar disponible.
 */
export async function setTheme(v: ThemeChoice, opts: { persist?: boolean } = {}): Promise<void> {
  themeChoice.set(v);
  if (opts.persist !== false) {
    try {
      await savePreferences({ theme: v });
    } catch {
      /* silencioso: la próxima sesión retomará desde localStorage */
    }
  }
}

/** Aplica el tema inicial sin esperar al backend — evita flash de tema incorrecto. */
export function bootstrapThemeFromLocal(): void {
  apply(loadLocal());
}

/**
 * Hidrata el store desde la preferencia guardada en backend tras login.
 * Solo sobreescribe si difiere de lo local — y persiste a localStorage.
 */
export function hydrateFromServer(server: ThemeChoice): void {
  const local = loadLocal();
  if (server !== local) {
    themeChoice.set(server);
  }
}
