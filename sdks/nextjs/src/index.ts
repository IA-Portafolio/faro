// Re-exporta ambas mitades por comodidad.
// El tree-shaking igual mantiene la mitad client fuera de los bundles de server cuando los
// consumidores importan `@iaportafolio/nextjs/server` (y viceversa).
export * from './server';
export * from './client';
