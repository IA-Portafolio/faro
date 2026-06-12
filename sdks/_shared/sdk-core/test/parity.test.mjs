// Spec ejecutable de paridad cross-SDK (Task 5 del plan M-5).
//
// Dos garantías:
//  1. La fuente canónica (@iaportafolio/sdk-core) cumple los casos fijos de
//     test/fixtures/parity-cases.json (la spec escrita está en
//     docs/superpowers/specs/2026-06-10-sdk-parity-spec.md).
//  2. Ningún SDK TS reimplementa localmente las funciones compartidas: cada
//     src debe importar de @iaportafolio/sdk-core y NO definirlas inline.
//     La paridad queda garantizada por construcción + este guard.
import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, join } from 'node:path';

import {
  stickyBucket,
  clampRollout,
  matchesFeatureConditions,
  normalizeConditions,
  scrubWire,
  DEFAULT_SCRUB_FIELDS,
  HEADER_SCRUB_FIELDS,
  REDACTED,
  SCRUB_REGEXES,
} from '../dist/index.js';

const here = dirname(fileURLToPath(import.meta.url));
const cases = JSON.parse(readFileSync(join(here, 'fixtures', 'parity-cases.json'), 'utf8'));

test('stickyBucket: valores canónicos fijos', () => {
  for (const c of cases.stickyBucket) {
    assert.equal(stickyBucket(c.input), c.expected, `stickyBucket(${JSON.stringify(c.input)})`);
  }
});

test('clampRollout: casos canónicos (incl. NaN/Infinity)', () => {
  for (const c of cases.clampRollout) {
    const input = c.$special === 'NaN' ? NaN : c.$special === 'Infinity' ? Infinity : c.input;
    assert.equal(clampRollout(input), c.expected, `clampRollout(${String(input)})`);
  }
});

test('matchesFeatureConditions: casos canónicos', () => {
  for (const c of cases.matchesFeatureConditions) {
    assert.equal(matchesFeatureConditions(c.flag, c.context), c.expected, c.name);
  }
});

test('normalizeConditions: objeto pasa, no-objeto → {}', () => {
  const obj = { properties: { a: 1 } };
  assert.equal(normalizeConditions(obj), obj);
  assert.deepEqual(normalizeConditions(undefined), {});
  assert.deepEqual(normalizeConditions(null), {});
});

test('scrubWire: casos canónicos', () => {
  for (const c of cases.scrubWire) {
    const wire = structuredClone(c.wire);
    const regexes = c.patterns.map((p) => SCRUB_REGEXES[p]);
    scrubWire(wire, c.needles, regexes);
    assert.deepEqual(wire, c.expected, c.name);
  }
});

test('constantes compartidas: valores canónicos', () => {
  assert.deepEqual(DEFAULT_SCRUB_FIELDS, cases.constants.DEFAULT_SCRUB_FIELDS);
  assert.deepEqual(HEADER_SCRUB_FIELDS, cases.constants.HEADER_SCRUB_FIELDS);
  assert.equal(REDACTED, cases.constants.REDACTED);
});

// ---- Guard anti-divergencia: los 3 SDKs TS deben consumir el core ----

const SHARED_FNS = [
  'stickyBucket', 'clampRollout', 'normalizeConditions',
  'matchesFeatureConditions', 'scrubWire', 'scrubString',
];
const SDK_SOURCES = [
  ['node', join(here, '..', '..', '..', 'node', 'src', 'index.ts')],
  ['nextjs', join(here, '..', '..', '..', 'nextjs', 'src', 'browser-core.ts')],
  ['expo', join(here, '..', '..', '..', 'expo', 'src', 'index.ts')],
];

for (const [name, path] of SDK_SOURCES) {
  test(`paridad por construcción: sdk-${name} importa el core y no reimplementa`, () => {
    const src = readFileSync(path, 'utf8');
    assert.match(src, /from '@iaportafolio\/sdk-core'/, `${name} debe importar @iaportafolio/sdk-core`);
    for (const fn of SHARED_FNS) {
      assert.doesNotMatch(
        src,
        new RegExp(`function ${fn}\\b`),
        `${name} reimplementa ${fn} localmente — usar @iaportafolio/sdk-core`,
      );
    }
    assert.doesNotMatch(src, /const SCRUB_REGEXES\s*[:=]/, `${name} redefine SCRUB_REGEXES localmente`);
  });
}
