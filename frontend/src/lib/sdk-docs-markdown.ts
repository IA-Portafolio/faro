/**
 * Render de la referencia de SDKs a **texto/Markdown**.
 *
 * Sirve a dos endpoints públicos pensados para que LLMs y crawlers puedan leer
 * la documentación sin ejecutar el SPA (la página `/docs` se renderiza en
 * cliente, así que un `GET /docs` sin JS devuelve un shell vacío):
 *   - `GET /llms.txt`  → índice conciso (convención llms.txt).
 *   - `GET /docs.md`   → referencia completa, todos los SDKs y métodos.
 *
 * Fuente única: `sdk-docs.ts`. Si cambian los métodos allí, este texto cambia
 * solo — no hay que mantener dos copias.
 */

import {
  sdks,
  profileDefaults,
  commonOptions,
  severities,
  productMatrix,
  totalMethods,
  type SdkDoc
} from './sdk-docs';

/** Bloque de un SDK en Markdown. */
function sdkSection(sdk: SdkDoc): string {
  const d = profileDefaults[sdk.profile];
  const lines: string[] = [];
  lines.push(`## ${sdk.name}`);
  lines.push('');
  lines.push(`- **Lenguaje / runtime:** ${sdk.language}`);
  lines.push(`- **Paquete:** \`${sdk.pkg}\``);
  lines.push(`- **Instalación:** \`${sdk.install}\``);
  lines.push(`- **Perfil:** ${d.label} (flush ${d.flushMs}ms · batch ${d.batch} · cola ${d.queue})`);
  lines.push(`- **Capacidades:** ${sdk.capabilities.join(', ')}`);
  lines.push('');
  lines.push(sdk.blurb);
  lines.push('');
  lines.push('### Inicialización');
  lines.push('');
  lines.push('```' + sdk.lang);
  lines.push(sdk.initExample);
  lines.push('```');
  lines.push('');
  lines.push('### Métodos');
  lines.push('');
  for (const g of sdk.groups) {
    lines.push(`#### ${g.title}`);
    if (g.note) lines.push(`_${g.note}_`);
    lines.push('');
    for (const m of g.methods) {
      const ret = m.returns ? ` → \`${m.returns}\`` : '';
      lines.push(`- \`${m.signature}\`${ret} — ${m.summary}`);
    }
    lines.push('');
  }
  return lines.join('\n');
}

