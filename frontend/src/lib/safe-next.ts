/**
 * Valida el parámetro `?next=` que el login usa para redirigir post-auth.
 *
 * Por qué existe: `goto()` de SvelteKit 2.x rechaza URLs externas con un
 * `Error` (lanza `Cannot use \`goto\` with an external URL`), así que pasarle
 * `//evil.com` o `https://evil.com` no llega a navegar — pero seguimos
 * dependiendo de una salvaguarda del framework. Esta función aplica la misma
 * regla a nivel de aplicación: si `next` no es un path absoluto que arranque
 * con `/` y no sea protocol-relative, vuelve a `/`. Cuesta tres líneas y
 * blinda contra una degradación futura del cliente de SvelteKit.
 *
 * Reglas:
 *   - `null` / `''` / whitespace → `'/'`
 *   - `//evil.com`               → `'/'`   (protocol-relative)
 *   - `/\evil.com`               → `'/'`   (protocol-relative con backslash)
 *   - `https://evil.com`         → `'/'`   (URL absoluta)
 *   - `javascript:alert(1)`      → `'/'`   (esquema hostil)
 *   - `/foo`                     → `'/foo'`
 *   - `/foo?bar=1#x`             → preservado tal cual
 */
export function safeNext(raw: string | null | undefined): string {
  if (!raw) return '/';
  // Trim defensivo: un atacante puede mandar `%20` o `+` en la URL.
  const v = raw.trim();
  if (v === '') return '/';
  // Debe arrancar con `/` y NO con `//` (protocol-relative).
  if (!v.startsWith('/') || v.startsWith('//')) return '/';
  // Backslashes: el WHATWG URL parser y los browsers normalizan `\` → `/`, así
  // que `/\evil.com` se interpreta como `//evil.com` (open redirect). Un path
  // legítimo nunca contiene `\`, así que rechazamos cualquier ocurrencia.
  if (v.includes('\\')) return '/';
  return v;
}
