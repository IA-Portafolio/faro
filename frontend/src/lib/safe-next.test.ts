import { describe, expect, it } from 'vitest';

import { safeNext } from './safe-next';

describe('safeNext', () => {
  it('devuelve "/" para null, undefined y string vacío', () => {
    expect(safeNext(null)).toBe('/');
    expect(safeNext(undefined)).toBe('/');
    expect(safeNext('')).toBe('/');
    expect(safeNext('   ')).toBe('/');
  });

  it('bloquea URLs absolutas (esquema http/https/ftp/etc)', () => {
    expect(safeNext('http://evil.com')).toBe('/');
    expect(safeNext('https://evil.com/path')).toBe('/');
    expect(safeNext('ftp://evil.com')).toBe('/');
  });

  it('bloquea URLs protocol-relative (//evil.com)', () => {
    // Caso real del que nos protege: el atacante manda
    // `/login?next=//evil.com` y espera que el browser navegue a evil.com.
    expect(safeNext('//evil.com')).toBe('/');
    expect(safeNext('//evil.com/path')).toBe('/');
  });

  it('bloquea protocol-relative con backslash (/\\evil.com)', () => {
    // El WHATWG URL parser y los browsers normalizan `\` → `/`, así que
    // `/\evil.com` ≡ `//evil.com`. Un check naive que solo mira `startsWith('//')`
    // lo deja pasar; nosotros rechazamos cualquier backslash.
    expect(safeNext('/\\evil.com')).toBe('/');
    expect(safeNext('/\\/evil.com')).toBe('/');
    expect(safeNext('/foo\\bar')).toBe('/');
  });

  it('bloquea esquemas javascript:, data:, vbscript: (XSS pretext)', () => {
    expect(safeNext('javascript:alert(1)')).toBe('/');
    expect(safeNext('data:text/html,<script>alert(1)</script>')).toBe('/');
    expect(safeNext('vbscript:msgbox(1)')).toBe('/');
  });

  it('permite paths absolutos same-origin', () => {
    expect(safeNext('/')).toBe('/');
    expect(safeNext('/dashboard')).toBe('/dashboard');
    expect(safeNext('/users/42?tab=events#x')).toBe('/users/42?tab=events#x');
  });

  it('ignora whitespace y/o control chars al borde', () => {
    // Un atacante podría inyectar `\t//evil.com` esperando bypass de un check
    // naive que solo mire `startsWith('/')`. Nuestro trim() + startsWith doble
    // cierra eso.
    expect(safeNext('\t//evil.com')).toBe('/');
    expect(safeNext('  /dashboard  ')).toBe('/dashboard');
  });
});