/** Referencia completa en Markdown (para `/docs.md`). */
export function renderFullMarkdown(baseUrl: string): string {
  const out: string[] = [];
  out.push('# Faro — Referencia de SDKs y API');
  out.push('');
  out.push(
    'Faro es una plataforma de observabilidad y product analytics auto-hospedada ' +
      '(logs, trazas, métricas, errores, eventos de producto, feature flags y monitores). ' +
      'Este documento describe **todos los SDKs y todos sus métodos públicos** para que ' +
      'puedas integrarlos correctamente.'
  );
  out.push('');
  out.push(`- **Instancia:** ${baseUrl}`);
  out.push(`- **SDKs:** ${sdks.length} · **Métodos documentados:** ${totalMethods()}`);
  out.push('- **Endpoint nativo de ingesta:** `POST /api/v1/ingest/logs` y `POST /api/v1/ingest/events` con `Authorization: Bearer <token-de-proyecto>`.');
  out.push('- **OpenTelemetry (sin lock-in):** apunta tu OTLP exporter a `/v1/logs`, `/v1/traces`, `/v1/metrics` con `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` y el mismo header `Authorization`.');
  out.push('');
  out.push('Todos los SDKs comparten la misma API conceptual: `init` configura una vez; ' +
    '`info/warn/warning/error` y `log` envían logs; `captureException` reporta errores; ' +
    '`track/identify/alias` (+ `page`/`screen` donde aplica) son product analytics estilo Segment/PostHog; ' +
    '`flush`/`close` drenan el buffer en el cierre. La auto-captura de excepciones no manejadas y el ' +
    'buffering asíncrono están activados por defecto.');
  out.push('');
  out.push('---');
  out.push('');

  // Índice
  out.push('## Índice de SDKs');
  out.push('');
  for (const s of sdks) {
    out.push(`- **${s.name}** — \`${s.install}\``);
  }
  out.push('');
  out.push('---');
  out.push('');

  // Cada SDK
  for (const s of sdks) {
    out.push(sdkSection(s));
    out.push('---');
    out.push('');
  }

  // Referencia común
  out.push('## Opciones comunes de `init()`');
  out.push('');
  out.push('| Opción | Tipo | Default | Descripción |');
  out.push('| --- | --- | --- | --- |');
  for (const o of commonOptions) {
    out.push(`| \`${o.name}\` | ${o.type} | ${o.default} | ${o.desc.replace(/\|/g, '\\|')} |`);
  }
  out.push('');

  out.push('## Contrato de severidades');
  out.push('');
  out.push('| Texto | Número OTel |');
  out.push('| --- | --- |');
  for (const s of severities) out.push(`| ${s.text} | ${s.num} |`);
  out.push('');
  out.push('Las excepciones se envían como `severity_text="ERROR"` con `exception.type`, `exception.message` y `exception.stacktrace`.');
  out.push('');

  out.push('## Disponibilidad de la API de producto');
  out.push('');
  out.push('| SDK | track | identify | page | screen | alias |');
  out.push('| --- | :--: | :--: | :--: | :--: | :--: |');
  const yn = (b: boolean) => (b ? '✔' : '—');
  for (const r of productMatrix) {
    out.push(`| ${r.sdk} | ${yn(r.track)} | ${yn(r.identify)} | ${yn(r.page)} | ${yn(r.screen)} | ${yn(r.alias)} |`);
  }
  out.push('');
  out.push('`page` solo existe donde hay routing de cliente (RUM web); `screen` solo en móvil.');
  out.push('');
  out.push('---');
  out.push('');
  out.push('_Generado desde la fuente única de los SDKs. Versión navegable y con buscador en `/docs`._');
  out.push('');

  return out.join('\n');
}

/** Índice estilo llms.txt (para `/llms.txt`). */
export function renderLlmsIndex(baseUrl: string): string {
  const out: string[] = [];
  out.push('# Faro');
  out.push('');
  out.push(
    '> Plataforma de observabilidad y product analytics auto-hospedada. SDKs para ' +
      'Node.js, Next.js, Expo/React Native, Python, Go, Flutter y Kotlin/Android que ' +
      'envían logs, errores, eventos de producto, trazas y métricas.'
  );
  out.push('');
  out.push('La referencia completa de cada SDK y de **todos sus métodos** está en un único ' +
    'documento Markdown pensado para lectura por LLMs:');
  out.push('');
  out.push('## Documentación');
  out.push('');
  out.push(`- [Referencia completa de SDKs y API (Markdown)](${baseUrl}/docs.md): instalación, ` +
    'inicialización, todos los métodos por SDK, opciones de `init()`, severidades y API de producto.');
  out.push(`- [Documentación navegable con buscador](${baseUrl}/docs): misma referencia en la UI.`);
  out.push('');
  out.push('## SDKs');
  out.push('');
  for (const s of sdks) {
    out.push(`- ${s.name} (\`${s.pkg}\`): \`${s.install}\``);
  }
  out.push('');
  out.push('## Ingesta');
  out.push('');
  out.push(`- Endpoint nativo: \`POST ${baseUrl}/api/v1/ingest/logs\` y \`/api/v1/ingest/events\` con \`Authorization: Bearer <token-de-proyecto>\`.`);
  out.push(`- OpenTelemetry: OTLP/HTTP-JSON a \`${baseUrl}/v1/logs|traces|metrics\`.`);
  out.push('');
  return out.join('\n');
}
