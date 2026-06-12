import { api } from './core';

export type User = { id: string; email: string; name: string; role: string; created_at: string };

export const fetchUsers = () => api<User[]>(`/api/v1/users`);
export const createUser = (body: { email: string; password: string; name?: string; role?: string }) =>
  api<User>(`/api/v1/users`, { method: 'POST', body: JSON.stringify(body) });
export const updateUser = (id: string, body: { name: string; role: string }) =>
  api<User>(`/api/v1/users/${id}`, { method: 'PUT', body: JSON.stringify(body) });
export const deleteUser = (id: string) =>
  api(`/api/v1/users/${id}`, { method: 'DELETE' });
export const changeUserPassword = (id: string, password: string) =>
  api(`/api/v1/users/${id}/password`, { method: 'PUT', body: JSON.stringify({ password }) });
