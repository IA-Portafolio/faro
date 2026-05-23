// Re-export both halves for convenience.
// Tree-shaking still keeps the client half out of server bundles when consumers
// import `@iaportafolio/nextjs/server` (and vice-versa).
export * from './server';
export * from './client';
