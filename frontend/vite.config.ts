import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

// La configuración de tests vive en `vitest.config.ts` (vite v8 ya no acepta
// `test` dentro de su tipo UserConfigExport). Mantener ambos aquí causaba
// que `svelte-check` reportara un overload mismatch.
export default defineConfig({
  plugins: [sveltekit()],
  server: {
    port: 3000,
    host: '0.0.0.0',
    proxy: {
      '/api': { target: 'http://127.0.0.1:8080', changeOrigin: false },
      '/healthz': { target: 'http://127.0.0.1:8080', changeOrigin: false }
    }
  }
});
