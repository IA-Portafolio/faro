// Suite unitaria de los helpers de feature flags del core.
// Los casos canónicos cross-SDK viven en parity.test.mjs; acá van bordes extra.
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  stickyBucket,
  clampRollout,
  normalizeConditions,
  matchesFeatureConditions,
} from '../dist/index.js';

test('stickyBucket es determinístico y está en [0, 100)', () => {
  for (const input of ['a', 'b', 'user-1', 'user-2', 'ñ', '']) {
    const v = stickyBucket(input);
    assert.equal(v, stickyBucket(input));
    assert.ok(Number.isInteger(v) && v >= 0 && v < 100, `${input} → ${v}`);
  }
});

test('stickyBucket distingue inputs distintos (no constante)', () => {
  const values = new Set(['u1', 'u2', 'u3', 'u4', 'u5', 'u6'].map(stickyBucket));
  assert.ok(values.size > 1);
});

test('clampRollout trunca decimales hacia cero', () => {
  assert.equal(clampRollout(0.9), 0);
  assert.equal(clampRollout(1.9), 1);
  assert.equal(clampRollout(-0.5), 0);
});

test('normalizeConditions: arrays cuentan como objeto (comportamiento heredado)', () => {
  const arr = [];
  assert.equal(normalizeConditions(arr), arr);
});

test('matchesFeatureConditions ignora claves extra del context', () => {
  const flag = { key: 'f', rollout_percentage: 100, conditions: { properties: { a: 1 } } };
  assert.equal(matchesFeatureConditions(flag, { properties: { a: 1, b: 2 } }), true);
});
