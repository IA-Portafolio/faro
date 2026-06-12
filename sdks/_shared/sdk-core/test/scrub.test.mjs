// Suite unitaria del scrubbing del core.
// Los casos canónicos cross-SDK viven en parity.test.mjs; acá van bordes extra.
import test from 'node:test';
import assert from 'node:assert/strict';

import { scrubString, scrubWire, SCRUB_REGEXES, REDACTED } from '../dist/index.js';

test('scrubString aplica todas las regexes en orden', () => {
  const out = scrubString(
    'mail a@b.co y jwt eyJx.eyJy.zzz',
    [SCRUB_REGEXES.email, SCRUB_REGEXES.jwt],
  );
  assert.equal(out, `mail ${REDACTED} y jwt ${REDACTED}`);
});

test('scrubString sin regexes devuelve el input intacto', () => {
  assert.equal(scrubString('nada que ocultar', []), 'nada que ocultar');
});

test('scrubWire: needle matchea por substring case-insensitive', () => {
  const wire = { message: 'm', attributes: { 'X-Api_Key-Header': 'v', ok: 'v' } };
  scrubWire(wire, ['api_key'], []);
  assert.equal(wire.attributes['X-Api_Key-Header'], REDACTED);
  assert.equal(wire.attributes.ok, 'v');
});

test('scrubWire: needle tiene prioridad sobre regex (no doble proceso)', () => {
  const wire = { message: 'm', attributes: { password: 'a@b.co' } };
  scrubWire(wire, ['password'], [SCRUB_REGEXES.email]);
  assert.equal(wire.attributes.password, REDACTED);
});

test('SCRUB_REGEXES credit-card matchea 13-19 dígitos con separadores', () => {
  assert.equal(scrubString('cc 4111 1111 1111 1111', [SCRUB_REGEXES['credit-card']]), `cc ${REDACTED}`);
});
