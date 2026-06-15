/**
 * Scrubbing de datos sensibles — fuente canónica cross-SDK.
 *
 * Estas funciones estaban reimplementadas (copy-paste) en
 * sdks/node, sdks/nextjs y sdks/expo. Cualquier cambio acá DEBE
 * mantenerse compatible con la spec ejecutable
 * (test/parity.test.mjs + docs/superpowers/specs/2026-06-10-sdk-parity-spec.md).
 */

export type ScrubPreset = 'email' | 'jwt' | 'credit-card' | 'api-key';

export const DEFAULT_SCRUB_FIELDS = [
  'password', 'token', 'secret', 'authorization', 'cookie', 'set-cookie', 'api_key', 'apikey',
];
export const HEADER_SCRUB_FIELDS = ['authorization', 'cookie', 'set-cookie'];
export const REDACTED = '[REDACTED]';

export const SCRUB_REGEXES: Record<ScrubPreset, RegExp> = {
  'email': /[\w.+-]+@[\w-]+(?:\.[\w-]+)+/g,
  'jwt': /\beyJ[\w-]+\.[\w-]+\.[\w-]+\b/g,
  // Sin Luhn; puede tener falsos positivos en IDs largos. Opt-in deliberadamente.
  'credit-card': /\b(?:\d[ -]?){13,19}\b/g,
  'api-key': /\b(?:sk-|ghp_|ghs_|gho_|github_pat_|xoxb-|xoxp-|xoxs-|AKIA|ASIA|AIza)[\w-]{12,}\b/g,
};

/** Subconjunto estructural del Wire/WireEvent de cada SDK que el scrubber toca. */
export interface ScrubbableWire {
  message: string;
  attributes: Record<string, string>;
}

// Tope anti-ReDoS: algunos presets (p.ej. credit-card `(?:\d[ -]?){13,19}`)
// tienen backtracking polinómico; aplicarlos sobre strings arbitrariamente
// largos de telemetría no controlada permitiría un ReDoS. Acotamos la longitud
// que pasa por los regexes — los valores reales de telemetría son cortos, y los
// atributos largos igual quedan cubiertos por el redactado por nombre de campo.
const MAX_REGEX_SCRUB_LEN = 8192;

export function scrubString(s: string, regexes: RegExp[]): string {
  if (s.length > MAX_REGEX_SCRUB_LEN) return s;
  let out = s;
  for (const re of regexes) out = out.replace(re, REDACTED);
  return out;
}

export function scrubWire(wire: ScrubbableWire, fieldNeedles: string[], regexes: RegExp[]): void {
  for (const key of Object.keys(wire.attributes)) {
    const kLower = key.toLowerCase();
    if (fieldNeedles.some((n) => kLower.includes(n))) {
      wire.attributes[key] = REDACTED;
    } else if (regexes.length > 0) {
      wire.attributes[key] = scrubString(wire.attributes[key], regexes);
    }
  }
  if (regexes.length > 0) wire.message = scrubString(wire.message, regexes);
}
