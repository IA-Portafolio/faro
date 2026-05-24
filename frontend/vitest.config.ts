import { defineConfig } from 'vitest/config';
import path from 'node:path';

// Config dedicada a tests unitarios de utilidades puras.
// No carga el plugin de SvelteKit a propósito: queremos arrancar rápido y sin
// tocar el runtime de Svelte. Para los módulos que importan `$app/environment`
// (p.ej. stores.ts) usamos un stub local.
export default defineConfig({
  resolve: {
    alias: {
      '$app/environment': path.resolve(__dirname, 'src/lib/__mocks__/app-environment.ts'),
      '$app/navigation': path.resolve(__dirname, 'src/lib/__mocks__/app-navigation.ts'),
      '$env/dynamic/public': path.resolve(__dirname, 'src/lib/__mocks__/env-dynamic-public.ts')
    }
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts']
  }
});
