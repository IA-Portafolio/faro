/**
 * Configuración de carga del layout raíz (SvelteKit).
 *
 * `ssr = false` + `prerender = false`: Faro es una SPA que se renderiza solo en el
 * cliente (todo va contra la API REST con la cookie de sesión), así que no hay
 * render en servidor ni prerender estático de las páginas.
 */
export const ssr = false;
export const prerender = false;
