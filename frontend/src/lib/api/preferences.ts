import { api } from './core';

export type ThemePref = 'light' | 'dark' | 'system';
export type TimeRangePref = '5m' | '15m' | '1h' | '6h' | '24h' | '7d';

export type Preferences = {
  theme: ThemePref;
  /** Slug del proyecto por defecto. `''` significa "ver todos". */
  default_project: string;
  default_time_range: TimeRangePref;
  updated_at: string;
};

/** Payload de actualización parcial (PATCH-style): omitir un campo lo deja intacto. */
export type PreferencesPatch = Partial<Pick<Preferences, 'theme' | 'default_project' | 'default_time_range'>>;

export const fetchPreferences = () => api<Preferences>(`/api/v1/me/preferences`);
export const savePreferences = (body: PreferencesPatch) =>
  api<Preferences>(`/api/v1/me/preferences`, { method: 'PUT', body: JSON.stringify(body) });
