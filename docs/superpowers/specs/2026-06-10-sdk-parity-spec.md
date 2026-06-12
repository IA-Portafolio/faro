# Spec de paridad cross-SDK — scrubbing + feature flags

> **Estado:** vigente desde 2026-06-10 (plan M-5).
> **Spec ejecutable:** `sdks/_shared/sdk-core/test/parity.test.mjs`
> **Casos canónicos:** `sdks/_shared/sdk-core/test/fixtures/parity-cases.json`

## Qué garantiza

Las funciones puras de scrubbing y feature flags deben comportarse **idéntico**
en todos los SDKs de Faro. Para los 3 SDKs TypeScript (node, nextjs, expo) la
paridad es **por construcción**: importan la única implementación de
`@iaportafolio/sdk-core` (`sdks/_shared/sdk-core/`), que tsup inlinea en cada
bundle publicado (el paquete core es `private` y no se publica).

El test de paridad agrega dos guardas:

1. **Casos canónicos fijos** (fixtures JSON) que la implementación core debe
   cumplir. Si alguien cambia la semántica, el test falla con diff.
2. **Guard anti-reimplementación**: falla si algún SDK TS vuelve a definir
   localmente `stickyBucket`, `clampRollout`, `normalizeConditions`,
   `matchesFeatureConditions`, `scrubWire`, `scrubString` o `SCRUB_REGEXES`,
   o deja de importar `@iaportafolio/sdk-core`.

Para SDKs no-TS (python, go, flutter, kotlin) los fixtures JSON son la fuente
canónica a portar: un harness en cada lenguaje puede leer el mismo
`parity-cases.json` (ver Task 6 del plan M-5 — fuera de alcance hoy).

## Contratos

### `stickyBucket(input: string): number`

FNV-1a 32-bit (offset `0x811c9dc5`, prime `0x01000193`, multiplicación con
wrap de 32 bits — `Math.imul` en JS) sobre los `charCode` del input, luego
`>>> 0` y `% 100`. Determinístico: decide qué usuarios entran a un rollout
parcial con la clave `"{project}:{flagKey}:{distinct_id}"`.

⚠️ **Cambiar esto re-bucketiza a TODOS los usuarios de TODOS los flags.**
Los valores fijados en fixtures (p. ej. `stickyBucket('user-42') === 99`)
nunca deben cambiar sin una migración deliberada.

Nota cross-lenguaje: el hash opera sobre **unidades UTF-16** (`charCodeAt`),
no bytes UTF-8. Un port a otro lenguaje debe iterar code units UTF-16.

### `clampRollout(value: unknown): number`

Entero en `[0, 100]`. No-número, `NaN`, `±Infinity` y strings → `0`.
Decimales se **truncan hacia cero** (`Math.trunc`), no se redondean.

### `matchesFeatureConditions(flag, context): boolean`

- Sin `conditions.properties` (o no-objeto) → `true`.
- Cada par requerido debe cumplirse con **igualdad estricta** (`!==`, sin
  coerción: `1 !== '1'`).
- Claves extra en el context se ignoran.

### `normalizeConditions(value)`

Objeto (incluye arrays — comportamiento heredado) pasa tal cual; cualquier
otra cosa → `{}`.

### Scrubbing

- `DEFAULT_SCRUB_FIELDS = ['password','token','secret','authorization','cookie','set-cookie','api_key','apikey']`
- `HEADER_SCRUB_FIELDS = ['authorization','cookie','set-cookie']`
- `REDACTED = '[REDACTED]'`
- `SCRUB_REGEXES`: presets `email`, `jwt`, `credit-card` (sin Luhn, opt-in),
  `api-key` — patrones exactos en `src/scrub.ts`.

`scrubWire(wire, needles, regexes)`:

1. Por cada attribute: si la **clave** en lowercase contiene algún needle
   (substring, case-insensitive) → el valor entero se reemplaza por
   `REDACTED` (los regexes NO se aplican además).
2. Si no matchea needle y hay regexes → se aplican todas al **valor**.
3. Si hay regexes → también se aplican al `message`. Sin regexes el message
   queda intacto (los needles no aplican al message).

Muta el wire in-place. Solo toca `message` y `attributes`.

## Cómo correr

```bash
cd sdks/_shared/sdk-core && npm test   # build + unit + parity
```

También corre dentro de `scripts/test-all.sh sdk-node` (el core es
prerequisito de los SDKs TS).

## Cómo extender

1. Agregá el caso a `parity-cases.json` **y** describilo acá.
2. Implementá en `sdks/_shared/sdk-core/src/`.
3. Si el cambio toca semántica visible, evaluá impacto en SDKs no-TS
   (python/go/flutter/kotlin) — hoy NO consumen el core y deben portarse a
   mano (divergencia conocida, ver plan M-5 Task 6).
