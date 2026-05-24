import { describe, expect, it } from 'vitest';

import {
  heatColor,
  isMature,
  retentionRate,
  weightedRetention,
  type RetentionLikeRow
} from './retention';

const row: RetentionLikeRow = {
  cohort_date: '2026-05-01',
  cohort_size: 100,
  d1_users: 42,
  d7_users: 25,
  d30_users: 5
};

describe('retention helpers', () => {
  it('computes retention rate for D1 D7 D30', () => {
    expect(retentionRate(row, 1)).toBe(0.42);
    expect(retentionRate(row, 7)).toBe(0.25);
    expect(retentionRate(row, 30)).toBe(0.05);
  });

  it('returns zero rate for empty cohorts', () => {
    expect(retentionRate({ ...row, cohort_size: 0 }, 1)).toBe(0);
  });

  it('marks a retention day mature only after the return date has elapsed', () => {
    expect(isMature('2026-05-01', 1, new Date('2026-05-02T12:00:00Z'))).toBe(true);
    expect(isMature('2026-05-01', 7, new Date('2026-05-07T23:59:59Z'))).toBe(false);
    expect(isMature('2026-05-01', 7, new Date('2026-05-08T00:00:00Z'))).toBe(true);
  });

  it('computes weighted retention using only mature non-empty cohorts', () => {
    const rows: RetentionLikeRow[] = [
      row,
      { cohort_date: '2026-05-20', cohort_size: 50, d1_users: 10, d7_users: 50, d30_users: 50 },
      { cohort_date: '2026-05-01', cohort_size: 0, d1_users: 0, d7_users: 0, d30_users: 0 }
    ];

    expect(weightedRetention(rows, 7, new Date('2026-05-24T00:00:00Z'))).toEqual({
      users: 25,
      cohortSize: 100,
      rate: 0.25
    });
  });

  it('returns muted style for immature cells and stronger color for higher rates', () => {
    expect(heatColor(0.5, false)).toContain('transparent');
    expect(heatColor(0.8, true)).not.toBe(heatColor(0.2, true));
  });
});
