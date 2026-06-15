import { api } from './core';

export type AuthUser = { id: string; email: string; name: string; role: string };

/** Respuesta del POST /auth/login. Si `needs_totp` está, el cliente debe llamar a
 *  /auth/login/2fa con el `challenge_token` y un código TOTP/recovery. */
export type LoginResponse =
  | (AuthUser & { needs_totp?: false })
  | { needs_totp: true; challenge_token: string; expires_in_secs: number };

export const login = (body: { email: string; password: string }) =>
  api<LoginResponse>(`/api/v1/auth/login`, { method: 'POST', body: JSON.stringify(body) });
export const loginTotp = (body: { challenge_token: string; code: string; recovery?: boolean }) =>
  api<AuthUser>(`/api/v1/auth/login/2fa`, { method: 'POST', body: JSON.stringify(body) });
export const logout = () =>
  api<{ ok: boolean }>(`/api/v1/auth/logout`, { method: 'POST' });
export const me = () => api<AuthUser>(`/api/v1/auth/me`);

// ---------- Sessions + 2FA ----------
export type SessionInfo = {
  token_hash: string;
  created_at: string;
  expires_at: string;
  is_current: boolean;
};

export const fetchSessions = () => api<SessionInfo[]>(`/api/v1/me/sessions`);
export const revokeOtherSessions = () =>
  api<{ revoked: number }>(`/api/v1/me/sessions/revoke-others`, { method: 'POST' });

export type TwoFaStatus = { enabled: boolean; recovery_codes_remaining: number };
export type TwoFaSetup = {
  secret_base32: string;
  otpauth_url: string;
  /** SVG inline del QR, listo para inyectar con {@html}. */
  qr_svg: string;
};
export type TwoFaEnableResult = { enabled: boolean; recovery_codes: string[] };

export const fetchTwoFaStatus = () => api<TwoFaStatus>(`/api/v1/me/security/2fa`);
export const twoFaSetup = () => api<TwoFaSetup>(`/api/v1/me/security/2fa/setup`, { method: 'POST' });
export const twoFaEnable = (code: string) =>
  api<TwoFaEnableResult>(`/api/v1/me/security/2fa/enable`, {
    method: 'POST',
    body: JSON.stringify({ code })
  });
export const twoFaDisable = (body: { password: string; code?: string; recovery_code?: string }) =>
  api<{ enabled: false }>(`/api/v1/me/security/2fa/disable`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
export const twoFaRegenRecovery = (body: { password: string; code: string }) =>
  api<TwoFaEnableResult>(`/api/v1/me/security/2fa/recovery-codes`, {
    method: 'POST',
    body: JSON.stringify(body)
  });
