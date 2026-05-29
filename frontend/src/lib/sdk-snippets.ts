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

/**
 * Signals que sólo entran al backend vía OTLP (no por los SDKs nativos
 * `@iaportafolio/*`, que únicamente envían logs/errores).
 */
export type OtlpSignal = 'metrics' | 'traces';

/** Comando curl con un payload OTLP/JSON mínimo válido para el signal. */
export function otlpCurlProbe(project: Project, signal: OtlpSignal): string {
  const base = apiBase();
  const t = project.ingest_token;
  const resource = `"resource": {"attributes": [{"key":"service.name","value":{"stringValue":"mi-servicio"}}]}`;
  if (signal === 'metrics') {
    return `curl -X POST ${base}/v1/metrics \\
  -H "Authorization: Bearer ${t}" \\
  -H "Content-Type: application/json" \\
  -d '{
    "resourceMetrics": [{
      ${resource},
      "scopeMetrics": [{
        "metrics": [{
          "name": "demo.hits",
          "sum": {
            "aggregationTemporality": 2,
            "isMonotonic": true,
            "dataPoints": [{"asInt":"1","timeUnixNano":"'$(date +%s%N)'"}]
          }
        }]
      }]
    }]
  }'`;
  }
  return `curl -X POST ${base}/v1/traces \\
  -H "Authorization: Bearer ${t}" \\
  -H "Content-Type: application/json" \\
  -d '{
    "resourceSpans": [{
      ${resource},
      "scopeSpans": [{
        "spans": [{
          "traceId": "5b8aa5a2d2c872e8321cf37308d69df2",
          "spanId":  "051581bf3cb55c13",
          "name":    "demo-span",
          "kind":    1,
          "startTimeUnixNano": "'$(date +%s%N)'",
          "endTimeUnixNano":   "'$(date +%s%N)'"
        }]
      }]
    }]
  }'`;
}

/**
 * Snippets de instrumentación OTel para signals que los SDKs `@iaportafolio/*`
 * no cubren (métricas y trazas). Se renderizan en el empty state de esas
 * páginas en lugar de `snippetsFor()`, que sólo configura logging.
 */
