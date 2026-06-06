import { describe, expect, it } from 'vitest';

import {
  commonOptions,
  productMatrix,
  profileDefaults,
  sdks,
  severities,
  totalMethods,
  type SdkProfile
} from './sdk-docs';

describe('sdks catalog', () => {
  it('is non-empty', () => {
    expect(sdks.length).toBeGreaterThan(0);
  });

  it('uses unique ids', () => {
    const ids = sdks.map((s) => s.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('gives every SDK the required descriptive fields', () => {
    for (const s of sdks) {
      expect(s.id, 'id').toBeTruthy();
      expect(s.name, `${s.id}.name`).toBeTruthy();
      expect(s.pkg, `${s.id}.pkg`).toBeTruthy();
      expect(s.install, `${s.id}.install`).toBeTruthy();
      expect(s.initExample.length, `${s.id}.initExample`).toBeGreaterThan(0);
      expect(['server', 'mobile', 'browser']).toContain(s.profile);
      expect(s.capabilities.length, `${s.id}.capabilities`).toBeGreaterThan(0);
    }
  });

  it('gives every SDK at least one method group with methods', () => {
    for (const s of sdks) {
      expect(s.groups.length, `${s.id}.groups`).toBeGreaterThan(0);
      for (const g of s.groups) {
        expect(g.methods.length, `${s.id}/${g.title}`).toBeGreaterThan(0);
        for (const m of g.methods) {
          expect(m.signature, `${s.id}/${g.title} signature`).toBeTruthy();
          expect(m.summary, `${s.id}/${g.title} summary`).toBeTruthy();
        }
      }
    }
  });
});

describe('totalMethods', () => {
  it('equals the flattened count of every method across every SDK', () => {
    const manual = sdks.reduce(
      (n, s) => n + s.groups.reduce((g, grp) => g + grp.methods.length, 0),
      0
    );
    expect(totalMethods()).toBe(manual);
    expect(totalMethods()).toBeGreaterThan(0);
  });
});

describe('profileDefaults', () => {
  it('defines every SDK profile', () => {
    const profiles: SdkProfile[] = ['server', 'mobile', 'browser'];
    for (const p of profiles) {
      expect(profileDefaults[p]).toBeDefined();
      expect(profileDefaults[p].flushMs).toBeGreaterThan(0);
      expect(profileDefaults[p].batch).toBeGreaterThan(0);
      expect(profileDefaults[p].queue).toBeGreaterThan(0);
      expect(profileDefaults[p].label).toBeTruthy();
    }
  });

  it('covers every profile referenced by an SDK', () => {
    for (const s of sdks) {
      expect(profileDefaults[s.profile]).toBeDefined();
    }
  });
});

describe('severities', () => {
  it('is strictly increasing by OTel severity number', () => {
    for (let i = 1; i < severities.length; i++) {
      expect(severities[i].num).toBeGreaterThan(severities[i - 1].num);
    }
  });

  it('matches the OTel WARN/ERROR thresholds Faro fingerprints on', () => {
    const byText = Object.fromEntries(severities.map((s) => [s.text, s.num]));
    expect(byText.INFO).toBe(9);
    expect(byText.WARN).toBe(13);
    expect(byText.ERROR).toBe(17);
  });
});

describe('commonOptions', () => {
  it('documents the core init options exactly once each', () => {
    const names = commonOptions.map((o) => o.name);
    expect(new Set(names).size).toBe(names.length);
    for (const required of ['endpoint', 'token', 'service']) {
      expect(names).toContain(required);
    }
  });
});

describe('productMatrix', () => {
  it('is non-empty and uniquely keyed by SDK', () => {
    const keys = productMatrix.map((r) => r.sdk);
    expect(keys.length).toBeGreaterThan(0);
    expect(new Set(keys).size).toBe(keys.length);
  });

  it('marks track + identify available for every listed SDK', () => {
    for (const row of productMatrix) {
      expect(row.track, `${row.sdk}.track`).toBe(true);
      expect(row.identify, `${row.sdk}.identify`).toBe(true);
    }
  });
});
