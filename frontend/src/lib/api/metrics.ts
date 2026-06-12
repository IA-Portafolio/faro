import { qs, type RangeArgs, api } from './core';

export type MetricName = {
  metric_name: string;
  metric_type: string;
  metric_unit: string;
  service_name: string;
};

export type Point = { ts: string; value: number };

export const fetchMetricNames = (r: RangeArgs = {}) => api<MetricName[]>(`/api/v1/metrics/names${qs(r)}`);
export const fetchMetricSeries = (params: RangeArgs & { name: string; service?: string; bucket_seconds?: number; agg?: string }) =>
  api<Point[]>(`/api/v1/metrics/series${qs(params)}`);
