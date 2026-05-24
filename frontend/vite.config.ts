import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';

// El bloque `test:` lo lee vitest (no rompe el build normal porque vite lo
// ignora). Los alias mapean los imports de runtime de SvelteKit (`$app/*`,
// `$env/*`) a los stubs en `src/lib/__mocks__/` para que tests unitarios de
// `lib/` no necesiten levantar todo el runtime de SK.
export default defineConfig({
  plugins: [sveltekit()],
  server: {
    port: 3000,
    host: '0.0.0.0',
    proxy: {
      '/api': { target: 'http://127.0.0.1:8080', changeOrigin: false },
      '/healthz': { target: 'http://127.0.0.1:8080', changeOrigin: false }
    }
  },
  test: {
    environment: 'node',
    include: ['src/**/*.{test,spec}.{ts,js}'],
    alias: {
      '$app/environment': fileURLToPath(
        new URL('./src/lib/__mocks__/app-environment.ts', import.meta.url)
      ),
      '$app/navigation': fileURLToPath(
        new URL('./src/lib/__mocks__/app-navigation.ts', import.meta.url)
      ),
      '$env/dynamic/public': fileURLToPath(
        new URL('./src/lib/__mocks__/env-dynamic-public.ts', import.meta.url)
      )
    }
  }
});
