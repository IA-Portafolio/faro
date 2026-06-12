import { qs, type RangeArgs, api } from './core';

export type Service = {
  service_name: string;
  log_count: number;
  error_count: number;
  last_seen: string;
};

export type ServiceMapNode = {
  service: string;
  calls: number;
  errors: number;
  p95_ms: number;
  is_root: number;
};

export type ServiceMapEdge = {
  source: string;
  target: string;
  calls: number;
  errors: number;
  p50_ms: number;
  p95_ms: number;
  p99_ms: number;
};

export type ServiceMap = {
  nodes: ServiceMapNode[];
  edges: ServiceMapEdge[];
};

export const fetchServices = (r: RangeArgs = {}) => api<Service[]>(`/api/v1/services${qs(r)}`);
export const fetchServiceMap = (r: RangeArgs = {}) => api<ServiceMap>(`/api/v1/services/map${qs(r)}`);
