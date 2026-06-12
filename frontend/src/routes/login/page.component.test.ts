/**
 * Component tests de la página /login (proyecto "components" de vitest:
 * jsdom + @testing-library/svelte, ver vitest.config.ts).
 *
 * Cubre los tres flujos del form: login simple → goto(next), login con 2FA
 * (needs_totp → fase TOTP → goto), y credenciales inválidas (401 → mensaje).
 * El backend se stubea ruteando `fetch` por pathname; la navegación se
 * inspecciona vía `lastGoto` del stub de `$app/navigation`.
 */
import { fireEvent, render, screen, waitFor } from '@testing-library/svelte';
import { get } from 'svelte/store';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Import relativo a propósito: el alias `$app/navigation` de vitest.config.ts
// apunta a este mismo archivo, así que `lastGoto` es la MISMA instancia que ve
// la página al hacer goto().
import { lastGoto } from '../../lib/__mocks__/app-navigation';
import { currentUser } from '$lib/stores';
import type { AuthUser } from '$lib/api';

import LoginPage from './+page.svelte';

const ANA: AuthUser = { id: 'u-1', email: 'ana@faro.dev', name: 'Ana', role: 'admin' };

const fetchMock = vi.fn();

/** Respuesta JSON estilo backend: body serializado + status. */
function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'Content-Type': 'application/json' }
  });
}

/** Stub de fetch ruteado por pathname. Cada handler devuelve una Response
 *  fresca por llamada (el body de una Response solo se puede consumir una vez). */
function routeFetch(routes: Record<string, (init?: RequestInit) => Response>): void {
  fetchMock.mockImplementation((input: RequestInfo | URL, init?: RequestInit) => {
    const path = new URL(String(input)).pathname;
    const handler = routes[path];
    if (!handler) {
      return Promise.reject(new Error(`fetch sin ruta stubeada en el test: ${path}`));
    }
    return Promise.resolve(handler(init));
  });
}

/** Completa email + contraseña y manda el form de fase 1. */
async function submitCredentials(): Promise<void> {
  // findBy*: espera a que el form esté montado (onMount dispara me() → 401).
  const email = await screen.findByLabelText('Email');
  const password = screen.getByLabelText('Contraseña');
  await fireEvent.input(email, { target: { value: 'ana@faro.dev' } });
  await fireEvent.input(password, { target: { value: 'secreta' } });
  await fireEvent.click(screen.getByRole('button', { name: 'Iniciar sesión' }));
}

beforeEach(() => {
  // URL ANTES del render: /login es ruta pública para api(), así que el 401 de
  // me() no fuerza window.location.assign; y `?next=` queda visible en $page.
  window.history.replaceState({}, '', '/login?next=/traces');
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  lastGoto.url = null;
  currentUser.set(null);
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('/login', () => {
  it('login simple: credenciales válidas → goto a safeNext(?next=)', async () => {
    routeFetch({
      '/api/v1/auth/me': () => json({ error: 'no session' }, 401),
      '/api/v1/auth/login': () => json(ANA)
    });

    render(LoginPage);
    await submitCredentials();

    await waitFor(() => expect(lastGoto.url).toBe('/traces'));
    expect(get(currentUser)).toEqual(ANA);
  });

  it('flujo TOTP: needs_totp → fase 2 → loginTotp con el challenge_token → goto', async () => {
    routeFetch({
      '/api/v1/auth/me': () => json({ error: 'no session' }, 401),
      '/api/v1/auth/login': () =>
        json({ needs_totp: true, challenge_token: 'tok', expires_in_secs: 300 }),
      '/api/v1/auth/login/2fa': () => json(ANA)
    });

    render(LoginPage);
    await submitCredentials();

    // Fase 2: aparece el form de verificación con el input de código.
    await screen.findByText('Verificación en dos pasos');
    const code = screen.getByLabelText('Código TOTP');
    await fireEvent.input(code, { target: { value: '123456' } });
    await fireEvent.click(screen.getByRole('button', { name: 'Verificar' }));

    await waitFor(() => expect(lastGoto.url).toBe('/traces'));
    expect(get(currentUser)).toEqual(ANA);

    // El POST a /2fa tiene que viajar con el challenge_token que dio fase 1.
    const totpCall = fetchMock.mock.calls.find(([url]) =>
      String(url).includes('/api/v1/auth/login/2fa')
    );
    expect(totpCall).toBeDefined();
    const body = JSON.parse((totpCall![1] as RequestInit).body as string);
    expect(body).toMatchObject({ challenge_token: 'tok', code: '123456' });
  });

  it('credenciales inválidas: 401 en login → mensaje de error y sin navegación', async () => {
    routeFetch({
      '/api/v1/auth/me': () => json({ error: 'no session' }, 401),
      '/api/v1/auth/login': () => json({ error: 'bad credentials' }, 401)
    });

    render(LoginPage);
    await submitCredentials();

    await screen.findByText('Email o contraseña incorrectos');
    expect(lastGoto.url).toBeNull();
    expect(get(currentUser)).toBeNull();
  });
});
