// Stub de `$app/stores` para component tests fuera del runtime de SvelteKit.
// Solo modela lo que las páginas usan hoy: `$page.url` (p.ej. para leer
// `?next=` en /login). El valor se calcula recién al suscribirse — no al
// importar el módulo — para que cada test pueda fijar la URL de jsdom con
// `window.history.replaceState(...)` antes del render y leerla fresca.
import { readable, type Readable } from 'svelte/store';

export const page: Readable<{ url: URL }> = readable(
  { url: new URL('http://localhost/') },
  (set) => {
    set({ url: new URL(window.location.href) });
  }
);
