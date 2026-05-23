# Faro SDKs

Librerías cliente para enviar logs y excepciones a Faro desde tu aplicación. Todos los SDKs comparten la misma API conceptual:

| Método                        | Qué hace                                                    |
| ----------------------------- | ----------------------------------------------------------- |
| `init({ token, service, ... })` | Configura el SDK (una sola vez al arranque)               |
| `log({ level, message, ... })`  | Envía un log estructurado                                 |
| `info(msg, attrs)`            | Atajo para `log({ level: 'INFO', ... })`                    |
| `warn(msg, attrs)`            | Atajo                                                       |
| `error(msg, attrs)`           | Atajo                                                       |
| `captureException(err, ctx)`  | Envía una excepción con stack trace + atributos             |
| `flush()`                     | Espera a que el buffer pendiente se envíe (úsalo al exit)   |
| `close()`                     | Cierra el SDK; tras esto los handlers globales se desinstalan |

Todos los SDKs implementan **auto-captura de excepciones no manejadas** (`uncaughtException` en Node, `sys.excepthook` en Python, `FlutterError.onError` en Flutter, etc.) y buffering asíncrono para no bloquear el código del usuario.

## Configuración común

```typescript
init({
  endpoint: "https://faro.iaportafolio.com",  // base URL de tu Faro
  token:    "tu-token-de-proyecto",            // visible en /projects → SDK
  service:  "mi-app",                          // service.name OTel
  environment: "production",                   // dev/staging/production
  release:  "v1.2.3",                          // opcional, para correlacionar con deploy
  attributes: { region: "eu-west-1" },         // adjuntos a TODOS los eventos
  // Tuning opcional:
  flushIntervalMs: 750,
  maxBatchSize: 200,
  maxQueueSize: 10_000,
})
```

## Endpoint que usan internamente

Los SDKs hablan con el endpoint nativo:

```http
POST /api/v1/ingest/logs
Authorization: Bearer <token-de-proyecto>
Content-Type: application/json

{ "service": "mi-app", "logs": [ /* batch */ ] }
```

Si prefieres **OpenTelemetry** estándar en lugar del SDK propio, apunta el OTLP exporter a `https://faro.iaportafolio.com/v1/logs|traces|metrics` con `Authorization: Bearer <token>` y `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`.

## SDKs disponibles

| Lenguaje / Plataforma | Carpeta              | Instalación                                   |
| --------------------- | -------------------- | --------------------------------------------- |
| Node.js + TypeScript  | [`node/`](./node)    | `npm install @faro/node`                      |
| Next.js               | [`nextjs/`](./nextjs)| `npm install @faro/nextjs`                    |
| Expo / React Native   | [`expo/`](./expo)    | `npm install @faro/expo`                      |
| Python                | [`python/`](./python)| `pip install faro-sdk`                        |
| Go                    | [`go/`](./go)        | `go get github.com/iaportafolio/faro-go`      |
| Flutter (Dart)        | [`flutter/`](./flutter)| `flutter pub add faro_sdk`                  |
| Kotlin (Android + JVM)| [`kotlin/`](./kotlin)| `implementation("com.iaportafolio:faro:0.1.0")` |

## Contrato de severidades

| Texto      | Número OTel |
| ---------- | ----------- |
| `TRACE`    | 1           |
| `DEBUG`    | 5           |
| `INFO`     | 9           |
| `WARN`     | 13          |
| `ERROR`    | 17          |
| `FATAL`    | 21          |

Las excepciones se mandan como `severity_text="ERROR"` y se etiquetan con `attributes.exception.type`, `exception.message` y `exception.stacktrace` — el agrupador de errores de Faro las convierte en issues automáticamente.

## Filosofía

- **Sin lock-in**: si decides cambiar a otro backend OTLP, los SDKs no te atan. El endpoint nativo es opcional, OTel es el camino estándar.
- **Sin captura mágica de queries de DB ni de requests HTTP** en este MVP — eso ya lo hacen los SDKs oficiales de OpenTelemetry. Para algo más rico, combina Faro con un OTel SDK.
- **Pocas líneas, mantenible**: cada SDK cabe en ~300 líneas de código. Lo que el SDK no haga, lo hace OTel.
