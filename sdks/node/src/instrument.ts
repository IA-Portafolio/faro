/**
 * Pre-loader de tracing — diseñado para ser cargado vía `--import`:
 *
 *   node --import @iaportafolio/node/instrument server.js
 *
 * Lee la config del entorno y llama a `initTracing` ANTES de que se importen
 * express, http, pg, etc., que es el único orden válido para que la
 * auto-instrumentación de OTel pueda hacer monkey-patching.
 *
 * Env vars:
 *   FARO_ENDPOINT          — URL de Faro. Requerida.
 *   FARO_INGEST_TOKEN      — token de proyecto. Requerida.
 *   OTEL_SERVICE_NAME      — service.name. Requerida (también acepta FARO_SERVICE_NAME).
 *   OTEL_SERVICE_VERSION   — service.version. Opcional (también FARO_SERVICE_VERSION).
 *   DEPLOYMENT_ENVIRONMENT — deployment.environment. Opcional (también NODE_ENV).
 *   FARO_TRACES_ENDPOINT   — override del endpoint completo de traces. Opcional.
 *   FARO_DISABLED_INSTRUMENTATIONS — lista separada por comas. Opcional.
 *
 * Si faltan endpoint/token/service, emite un warning a stderr y sigue. No
 * lanza para no romper el boot del proceso del usuario.
 */

import { initTracing } from './tracing.js';

const endpoint = process.env.FARO_ENDPOINT;
const token = process.env.FARO_INGEST_TOKEN;
const service = process.env.OTEL_SERVICE_NAME ?? process.env.FARO_SERVICE_NAME;
const release = process.env.OTEL_SERVICE_VERSION ?? process.env.FARO_SERVICE_VERSION;
const environment = process.env.DEPLOYMENT_ENVIRONMENT ?? process.env.NODE_ENV;
const tracesEndpoint = process.env.FARO_TRACES_ENDPOINT;
const disabled = process.env.FARO_DISABLED_INSTRUMENTATIONS
  ?.split(',')
  .map((s) => s.trim())
  .filter(Boolean);

if (endpoint && token && service) {
  initTracing({
    endpoint,
    token,
    service,
    tracesEndpoint,
    release,
    environment,
    disabledInstrumentations: disabled,
  });
} else {
  const missing: string[] = [];
  if (!endpoint) missing.push('FARO_ENDPOINT');
  if (!token) missing.push('FARO_INGEST_TOKEN');
  if (!service) missing.push('OTEL_SERVICE_NAME');
  console.warn(
    `[faro/instrument] tracing no inicializado — faltan env vars: ${missing.join(', ')}`,
  );
}
