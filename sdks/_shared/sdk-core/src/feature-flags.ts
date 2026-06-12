/**
 * Feature flags — helpers puros, fuente canónica cross-SDK.
 *
 * Reimplementados antes en sdks/node, sdks/nextjs y sdks/expo. Cambios acá
 * DEBEN mantenerse compatibles con la spec ejecutable
 * (test/parity.test.mjs + docs/superpowers/specs/2026-06-10-sdk-parity-spec.md).
 * En particular `stickyBucket` (FNV-1a 32-bit % 100) define a qué usuarios
 * les toca un rollout parcial: cambiarlo re-bucketiza a TODOS los usuarios.
 */

export interface FeatureFlagContext {
  distinct_id?: string;
  properties?: Record<string, unknown>;
}

export interface FeatureFlagWire {
  key: string;
  rollout_percentage: number;
  conditions?: {
    properties?: Record<string, unknown>;
  } & Record<string, unknown>;
}

export function clampRollout(value: unknown): number {
  const n = typeof value === 'number' && Number.isFinite(value) ? Math.trunc(value) : 0;
  return Math.max(0, Math.min(100, n));
}

export function normalizeConditions(value: FeatureFlagWire['conditions']): FeatureFlagWire['conditions'] {
  return value && typeof value === 'object' ? value : {};
}

export function matchesFeatureConditions(flag: FeatureFlagWire, context: FeatureFlagContext): boolean {
  const required = flag.conditions?.properties;
  if (!required || typeof required !== 'object') return true;
  const props = context.properties ?? {};
  for (const [key, expected] of Object.entries(required)) {
    if (props[key] !== expected) return false;
  }
  return true;
}

/** FNV-1a 32-bit sobre el input, módulo 100. Determinístico cross-SDK. */
export function stickyBucket(input: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < input.length; i++) {
    hash ^= input.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0) % 100;
}
