import { defineConfig, devices } from '@playwright/test';

/**
 * E2E del dashboard de Faro (browser real → frontend SvelteKit → backend).
 *
 * Llena el hueco que la auditoría marcó: los tests de componentes (vitest) stubean
 * `fetch`, así que NO pueden cazar bugs de integración real como CORS/cookie roto
 * (el commit f37306c y la nota de que FARO_DASHBOARD_ORIGINS corría en modo DEV son
 * exactamente de esa clase). Un e2e con browser real sí los caza.
 *
 * Config:
 *   FARO_E2E_BASE_URL   URL del dashboard a probar. Default: la prod pública.
 *   FARO_E2E_EMAIL      credenciales para el test del round-trip autenticado.
 *   FARO_E2E_PASSWORD   Si faltan, ese test se SKIPea (no rompe la suite).
 */
export default defineConfig({
  testDir: './e2e',
  // Sólo archivos *.e2e.ts (los *.test.ts son de vitest y NO deben correr acá).
  testMatch: '**/*.e2e.ts',
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  reporter: process.env.CI ? 'github' : 'list',
  use: {
    baseURL: process.env.FARO_E2E_BASE_URL ?? 'https://faro.iaportafolio.com',
    trace: 'on-first-retry',
  },
  projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
});
