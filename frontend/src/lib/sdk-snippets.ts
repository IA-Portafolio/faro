/**
 * Snippets de inicialización por SDK para un proyecto concreto.
 *
 * Fuente única para los dos sitios donde se renderizan:
 *   - `/settings/projects` (drawer "SDK" con todos los lenguajes).
 *   - `OnboardingEmpty.svelte` (empty state de logs/traces/errors/etc.).
 *
 * El token del proyecto se inyecta directamente para que el usuario pueda
 * copiar-pegar sin tener que hidratarlo a mano.
 */

import { apiBase, type Project } from './api';

export type SnippetGroup = 'backend' | 'frontend' | 'otros';

export type Snippet = {
  id: string;
  label: string;
  group: SnippetGroup;
  install: string;
  code: string;
};

export const groupLabels: Record<SnippetGroup, string> = {
  backend: 'Backend',
  frontend: 'Frontend',
  otros: 'Otros'
};

export const groupOrder: SnippetGroup[] = ['backend', 'frontend', 'otros'];

/** Comando curl autocontenido que envía un log de prueba al backend. */
export function curlProbe(project: Project): string {
  const base = apiBase();
  const t = project.ingest_token;
  return `curl -X POST ${base}/api/v1/ingest/logs \\
  -H "Authorization: Bearer ${t}" \\
  -H "Content-Type: application/json" \\
  -d '{
    "service": "mi-servicio",
    "logs": [
      { "level": "INFO", "message": "hola desde ${project.slug}" }
    ]
  }'`;
}

export function snippetsFor(p: Project): Snippet[] {
  const base = apiBase();
  const t = p.ingest_token;
  return [
    {
      id: 'node',
      label: 'Node.js',
      group: 'backend',
      install: 'npm install @iaportafolio/node',
      code: `import * as faro from '@iaportafolio/node';

faro.init({
  endpoint: '${base}',
  token: process.env.FARO_TOKEN!,           // ${t.slice(0, 6)}…${t.slice(-4)}
  service: 'mi-servicio',
  environment: 'production',
});

faro.info('arranque ok', { port: 8080 });

try {
  await charge(order);
} catch (err) {
  faro.captureException(err, { tags: { order_id: order.id } });
  throw err;
}`
    },
    {
      id: 'nextjs',
      label: 'Next.js',
      group: 'frontend',
      install: 'npm install @iaportafolio/nextjs @iaportafolio/node',
      code: `// instrumentation.ts
export async function register() {
  const { registerFaro } = await import('@iaportafolio/nextjs/server');
  registerFaro({
    endpoint: '${base}',
    token: process.env.FARO_TOKEN!,
    service: 'mi-next-app',
    environment: process.env.NODE_ENV,
  });
}

// app/faro-client.tsx
'use client';
import { useEffect } from 'react';
import { initFaroClient } from '@iaportafolio/nextjs/client';

export function FaroClient() {
  useEffect(() => {
    initFaroClient({
      endpoint: '${base}',
      token: '${t}',
      service: 'mi-next-app-web',
    });
  }, []);
  return null;
}`
    },
    {
      id: 'python',
      label: 'Python',
      group: 'backend',
      install: 'pip install faro-sdk',
      code: `import faro_sdk as faro

faro.init(
    endpoint='${base}',
    token='${t}',
    service='mi-servicio',
    environment='production',
)

faro.info('arranque ok', port=8080)

try:
    procesar(archivo)
except Exception as exc:
    faro.capture_exception(exc, tags={'archivo': archivo.name})
    raise`
    },
    {
      id: 'go',
      label: 'Go',
      group: 'backend',
      install: 'go get github.com/IA-Portafolio/faro/sdks/go',
      code: `import faro "github.com/IA-Portafolio/faro/sdks/go"

faro.Init(faro.Options{
    Endpoint:    "${base}",
    Token:       "${t}",
    Service:     "mi-servicio",
    Environment: "production",
})
defer faro.Close(context.Background())

faro.Info("arranque ok", map[string]any{"port": 8080})

if err := charge(order); err != nil {
    faro.CaptureException(err, map[string]string{"order_id": order.ID})
}`
    },
    {
      id: 'flutter',
      label: 'Flutter',
      group: 'frontend',
      install: 'flutter pub add faro_sdk',
      code: `import 'package:faro_sdk/faro_sdk.dart';

Faro.run(
  options: const FaroOptions(
    endpoint: '${base}',
    token: '${t}',
    service: 'mi-app-mobile',
    environment: 'production',
  ),
  appRunner: () => runApp(const MyApp()),
);

// más adelante…
Faro.instance.info('login ok', {'user_id': 42});

try { await pagar(); } catch (e, st) {
  Faro.instance.captureException(e, stack: st, tags: {'flow': 'checkout'});
  rethrow;
}`
    },
    {
      id: 'kotlin',
      label: 'Kotlin / Android',
      group: 'frontend',
      install: 'implementation("com.iaportafolio:faro:0.1.0")',
      code: `import com.iaportafolio.faro.Faro
import com.iaportafolio.faro.FaroOptions

Faro.init(FaroOptions(
    endpoint = "${base}",
    token = "${t}",
    service = "android-app",
    environment = "production",
    release = BuildConfig.VERSION_NAME,
))

Faro.info("login ok", mapOf("user_id" to user.id))

try { pay() } catch (e: Throwable) {
    Faro.captureException(e, mapOf("flow" to "checkout"))
    throw e
}`
    },
    {
      id: 'expo',
      label: 'Expo / React Native',
      group: 'frontend',
      install: 'npx expo install @iaportafolio/expo',
      code: `import * as faro from '@iaportafolio/expo';

faro.init({
  endpoint: '${base}',
  token: process.env.EXPO_PUBLIC_FARO_TOKEN!,
  service: 'mi-app-mobile',
  environment: __DEV__ ? 'dev' : 'production',
});

faro.info('app montada');

try {
  await pagar();
} catch (err) {
  faro.captureException(err, { tags: { flow: 'checkout' } });
}`
    },
    {
      id: 'otlp',
      label: 'OpenTelemetry',
      group: 'otros',
      install: '# usa el OTel SDK oficial de tu lenguaje',
      code: `# Configura tu OTel SDK con estas variables:
export OTEL_EXPORTER_OTLP_ENDPOINT=${base}
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer ${t}"
export OTEL_SERVICE_NAME=mi-servicio

# Logs van a /v1/logs, trazas a /v1/traces, métricas a /v1/metrics.`
    },
    {
      id: 'curl',
      label: 'curl',
      group: 'otros',
      install: '# zero install',
      code: curlProbe(p)
    }
  ];
}
