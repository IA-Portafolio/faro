import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// Stub mutable de $env/dynamic/public (alias en vitest.config.ts): fijamos la
// base para que las URLs no dependan de window.
import { env } from '$env/dynamic/public';

import {
  fetchSessions,
  fetchTwoFaStatus,
  login,
  loginTotp,
  logout,
  me,
  revokeOtherSessions,
  twoFaDisable,
  twoFaEnable,
  twoFaRegenRecovery,
  twoFaSetup
} from './auth';

const fetchMock = vi.fn();

beforeEach(() => {
  env.PUBLIC_API_BASE = 'http://api.test';
  fetchMock.mockReset();
  // Response fresca por llamada: el body solo se puede consumir una vez.
  fetchMock.mockImplementation(() =>
    Promise.resolve(new Response(JSON.stringify({ ok: true }), { status: 200 }))
  );
  vi.stubGlobal('fetch', fetchMock);
});

afterEach(() => {
  vi.unstubAllGlobals();
  // Cast: el ambient type de SvelteKit declara los valores como no-opcionales,
  // pero el stub de tests es un Record mutable.
  delete (env as Record<string, string | undefined>).PUBLIC_API_BASE;
});

/** Desarma la última llamada capturada por el mock de fetch: path exacto,
 *  método HTTP y body JSON ya parseado (undefined si no mandó body). */
function lastCall() {
  const call = fetchMock.mock.calls.at(-1) as [string, RequestInit];
  const [url, init] = call;
  return {
    path: new URL(url).pathname,
    method: init.method,
    body: typeof init.body === 'string' ? JSON.parse(init.body) : undefined
  };
}

describe('wrappers de auth', () => {
  it('login: POST /api/v1/auth/login con email y password', async () => {
    await login({ email: 'ana@faro.dev', password: 'secreta' });
    expect(lastCall()).toEqual({
      path: '/api/v1/auth/login',
      method: 'POST',
      body: { email: 'ana@faro.dev', password: 'secreta' }
    });
  });

  it('loginTotp: POST /api/v1/auth/login/2fa con challenge + código', async () => {
    await loginTotp({ challenge_token: 'ct-123', code: '654321', recovery: true });
    expect(lastCall()).toEqual({
      path: '/api/v1/auth/login/2fa',
      method: 'POST',
      body: { challenge_token: 'ct-123', code: '654321', recovery: true }
    });
  });

  it('logout: POST /api/v1/auth/logout sin body', async () => {
    await logout();
    expect(lastCall()).toEqual({
      path: '/api/v1/auth/logout',
      method: 'POST',
      body: undefined
    });
  });

  it('me: GET /api/v1/auth/me', async () => {
    await me();
    expect(lastCall()).toEqual({
      path: '/api/v1/auth/me',
      method: undefined, // GET implícito de fetch
      body: undefined
    });
  });

  it('fetchSessions: GET /api/v1/me/sessions', async () => {
    await fetchSessions();
    expect(lastCall()).toEqual({
      path: '/api/v1/me/sessions',
      method: undefined,
      body: undefined
    });
  });

  it('revokeOtherSessions: POST /api/v1/me/sessions/revoke-others sin body', async () => {
    await revokeOtherSessions();
    expect(lastCall()).toEqual({
      path: '/api/v1/me/sessions/revoke-others',
      method: 'POST',
      body: undefined
    });
  });

  it('fetchTwoFaStatus: GET /api/v1/me/security/2fa', async () => {
    await fetchTwoFaStatus();
    expect(lastCall()).toEqual({
      path: '/api/v1/me/security/2fa',
      method: undefined,
      body: undefined
    });
  });

  it('twoFaSetup: POST /api/v1/me/security/2fa/setup sin body', async () => {
    await twoFaSetup();
    expect(lastCall()).toEqual({
      path: '/api/v1/me/security/2fa/setup',
      method: 'POST',
      body: undefined
    });
  });

  it('twoFaEnable: POST /api/v1/me/security/2fa/enable con el código envuelto', async () => {
    await twoFaEnable('123456');
    expect(lastCall()).toEqual({
      path: '/api/v1/me/security/2fa/enable',
      method: 'POST',
      body: { code: '123456' }
    });
  });

  it('twoFaDisable: POST /api/v1/me/security/2fa/disable con password + código', async () => {
    await twoFaDisable({ password: 'secreta', code: '123456' });
    expect(lastCall()).toEqual({
      path: '/api/v1/me/security/2fa/disable',
      method: 'POST',
      body: { password: 'secreta', code: '123456' }
    });
  });

  it('twoFaRegenRecovery: POST /api/v1/me/security/2fa/recovery-codes', async () => {
    await twoFaRegenRecovery({ password: 'secreta', code: '654321' });
    expect(lastCall()).toEqual({
      path: '/api/v1/me/security/2fa/recovery-codes',
      method: 'POST',
      body: { password: 'secreta', code: '654321' }
    });
  });
});
