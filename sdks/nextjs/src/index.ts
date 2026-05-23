// Re-export both halves for convenience.
// Tree-shaking still keeps the client half out of server bundles when consumers
// import `@faro/nextjs/server` (and vice-versa).
export * from './server';
export * from './client';
