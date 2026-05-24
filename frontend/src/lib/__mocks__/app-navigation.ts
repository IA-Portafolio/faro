// Stub de `$app/navigation` para tests unitarios fuera del runtime de SvelteKit.
// En tests no hay router real: `goto` solo registra la ruta para que los tests
// puedan inspeccionarla si lo necesitan.
export const lastGoto: { url: string | URL | null } = { url: null };

export function goto(url: string | URL): Promise<void> {
  lastGoto.url = url;
  return Promise.resolve();
}

export function invalidate(): Promise<void> {
  return Promise.resolve();
}

export function invalidateAll(): Promise<void> {
  return Promise.resolve();
}

export function pushState(): void {}
export function replaceState(): void {}
export function preloadCode(): Promise<void> { return Promise.resolve(); }
export function preloadData(): Promise<void> { return Promise.resolve(); }
export function beforeNavigate(): void {}
export function afterNavigate(): void {}
export function onNavigate(): void {}
export function disableScrollHandling(): void {}
export function goto_disable_history_restore(): void {}
