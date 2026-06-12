import { api } from './core';

export type ExperimentVariantResult = {
  variant: 'A' | 'B' | string;
  sample: number;
  conversions: number;
  conversion_rate: number;
};

export type ExperimentAnalyzeRequest = {
  flag_key: string;
  conversion_event: string;
  project?: string;
  from?: string;
  to?: string;
  last_minutes?: number;
};

export type ExperimentAnalyzeResult = {
  flag_key: string;
  conversion_event: string;
  project: string;
  from: string;
  to: string;
  variants: ExperimentVariantResult[];
  sample: number;
  winner: string;
  absolute_delta: number;
  relative_lift: number;
  p_value: number;
  ci95_low: number;
  ci95_high: number;
  summary: string;
};

export const analyzeExperiment = (body: ExperimentAnalyzeRequest) =>
  api<ExperimentAnalyzeResult>(`/api/v1/experiments/analyze`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
