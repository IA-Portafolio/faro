import { expect, test } from '@playwright/test';

/**
 * E2E del flujo de acceso. El valor frente a los component tests (que stubean
 * fetch) es que esto ejercita el stack REAL: navegador → SvelteKit → backend, con
 * cookies y CORS reales — la clase de bug que rompió prod (CORS/cookie) y que un
 * test con fetch mockeado no puede cazar.
 */

test('la página de login renderiza el formulario (email + password)', async ({ page }) => {
  const resp = await page.goto('/login');
  expect(resp?.status(), 'GET /login debe responder 2xx').toBeLessThan(400);

  // Los inputs reales del form (ver routes/login/+page.svelte).
  await expect(page.locator('#email-input')).toBeVisible();
  await expect(page.locator('#pass-input')).toBeVisible();
  await expect(page.getByRole('button', { name: /entrar|ingresar|iniciar/i })).toBeVisible();
});

const E2E_EMAIL = process.env.FARO_E2E_EMAIL;
const E2E_PASSWORD = process.env.FARO_E2E_PASSWORD;

// Round-trip autenticado: éste es el que caza CORS/cookie roto. Se SKIPea si no
// hay credenciales (CI lo corre contra un stack de test con FARO_E2E_*; no se
// ejecuta contra prod por defecto para no crear sesiones).
test('login real → redirige fuera de /login y carga el dashboard (cookie + CORS OK)', async ({
  page,
}) => {
  test.skip(
    !E2E_EMAIL || !E2E_PASSWORD,
    'definí FARO_E2E_EMAIL / FARO_E2E_PASSWORD (usuario SIN 2FA) para correr este test',
  );

  await page.goto('/login');
  await page.locator('#email-input').fill(E2E_EMAIL!);
  await page.locator('#pass-input').fill(E2E_PASSWORD!);
  await page.getByRole('button', { name: /entrar|ingresar|iniciar/i }).click();

  // Tras el login exitoso el SPA hace goto(next) (default '/'): la URL deja de ser
  // /login y el form de login desaparece. Si CORS o la cookie estuvieran rotos, el
  // login fallaría y seguiríamos en /login.
  await expect(page).not.toHaveURL(/\/login\b/, { timeout: 15_000 });
  await expect(page.locator('#pass-input')).toHaveCount(0);
});
