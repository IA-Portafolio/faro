import { qs, type RangeArgs, api } from './core';

export type RetentionCohort = {
  cohort_date: string;
  cohort_size: number;
  d1_users: number;
  d7_users: number;
  d30_users: number;
};

export type RetentionResult = {
  from: string;
  to: string;
  event_name: string;
  interval: 'day';
  columns: Array<1 | 7 | 30>;
  cohorts: RetentionCohort[];
  took_ms: number;
};

export type RetentionFilters = RangeArgs & {
  event_name?: string;
  interval?: 'day';
};

export const fetchRetention = (params: RetentionFilters = {}) =>
  api<RetentionResult>(`/api/v1/retention${qs(params)}`);
