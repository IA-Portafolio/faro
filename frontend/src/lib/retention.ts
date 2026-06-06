/**
 * Cálculo de retención por cohortes para la vista `/retention`.
 *
 * Cada fila es una cohorte (usuarios vistos por primera vez un día dado) y se
 * mide cuántos volvieron a los D1/D7/D30. `weightedRetention` promedia la tasa
 * ponderando por tamaño de cohorte e ignora las cohortes aún "inmaduras"
 * (`isMature`: todavía no pasó el plazo de D días, así no contaminan la media).
 * `heatColor`/`formatRetentionPct` dan el color y el texto del heatmap.
 */
export type RetentionDay = 1 | 7 | 30;

export type RetentionLikeRow = {
  cohort_date: string;
  cohort_size: number;
  d1_users: number;
  d7_users: number;
  d30_users: number;
};

export type WeightedRetention = {
  users: number;
  cohortSize: number;
  rate: number;
};

function usersForDay(row: RetentionLikeRow, day: RetentionDay): number {
  if (day === 1) return row.d1_users;
  if (day === 7) return row.d7_users;
  return row.d30_users;
}

export function retentionRate(row: RetentionLikeRow, day: RetentionDay): number {
  if (row.cohort_size <= 0) return 0;
  return usersForDay(row, day) / row.cohort_size;
}

export function isMature(cohortDate: string, day: RetentionDay, asOf = new Date()): boolean {
  const start = new Date(`${cohortDate}T00:00:00Z`);
  if (Number.isNaN(start.getTime())) return false;
  const target = new Date(start);
  target.setUTCDate(start.getUTCDate() + day);
  return asOf.getTime() >= target.getTime();
}

export function weightedRetention(
  rows: RetentionLikeRow[],
  day: RetentionDay,
  asOf = new Date()
): WeightedRetention {
  let users = 0;
  let cohortSize = 0;
  for (const row of rows) {
    if (row.cohort_size <= 0 || !isMature(row.cohort_date, day, asOf)) continue;
    users += usersForDay(row, day);
    cohortSize += row.cohort_size;
  }
  return {
    users,
    cohortSize,
    rate: cohortSize > 0 ? users / cohortSize : 0
  };
}

export function heatColor(rate: number, mature: boolean): string {
  if (!mature) return 'transparent';
  const clamped = Math.max(0, Math.min(1, Number.isFinite(rate) ? rate : 0));
  const alpha = 0.1 + clamped * 0.72;
  return `rgba(34, 197, 94, ${alpha.toFixed(3)})`;
}

export function formatRetentionPct(rate: number): string {
  if (!Number.isFinite(rate)) return '-';
  return `${(rate * 100).toFixed(1)}%`;
}
