import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

// El alias de vitest.config.ts resuelve esto al stub de __mocks__, cuyo `env`
// es un objeto mutable: seteamos/borramos claves por test.
import { env } from '$env/dynamic/public';

import { api, apiBase, qs, UnauthorizedError } from './core';

afterEach(() => {
  vi.unstubAllGlobals();
  // Cast: el ambient type de SvelteKit declara los valores como no-opcionales,
  // pero el stub de tests es un Record mutable.
  delete (env as Record<string, string | undefined>).PUBLIC_API_BASE;
});

describe('qs', () => {
  it('omite undefined, null y string vacío pero conserva 0 y false', () => {
    expect(qs({ a: undefined, b: null, c: '', d: 0, e: false })).toBe('?d=0&e=false');
  });

  it('devuelve string vacío cuando no queda ningún param', () => {
    expect(qs({})).toBe('');
    expect(qs({ a: undefined, b: null, c: '' })).toBe('');
  });

  it('prefija con ? y stringifica los valores', () => {
    expect(qs({ limit: 50, project: 'demo' })).toBe('?limit=50&project=demo');
  });

  it('escapea los valores que necesitan encoding', () => {
    expect(qs({ q: 'a b&c' })).toBe('?q=a+b%26c');
  });
});

describe('apiBase', () => {
  it('usa PUBLIC_API_BASE cuando está seteada', () => {
    env.PUBLIC_API_BASE = 'https://api.faro.dev';
    expect(apiBase()).toBe('https://api.faro.dev');
  });

  it('strippea el trailing slash de PUBLIC_API_BASE', () => {
    env.PUBLIC_API_BASE = 'https://api.faro.dev/';
    expect(apiBase()).toBe('https://api.faro.dev');
  });

  it('sin env cae al host actual en el puerto 8080 cuando hay window', () => {
    vi.stubGlobal('window', {
      location: { protocol: 'https:', hostname: 'faro.example' }
    });
    expect(apiBase()).toBe('https://faro.example:8080');
  });

  it('sin env ni window (SSR) cae a http://localhost:8080', () => {
    expect(apiBase()).toBe('http://localhost:8080');
  });
});

describe('api', () => {
  // Base fija para que las URLs aserveradas no dependan de window.
  beforeEach(() => {
    env.PUBLIC_API_BASE = 'http://api.test';
  });

  /** Stubbea fetch global construyendo una Response fresca por llamada (el
   *  body de una Response solo se puede consumir una vez). */
  function stubFetch(makeRes: () => Response) {
    const fetchMock = vi.fn(() => Promise.resolve(makeRes()));
    vi.stubGlobal('fetch', fetchMock);
    return fetchMock;
  }

  it('con 200 devuelve el JSON parseado', async () => {
    stubFetch(() => new Response(JSON.stringify({ ok: true, n: 7 }), { status: 200 }));
    await expect(api('/api/v1/x')).resolves.toEqual({ ok: true, n: 7 });
  });

  it('manda credentials include y Content-Type application/json por default', async () => {
    const fetchMock = stubFetch(() => new Response('{}', { status: 200 }));
    await api('/api/v1/x');
    expect(fetchMock).toHaveBeenCalledWith('http://api.test/api/v1/x', {
      credentials: 'include',
      headers: { 'Content-Type': 'application/json' }
    });
  });

  it('mergea headers custom sin pisar el resto del init', async () => {
    const fetchMock = stubFetch(() => new Response('{}', { status: 200 }));
    await api('/api/v1/x', { method: 'POST', headers: { Authorization: 'Bearer t0k3n' } });
    const [, init] = fetchMock.mock.calls[0] as unknown as [string, RequestInit];
    expect(init).toEqual({
      method: 'POST',
      credentials: 'include',
      headers: { 'Content-Type': 'application/json', Authorization: 'Bearer t0k3n' }
    });
  });

  it('401 en ruta privada lanza UnauthorizedError y redirige a /login?next=', async () => {
    stubFetch(() => new Response('', { status: 401 }));
    const assign = vi.fn();
    vi.stubGlobal('window', { location: { pathname: '/traces', assign } });
    await expect(api('/api/v1/x')).rejects.toBeInstanceOf(UnauthorizedError);
    expect(assign).toHaveBeenCalledWith('/login?next=%2Ftraces');
  });

  it.each(['/login', '/docs', '/docs/x'])(
    '401 en ruta pública %s lanza UnauthorizedError SIN redirect',
    async (pathname) => {
      stubFetch(() => new Response('', { status: 401 }));
      const assign = vi.fn();
      vi.stubGlobal('window', { location: { pathname, assign } });
      await expect(api('/api/v1/x')).rejects.toBeInstanceOf(UnauthorizedError);
      expect(assign).not.toHaveBeenCalled();
    }
  );

  it('401 sin window (SSR) lanza UnauthorizedError sin explotar', async () => {
    stubFetch(() => new Response('', { status: 401 }));
    await expect(api('/api/v1/x')).rejects.toBeInstanceOf(UnauthorizedError);
  });

  it('res no-ok lanza Error con "HTTP <status>: <body>"', async () => {
    stubFetch(() => new Response('boom interno', { status: 500 }));
    await expect(api('/api/v1/x')).rejects.toThrow('HTTP 500: boom interno');
  });
});
