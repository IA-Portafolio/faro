// Stub de `$app/environment` para tests unitarios fuera del runtime de SvelteKit.
// En tests siempre estamos en "server", no en browser.
export const browser = false;
export const dev = false;
export const building = false;
export const version = 'test';
