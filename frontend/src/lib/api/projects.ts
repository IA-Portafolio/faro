import { api } from './core';

export type Project = {
  id: string;
  slug: string;
  name: string;
  description: string;
  ingest_token: string;
  dsn: string;
  created_at: string;
  updated_at: string;
};

export const fetchProjects = () => api<Project[]>(`/api/v1/projects`);
export const fetchProject = (slug: string) => api<Project>(`/api/v1/projects/${slug}`);
export const createProject = (body: { name: string; slug?: string; description?: string }) =>
  api<Project>(`/api/v1/projects`, { method: 'POST', body: JSON.stringify(body) });
export const updateProject = (slug: string, body: { name: string; description?: string }) =>
  api<Project>(`/api/v1/projects/${slug}`, { method: 'PUT', body: JSON.stringify(body) });
export const deleteProject = (slug: string) =>
  api(`/api/v1/projects/${slug}`, { method: 'DELETE' });
export const rotateProjectToken = (slug: string) =>
  api<Project>(`/api/v1/projects/${slug}/rotate`, { method: 'POST' });