export function otlpSnippetsFor(p: Project, signal: OtlpSignal): Snippet[] {
  const base = apiBase();
  const t = p.ingest_token;
  if (signal === 'metrics') {
    return [
      {
        id: 'otel-node',
        label: 'Node.js (OTel)',
        group: 'backend',
        install:
          'npm i @opentelemetry/sdk-metrics @opentelemetry/exporter-metrics-otlp-http @opentelemetry/resources',
        code: `import { MeterProvider, PeriodicExportingMetricReader } from '@opentelemetry/sdk-metrics';
import { OTLPMetricExporter } from '@opentelemetry/exporter-metrics-otlp-http';
import { Resource } from '@opentelemetry/resources';

const exporter = new OTLPMetricExporter({
  url: '${base}/v1/metrics',
  headers: { Authorization: 'Bearer ${t}' },
});

const provider = new MeterProvider({
  resource: new Resource({ 'service.name': 'mi-servicio' }),
  readers: [new PeriodicExportingMetricReader({ exporter, exportIntervalMillis: 10_000 })],
});

const meter = provider.getMeter('mi-app');
const requests = meter.createCounter('http.requests.total');
requests.add(1, { route: '/api/foo', status: '200' });`
      },
      {
        id: 'otel-python',
        label: 'Python (OTel)',
        group: 'backend',
        install:
          'pip install opentelemetry-api opentelemetry-sdk opentelemetry-exporter-otlp-proto-http',
        code: `from opentelemetry import metrics
from opentelemetry.sdk.metrics import MeterProvider
from opentelemetry.sdk.metrics.export import PeriodicExportingMetricReader
from opentelemetry.exporter.otlp.proto.http.metric_exporter import OTLPMetricExporter
from opentelemetry.sdk.resources import Resource

exporter = OTLPMetricExporter(
    endpoint='${base}/v1/metrics',
    headers={'Authorization': 'Bearer ${t}'},
)
reader = PeriodicExportingMetricReader(exporter, export_interval_millis=10_000)
metrics.set_meter_provider(MeterProvider(
    resource=Resource.create({'service.name': 'mi-servicio'}),
    metric_readers=[reader],
))

meter = metrics.get_meter('mi-app')
counter = meter.create_counter('http.requests.total')
counter.add(1, {'route': '/api/foo', 'status': '200'})`
      },
      {
        id: 'otel-go',
        label: 'Go (OTel)',
        group: 'backend',
        install:
          'go get go.opentelemetry.io/otel/sdk/metric go.opentelemetry.io/otel/exporters/otlp/otlpmetric/otlpmetrichttp',
        code: `import (
  "context"
  "time"
  "go.opentelemetry.io/otel"
  "go.opentelemetry.io/otel/attribute"
  "go.opentelemetry.io/otel/exporters/otlp/otlpmetric/otlpmetrichttp"
  sdkmetric "go.opentelemetry.io/otel/sdk/metric"
  "go.opentelemetry.io/otel/sdk/resource"
  semconv "go.opentelemetry.io/otel/semconv/v1.21.0"
)

exporter, _ := otlpmetrichttp.New(context.Background(),
  otlpmetrichttp.WithEndpointURL("${base}/v1/metrics"),
  otlpmetrichttp.WithHeaders(map[string]string{"Authorization": "Bearer ${t}"}),
)
provider := sdkmetric.NewMeterProvider(
  sdkmetric.WithResource(resource.NewWithAttributes(semconv.SchemaURL, semconv.ServiceName("mi-servicio"))),
  sdkmetric.WithReader(sdkmetric.NewPeriodicReader(exporter, sdkmetric.WithInterval(10*time.Second))),
)
otel.SetMeterProvider(provider)

counter, _ := provider.Meter("mi-app").Int64Counter("http.requests.total")
counter.Add(context.Background(), 1, sdkmetric.WithAttributes(attribute.String("route", "/api/foo")))`
      },
      {
        id: 'otel-env',
        label: 'Variables OTel',
        group: 'otros',
        install: '# cualquier SDK OTel oficial respeta estas vars',
        code: `export OTEL_EXPORTER_OTLP_ENDPOINT=${base}
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer ${t}"
export OTEL_SERVICE_NAME=mi-servicio
export OTEL_METRICS_EXPORTER=otlp`
      },
      {
        id: 'otel-curl',
        label: 'curl',
        group: 'otros',
        install: '# zero install',
        code: otlpCurlProbe(p, 'metrics')
      }
    ];
  }
  // traces
  return [
    {
      id: 'otel-node',
      label: 'Node.js (OTel)',
      group: 'backend',
      install:
        'npm i @opentelemetry/sdk-trace-node @opentelemetry/exporter-trace-otlp-http @opentelemetry/resources',
      code: `import { NodeTracerProvider } from '@opentelemetry/sdk-trace-node';
import { BatchSpanProcessor } from '@opentelemetry/sdk-trace-base';
import { OTLPTraceExporter } from '@opentelemetry/exporter-trace-otlp-http';
import { Resource } from '@opentelemetry/resources';

const exporter = new OTLPTraceExporter({
  url: '${base}/v1/traces',
  headers: { Authorization: 'Bearer ${t}' },
});

const provider = new NodeTracerProvider({
  resource: new Resource({ 'service.name': 'mi-servicio' }),
});
provider.addSpanProcessor(new BatchSpanProcessor(exporter));
provider.register();

const tracer = provider.getTracer('mi-app');
const span = tracer.startSpan('charge-order');
try { /* ... */ } finally { span.end(); }`
    },
    {
      id: 'otel-python',
      label: 'Python (OTel)',
      group: 'backend',
      install:
        'pip install opentelemetry-api opentelemetry-sdk opentelemetry-exporter-otlp-proto-http',
      code: `from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import BatchSpanProcessor
from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.resources import Resource

exporter = OTLPSpanExporter(
    endpoint='${base}/v1/traces',
    headers={'Authorization': 'Bearer ${t}'},
)
provider = TracerProvider(resource=Resource.create({'service.name': 'mi-servicio'}))
provider.add_span_processor(BatchSpanProcessor(exporter))
trace.set_tracer_provider(provider)

with trace.get_tracer('mi-app').start_as_current_span('charge-order'):
    cobrar(pedido)`
    },
    {
      id: 'otel-go',
      label: 'Go (OTel)',
      group: 'backend',
      install:
        'go get go.opentelemetry.io/otel/sdk/trace go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp',
      code: `import (
  "context"
  "go.opentelemetry.io/otel"
  "go.opentelemetry.io/otel/exporters/otlp/otlptrace/otlptracehttp"
  sdktrace "go.opentelemetry.io/otel/sdk/trace"
  "go.opentelemetry.io/otel/sdk/resource"
  semconv "go.opentelemetry.io/otel/semconv/v1.21.0"
)

exporter, _ := otlptracehttp.New(context.Background(),
  otlptracehttp.WithEndpointURL("${base}/v1/traces"),
  otlptracehttp.WithHeaders(map[string]string{"Authorization": "Bearer ${t}"}),
)
provider := sdktrace.NewTracerProvider(
  sdktrace.WithBatcher(exporter),
  sdktrace.WithResource(resource.NewWithAttributes(semconv.SchemaURL, semconv.ServiceName("mi-servicio"))),
)
otel.SetTracerProvider(provider)

ctx, span := provider.Tracer("mi-app").Start(context.Background(), "charge-order")
defer span.End()`
    },
    {
      id: 'otel-env',
      label: 'Variables OTel',
      group: 'otros',
      install: '# cualquier SDK OTel oficial respeta estas vars',
      code: `export OTEL_EXPORTER_OTLP_ENDPOINT=${base}
export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
export OTEL_EXPORTER_OTLP_HEADERS="Authorization=Bearer ${t}"
export OTEL_SERVICE_NAME=mi-servicio
export OTEL_TRACES_EXPORTER=otlp`
    },
    {
      id: 'otel-curl',
      label: 'curl',
      group: 'otros',
      install: '# zero install',
      code: otlpCurlProbe(p, 'traces')
    }
  ];
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
