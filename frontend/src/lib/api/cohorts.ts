import { api, qs } from './core';

export type CohortFilter = { key: string; value: string };
export type CohortDefinition = {
  event: string;
  op: '==' | '>=' | '>' | '<=' | '<';
  count: number;
  /** Ventana hacia atrás desde ahora, en días. */
  last_days: number;
  filters?: CohortFilter[];
};

export type Cohort = {
  id: string;
  project_id: string;
  name: string;
  description: string;
  /** JSON serializado de [`CohortDefinition`]. Usar `parseCohortDefinition`. */
  definition: string;
  created_at: string;
  updated_at: string;
  created_by: string;
  deleted: number;
  version: number;
};

export type CohortInput = {
  name: string;
  description?: string;
  project?: string;
  definition: CohortDefinition;
};

export type CohortPreview = {
  size: number;
  sample: string[];
  took_ms: number;
};

export type CohortRetentionPoint = {
  /** 0 = hoy, 1 = ayer, … */
  day_back: number;
  active_users: number;
};

export type CohortRetention = {
  cohort_size: number;
  horizon_days: number;
  points: CohortRetentionPoint[];
  took_ms: number;
};

export type CohortOverlap = {
  size_a: number;
  size_b: number;
  intersection: number;
  /** Jaccard ∈ [0, 1]. */
  jaccard: number;
  took_ms: number;
};

/** Parser tolerante: una definition malformada se reporta a UI sin reventar la pantalla. */
export function parseCohortDefinition(raw: string): CohortDefinition | null {
  try {
    const v = JSON.parse(raw) as CohortDefinition;
    if (!v.event || typeof v.event !== 'string') return null;
    if (typeof v.count !== 'number') return null;
    if (typeof v.last_days !== 'number') return null;
    if (!v.op) return null;
    return v;
  } catch {
    return null;
  }
}

export const listCohorts = (r: { project?: string } = {}) =>
  api<Cohort[]>(`/api/v1/cohorts${qs(r)}`);
export const getCohort = (id: string) =>
  api<Cohort>(`/api/v1/cohorts/${encodeURIComponent(id)}`);
export const createCohort = (body: CohortInput) =>
  api<Cohort>(`/api/v1/cohorts`, { method: 'POST', body: JSON.stringify(body) });
export const updateCohort = (id: string, body: CohortInput) =>
  api<Cohort>(`/api/v1/cohorts/${encodeURIComponent(id)}`, {
    method: 'PUT',
    body: JSON.stringify(body)
  });
export const deleteCohort = (id: string) =>
  api<{ ok: boolean }>(`/api/v1/cohorts/${encodeURIComponent(id)}`, { method: 'DELETE' });

export const previewCohort = (body: {
  project?: string;
  definition: CohortDefinition;
  sample_limit?: number;
}) => api<CohortPreview>(`/api/v1/cohorts/preview`, { method: 'POST', body: JSON.stringify(body) });

export const fetchCohortUsers = (id: string, params: { limit?: number } = {}) =>
  api<CohortPreview>(`/api/v1/cohorts/${encodeURIComponent(id)}/users${qs(params)}`);

export const fetchCohortRetention = (id: string, params: { horizon_days?: number } = {}) =>
  api<CohortRetention>(`/api/v1/cohorts/${encodeURIComponent(id)}/retention${qs(params)}`);

export const fetchCohortOverlap = (id: string, other: string) =>
  api<CohortOverlap>(`/api/v1/cohorts/${encodeURIComponent(id)}/overlap${qs({ other })}`);
