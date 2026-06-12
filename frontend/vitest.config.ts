import path from 'node:path';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { svelteTesting } from '@testing-library/svelte/vite';
import { configDefaults, defineConfig } from 'vitest/config';

// Dos proyectos de vitest en un solo `vitest run` (npm test no cambia):
//
//  - "unit": tests de módulos puros (environment node). NO carga el plugin de
//    Svelte a propósito: queremos arrancar rápido y sin tocar el runtime de
//    Svelte. Para los módulos que importan `$app/environment` (p.ej. stores.ts)
//    usamos un stub local.
//  - "components": component tests de Svelte 5 (`*.component.test.ts`) con
//    @testing-library/svelte sobre jsdom. Acá SÍ cargamos el plugin de Svelte
//    (compila los .svelte) más svelteTesting() (cleanup automático entre tests).
//    `resolve.conditions: ['browser']` es obligatorio: sin eso vitest resuelve
//    el build SSR de Svelte 5 y `onMount` nunca corre.

// Stubs compartidos de los módulos virtuales de SvelteKit (ver src/lib/__mocks__/).
const stubAlias = {
  '$app/environment': path.resolve(__dirname, 'src/lib/__mocks__/app-environment.ts'),
  '$app/navigation': path.resolve(__dirname, 'src/lib/__mocks__/app-navigation.ts'),
  '$env/dynamic/public': path.resolve(__dirname, 'src/lib/__mocks__/env-dynamic-public.ts')
};

export default defineConfig({
  test: {
    projects: [
      {
        resolve: {
          alias: stubAlias
        },
        test: {
          name: 'unit',
          environment: 'node',
          include: ['src/**/*.test.ts'],
          exclude: [...configDefaults.exclude, 'src/**/*.component.test.ts']
        }
      },
      {
        plugins: [svelte(), svelteTesting()],
        resolve: {
          alias: {
            ...stubAlias,
            '$app/stores': path.resolve(__dirname, 'src/lib/__mocks__/app-stores.ts'),
            $lib: path.resolve(__dirname, 'src/lib')
          },
          conditions: ['browser']
        },
        test: {
          name: 'components',
          environment: 'jsdom',
          include: ['src/**/*.component.test.ts']
        }
      }
    ]
  }
});
