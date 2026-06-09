# Faro SDKs

> ⚠️ **Al cambiar la API pública de un SDK, actualiza la documentación en el mismo cambio.**
> La doc (`/docs`, `/docs.md`, `/llms.txt`) se alimenta de `frontend/src/lib/sdk-docs.ts`,
> que se mantiene a mano. Reglas y checklist: [`MANTENIMIENTO-DOCS.md`](./MANTENIMIENTO-DOCS.md).

Librerías cliente para enviar logs y excepciones a Faro desde tu aplicación. Todos los SDKs comparten la misma API conceptual:

| Método                        | Qué hace                                                    |
| ----------------------------- | ----------------------------------------------------------- |
| `init({ token, service, ... })` | Configura el SDK (una sola vez al arranque)               |
| `log({ level, message, ... })`  | Envía un log estructurado                                 |
| `info(msg, attrs)`            | Atajo para `log({ level: 'INFO', ... })`                    |
| `warn(msg, attrs)`            | Atajo                                                       |
| `error(msg, attrs)`           | Atajo                                                       |
| `captureException(err, ctx)`  | Envía una excepción con stack trace + atributos             |
| `track(name, props)`          | Envía un product event ([ver abajo](#api-de-tracking-de-eventos-de-producto)) |
| `identify(userId, traits)`    | Asocia eventos futuros al usuario y emite `$identify`       |
| `page(path, props)`           | Page view (web; Next.js cliente, Flutter web)               |
| `screen(name, props)`         | Screen view (mobile; Expo, Flutter, Kotlin)                 |
| `alias(prevId, newId)`        | Fusiona sesión pre-login con user post-login                |
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
  // Ajustes opcionales — los defaults dependen del **perfil** del SDK (ver más abajo).
  flushIntervalMs: 750,
  maxBatchSize: 200,
  maxQueueSize: 10_000,
})
```

## Perfiles de defaults

No todos los SDKs viven en el mismo entorno: un proceso Node de larga duración tolera colas de decenas de miles de eventos, pero un navegador o un móvil no. Para que los defaults sean predecibles, cada SDK declara a qué **perfil** pertenece y hereda de ahí su flush/batch/queue baseline:

| Perfil    | `flushIntervalMs` | `maxBatchSize` | `maxQueueSize` | Pensado para                                                            |
| --------- | ----------------- | -------------- | -------------- | ----------------------------------------------------------------------- |
| `server`  | 750               | 200            | 10 000         | Procesos de larga duración con red estable y memoria abundante          |
| `mobile`  | 1500              | 100            | 5 000          | Apps nativas: batería, red intermitente, memoria limitada               |
| `browser` | 2000              | 100            | 2 000          | Pestañas: vida corta, ancho de banda compartido, riesgo de cierre súbito |

Cómo leer la tabla: el perfil define el **orden de magnitud**, no un número sagrado. Un SDK puede salirse ±50 % si tiene una razón (p.ej. Expo flushea un poco más lento por el coste del bridge JS↔nativo). Lo que **no** debería pasar es que dos SDKs del mismo perfil difieran 10×.

### Perfil declarado por cada SDK

| SDK              | Perfil    | Defaults reales            | Notas                                                          |
| ---------------- | --------- | -------------------------- | -------------------------------------------------------------- |
| Node.js          | `server`  | 750ms / 200 / 10 000       | baseline del perfil                                            |
| Python           | `server`  | 750ms / 200 / 10 000       | baseline del perfil                                            |
| Go               | `server`  | 750ms / 200 / 10 000       | baseline del perfil                                            |
| Flutter          | `mobile`  | 1500ms / 100 / 5 000       | baseline del perfil                                            |
| Kotlin (Android) | `mobile`  | 1500ms / 100 / 5 000       | baseline del perfil                                            |
| Next.js (client) | `browser` | 2000ms / 100 / 2 000       | baseline del perfil                                            |
| Next.js (server) | `server`  | 750ms / 200 / 10 000       | el subimport `/server` hereda del perfil server                |
| Expo / RN        | `mobile`  | 2500ms / 80 / 2 000        | más conservador que `mobile` por el coste del bridge JS↔nativo y batería |

### Guía para futuros SDKs

Si mañana alguien añade un SDK Java, Ruby, Swift, .NET, etc., el proceso es:

1. Elegir el perfil que mejor describa el runtime (server / mobile / browser).
2. Usar los valores baseline del perfil como defaults, salvo razón técnica concreta y documentada.
3. Declarar el perfil **dos veces** — una en el README del SDK (línea visible al principio) y otra como constante / comentario junto a los defaults en el código.
4. Si los defaults se desvían del baseline, anotar el porqué en la columna "Notas" de la tabla de arriba.

Así un usuario que ya conoce, por ejemplo, el SDK de Node sabe qué esperar del SDK de Python sin tener que leerse el código — y un autor de un SDK nuevo no tiene que reinventar los números.

## API uniforme entre SDKs

Algunas piezas se repiten en los 7 SDKs con el mismo nombre, los mismos defaults y la misma semántica. Si están en este documento, debes poder esperarlas en cualquier SDK (y, si alguien escribe uno nuevo, las debe copiar).

### Alias `warn` / `warning`

El SDK siempre expone **ambos** nombres como aliases del mismo método. Razón: `logging` de Python (y muchos loggers JVM) mapean a `WARNING`; el resto del ecosistema usa `warn`. Que ambos funcionen evita el pinchazo típico al migrar de un logger estándar.

| Lenguaje  | Forma A         | Forma B          |
| --------- | --------------- | ---------------- |
| Node / Next.js / Expo | `faro.warn()`   | `faro.warning()` |
| Python    | `faro.warn()`   | `faro.warning()` |
| Go        | `faro.Warn()`   | `faro.Warning()` |
| Kotlin    | `Faro.warn()`   | `Faro.warning()` |
| Flutter   | `Faro.instance.warn()` | `Faro.instance.warning()` |

El wire siempre lleva `level: "WARN"` — los nombres viven solo en el lado cliente.

### Auto-redacción (scrubbing)

Todos los SDKs aplican un pipeline `compose attrs → scrub → beforeSend → enqueue` **antes** de poner el evento en la cola. La redacción es defensa en profundidad: lo que no sale del cliente no puede filtrarse en tránsito, y complementa al PII redaction server-side.

Tres opciones, mismos defaults en todos los SDKs:

| Opción           | Default                                                                                  | Qué hace                                                                                            |
| ---------------- | ---------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------- |
| `scrubFields`    | `['password','token','secret','authorization','cookie','set-cookie','api_key','apikey']` | Substring match case-insensitive contra la **clave** del atributo → valor reemplazado por `[REDACTED]` |
| `scrubHeaders`   | `true`                                                                                   | Suma `authorization`, `cookie`, `set-cookie` a la lista de needles                                  |
| `scrubPatterns`  | `['jwt','api-key']`                                                                      | Presets de regex aplicados a **values string** y al `message`                                       |

Presets de regex disponibles (mismos en todos los SDKs):

| Preset        | Detecta                                                                                       | Falsos positivos               |
| ------------- | --------------------------------------------------------------------------------------------- | ------------------------------ |
| `email`       | `foo@bar.com`                                                                                 | Bajos                          |
| `jwt`         | `eyJ...`.`...`.`...`                                                                          | Casi nulos — preset por defecto |
| `api-key`     | `sk-...`, `ghp_...`, `xoxb-...`, `AKIA...`, `AIza...`, etc.                                   | Casi nulos — preset por defecto |
| `credit-card` | Cualquier secuencia de 13–19 dígitos con guiones/espacios; **sin Luhn** → falsos positivos en IDs largos | Altos — opt-in   |

Ejemplo (Node):

```ts
faro.init({
  endpoint: '...', token: '...', service: '...',
  scrubFields: [...DEFAULT, 'session_secret', 'private_key'],  // extender
  scrubPatterns: ['jwt', 'api-key', 'email'],                  // activar email también
});
```

### Hook `beforeSend`

Última oportunidad de muestrear, transformar o descartar un evento sin esperar a una release del SDK. Firma uniforme:

```text
beforeSend(entry) -> entry | null   // null = descartar
```

Recibe el `Wire`/`WireEntry`/`WireEvent` (el payload exacto que se va a enviar) **post-scrub**, así que ya viene saneado y el hook puede tomar decisiones sobre datos limpios. Mutar el objeto y devolverlo es válido.

Ejemplo (Python):

```python
def sample_noisy_paths(entry):
    if entry["attributes"].get("http.path", "").startswith("/healthz"):
        return None  # descartar healthchecks
    return entry

faro.init(endpoint="...", token="...", service="...", before_send=sample_noisy_paths)
```

### Graceful shutdown / no-loss-on-exit

El SDK bufferiza para no bloquear, lo que significa que un proceso/pestaña/app que muere de golpe pierde lo que tenga en cola. Cada runtime tiene una salida limpia distinta — el patrón es siempre el mismo: **algún hook de fin de vida → `close()` o `flush()` con timeout acotado**.

| Runtime          | Auto (instalado por el SDK)                                                                      | Lo que tienes que hacer tú                                                                       |
| ---------------- | ------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------ |
| Node             | `uncaughtException`, `unhandledRejection`, `beforeExit` → kick de `flush()`                      | Para `SIGTERM`/`SIGINT`: `process.on('SIGTERM', () => faro.close(5000).then(() => process.exit(0)))` |
| Next.js (server) | igual que Node (lo hereda)                                                                       | igual que Node                                                                                   |
| Next.js (client) | `visibilitychange=hidden` + `pagehide` → `flush()` con `navigator.sendBeacon` o `fetch keepalive`| Nada — todo automático                                                                            |
| Expo / RN        | `AppState 'background'/'inactive'` → `flush()` + `ErrorUtils` para fatales                       | Nada — pero llamar a `close()` antes de re-`init()` libera el listener                            |
| Python           | `atexit` → `close()` con join del worker daemon                                                  | Para `SIGTERM` en contenedores (k8s): `signal.signal(SIGTERM, lambda *_: faro.close())`           |
| Go               | nada (Go no tiene `atexit`)                                                                      | `defer faro.Close(ctx)` o handler de señal explícito                                              |
| Flutter          | `WidgetsBindingObserver` → `flush()` en `paused`/`hidden`/`detached`                             | Nada — `Faro.run()` lo activa por ti                                                              |
| Kotlin (Android) | `Thread.setDefaultUncaughtExceptionHandler` para fatales                                         | En `Activity.onStop()` o `Application.onTerminate()`: `Faro.flush(timeoutMs = 2000)`              |

Notas sobre el cierre acotado:

- `close()` siempre **acepta un timeout** (`timeoutMs` en TS/Expo, `timeout=` en Python, `timeoutMs =` en Kotlin, `timeout:` Duration en Flutter, `ctx` en Go). Si la red está caída no debería bloquear el proceso indefinidamente.
- En Python `close()` hace `worker.join(timeout=...)` además del drenado — el worker es daemon, sin join podría truncarse a mitad de POST.
- En Node `close()` rompe el bucle si la cola no se reduce entre flushes (red caída → no insiste).
- Lo que se pierde si el timeout vence: los eventos que ya estaban en cola pero no llegaron a salir. Llega como mucho **un** batch incompleto al servidor en ese escenario.

## Endpoint que usan internamente

Los SDKs hablan con el endpoint nativo:

```http
POST /api/v1/ingest/logs
Authorization: Bearer <token-de-proyecto>
Content-Type: application/json

{ "service": "mi-app", "logs": [ /* batch */ ] }
```

Si prefieres **OpenTelemetry** estándar en lugar del SDK propio, apunta el OTLP exporter a `https://faro.iaportafolio.com/v1/logs|traces|metrics` con `Authorization: Bearer <token>` y `OTEL_EXPORTER_OTLP_PROTOCOL=http/json`.

## API de tracking de eventos de producto

Los métodos `track` / `identify` / `page` / `screen` / `alias` siguen la convención **Segment / PostHog**, así que cualquiera que venga de allí no tiene que reaprender nada. Los eventos van a un endpoint dedicado (`POST /api/v1/ingest/events`) y persisten en `faro.product_events`, separados de los logs — eso permite consultarlos para funnels, retention, cohortes, etc. sin pelearse con los logs.

```typescript
// Web / Node
faro.track('checkout_completed', { amount: 99.50, currency: 'USD' });
faro.identify('user_42', { email: 'a@b.com', plan: 'pro' });
faro.page('/checkout/success');                    // solo web (Next.js client, Flutter web)
faro.screen('CheckoutSuccess', { source: 'cart' }); // solo mobile (Expo, Flutter, Kotlin)
faro.alias('anon_abc123', 'user_42');              // fusiona sesión pre-login → post-login
```

### Disponibilidad por SDK

| SDK              | `track` | `identify` | `page` | `screen` | `alias` |
| ---------------- | :-----: | :--------: | :----: | :------: | :-----: |
| Node.js          |    ✔    |     ✔      |        |          |    ✔    |
| Python           |    ✔    |     ✔      |        |          |    ✔    |
| Go               |    ✔    |     ✔      |        |          |    ✔    |
| Next.js (server) |    ✔    |     ✔      |        |          |    ✔    |
| Next.js (client) |    ✔    |     ✔      |   ✔    |          |    ✔    |
| Expo / RN        |    ✔    |     ✔      |        |    ✔     |    ✔    |
| Flutter          |    ✔    |     ✔      |   ✔    |    ✔     |    ✔    |
| Kotlin (Android) |    ✔    |     ✔      |        |    ✔     |    ✔    |

`page` solo existe donde hay routing de cliente (RUM); `screen` solo en mobile. Los SDKs server-side se quedan con los tres core (`track`/`identify`/`alias`) porque page/screen no aplican a su contexto.

### Modelo de IDs

| Campo          | Quién lo setea | Cuándo |
| -------------- | -------------- | ------ |
| `anonymous_id` | SDK, automático | En el primer `init()`. Persiste en `localStorage` en navegador (sobrevive a reloads para que `alias` pueda fusionar); regenerado por proceso en Node/Python/Go/Expo/Kotlin/Flutter. |
| `distinct_id`  | `identify(userId)` | Mientras no haya `identify`, el SDK rellena `distinct_id` con `anonymous_id`. Tras `identify`, queda fijado al `userId` y todos los eventos siguientes lo usan. `alias(prev, new)` también lo pisa. |
| `session_id`   | SDK o backend | Si el SDK tiene ciclo de vida de sesión (Next.js client / RUM web, replay, mobile con lifecycle claro), lo manda explícito. Si viene vacío, el backend sesionaliza retroactivamente por gap de 30 min y mantiene `product_sessions`. |

En browser, `anonymous_id` se genera con `crypto.randomUUID()` y queda persistido en `localStorage`. Cuando `identify('user_42')` se invoca por primera vez, el SDK emite automáticamente `$alias` con `properties: { from: anonymous_id, to: 'user_42' }`; los eventos siguientes conservan ambos campos (`distinct_id='user_42'` y el `anonymous_id` original) para permitir joins retrospectivos.

El backend materializa esa relación en `product_user_aliases` y `product_users`. Las queries de usuario expanden `distinct_id = user_42 OR anonymous_id IN (...)`, así el feed de actividad incluye eventos anónimos previos al login.

### Auto-correlación con tracing

Los SDKs server-side adjuntan `trace_id` y `span_id` a product events cuando hay W3C tracecontext activo. En Node/Next server y Python se intenta leer OpenTelemetry si está instalado y también puedes pasar un provider (`traceContext` / `trace_context`). En Go usa `TrackContext(ctx, ...)`; el middleware de Faro copia el header `traceparent` al `context.Context` del request.

Con esto un `faro.track('checkout_completed')` emitido dentro del request que sirvió el checkout queda conectado con el trace backend. En el dashboard, un evento con `trace_id` puede mostrar la acción “ver trace”.

### Wire format

```http
POST /api/v1/ingest/events
Authorization: Bearer <token>
Content-Type: application/json

{
  "service": "mi-app",
  "events": [
    {
      "type": "track",                          // track | identify | page | screen | alias
      "name": "checkout_completed",             // event_name custom (track), path (page), screen (screen), o $identify/$alias
      "timestamp": "2026-05-24T12:34:56.789Z",
      "distinct_id": "user_42",                  // post-identify; pre-identify == anonymous_id
      "anonymous_id": "anon_abc",                // en `alias` lleva el ID PREVIO que se fusiona con distinct_id
      "session_id": "",                          // opcional; si viene vacío, backend deriva sesión por gap
      "properties": { "amount": 99.50, "currency": "USD" },
      "user_properties": { "plan": "pro" },     // solo se manda explícito en `identify`
      "context": { "page.url": "...", "user_agent": "...", "environment": "production" },
      "source": "web",                           // web | mobile | backend
      "trace_id": "4bf92f3577b34da6a3ce929d0e0e4736",
      "span_id": "00f067aa0ba902b7"
    }
  ]
}
```

Eventos especiales — el SDK los traduce a `event_name` PostHog-style automáticamente:

| Método del SDK   | `type`     | `event_name` en la tabla |
| ---------------- | ---------- | ------------------------ |
| `track(name, …)` | `track`    | `name`                   |
| `identify(…)`    | `identify` | `$identify`              |
| `page(path, …)`  | `page`     | `$pageview` (path va a `properties.path`) |
| `screen(name, …)`| `screen`   | `$screen` (name va a `properties.name`)   |
| `alias(prev,new)`| `alias`    | `$alias`                 |

### Comportamiento esperado

- **Buffering compartido**: los events viajan en su propia cola pero respetan el mismo `flushIntervalMs` / `maxBatchSize` / `maxQueueSize` que los logs. Un `flush()` drena ambas colas; un `close()` también. Si una cola se llena, los eventos nuevos se descartan con log a stderr (igual que los logs).
- **`identify` también enriquece logs**: tras `identify(userId)`, los logs siguientes que el SDK envíe llevarán `user.id` (donde aplique — RUM browser, Expo). Es por simetría: si llamás `identify` para tracking, también es razonable que los logs de ese usuario aparezcan asociados a él.
- **`scrub*` se aplica server-side a events también**: el backend pasa las reglas de redacción del proyecto sobre `properties`, `user_properties` y `context` antes de persistir. Los SDKs no scrubean events client-side hoy (a diferencia de logs) — la sensibilidad típica está en logs de errores, no en payloads explícitos de tracking; si quieres scrubbing client-side de events, abre un issue.

### Auto-tracking web (opt-in)

El SDK browser (hoy `@iaportafolio/nextjs/client`, futuro Svelte/vanilla) puede emitir product events automaticamente:

```ts
faro.init({
  autoCapture: {
    pageViews: true,
    clicks: true,
    formSubmissions: true,
    rageClicks: true,
    deadClicks: true,
  },
});
```

`pageViews` emite `page()` en init y navegaciones SPA. `clicks` captura `[data-faro]`, `button` y `a`. `formSubmissions` captura `form[data-faro-form]`. `rageClicks` y `deadClicks` detectan UX rota y emiten `$rage_click` / `$dead_click`.

Esto no cambia los flags legacy de RUM: `captureClicks` y `captureNavigation` siguen generando breadcrumbs; `autoCapture` genera product events y está apagado por defecto.

## Feature flags y experimentos

Los **7 SDKs** evalúan feature flags **localmente** (sin round-trip por evaluación) y, cuando un usuario entra al targeting, emiten un product event `$feature_exposure` que alimenta el A/B testing y el rollback-por-errores de Faro. Ver el flujo completo en [`docs/feature-flags-experiments.md`](../docs/feature-flags-experiments.md).

```typescript
// Node / Next.js / Expo
if (faro.isFeatureEnabled('new-checkout', { distinct_id: 'user_42', properties: { plan: 'pro' } })) {
  // render del treatment
}
```

| SDK              | Firma de evaluación                                                  |
| ---------------- | ------------------------------------------------------------------- |
| Node / Next.js / Expo | `faro.isFeatureEnabled(key, { distinct_id?, properties? })` → `boolean` |
| Python           | `faro.is_feature_enabled(key, distinct_id=None, properties=None)` → `bool` |
| Go               | `faro.IsFeatureEnabled(key, faro.FlagContext{DistinctID, Properties})` → `bool` |
| Kotlin           | `Faro.isFeatureEnabled(key, distinctId?, properties?)` → `Boolean`  |
| Flutter          | `Faro.instance.isFeatureEnabled(key, distinctId:, properties:)` → `bool` |

Semántica uniforme (idéntica en todos los SDKs):

- **Snapshot local + refresh periódico.** El SDK hace `GET /api/v1/ingest/feature-flags` cada `featureFlagRefreshIntervalMs` (default **30 s**; `feature_flag_refresh_interval` en Python, `FeatureFlagRefreshInterval` en Go, etc.). No hay fetch inicial bloqueante: hasta el primer refresh los flags evalúan a `false`. `refreshFeatureFlags()` fuerza un refresh inmediato (útil en arranque/tests).
- **Sticky por `distinct_id`.** El bucket se deriva de `FNV-1a(project:flag:distinct_id) % 100` contra `rollout_percentage`, así un mismo usuario obtiene siempre la misma variante — y el mismo bucket en cualquier SDK/plataforma.
- **`conditions`.** Si el flag trae `conditions.properties`, el usuario solo entra al experimento si **todas** matchean (igualdad estricta) contra el `properties` pasado.
- **`$feature_exposure` deduplicado.** Se emite **una vez** por combinación `(project, flag, distinct_id, variant)` — `variant` es `"B"` (enabled) o `"A"` (control). Viaja por la misma cola de product events (mismo flush/batch/queue).
- **`distinct_id`** se resuelve como `context.distinct_id` → `distinct_id` post-`identify` → `anonymous_id`.

## SDKs disponibles

| Lenguaje / Plataforma | Carpeta              | Instalación                                   |
| --------------------- | -------------------- | --------------------------------------------- |
| Node.js + TypeScript  | [`node/`](./node)    | `npm install @iaportafolio/node`                      |
| Next.js               | [`nextjs/`](./nextjs)| `npm install @iaportafolio/nextjs`                    |
| Expo / React Native   | [`expo/`](./expo)    | `npm install @iaportafolio/expo`                      |
| Python                | [`python/`](./python)| `pip install faro-sdk`                        |
| Go                    | [`go/`](./go)        | `go get github.com/IA-Portafolio/faro/sdks/go`      |
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

## Tests

Cada SDK tiene una suite que cubre **las 6 invariantes mínimas** (publicación a npm/PyPI/Maven/pub.dev es prácticamente irreversible — un bug en `@iaportafolio/<x>@<v>` está en miles de instalaciones a la hora). Las suites se ejecutan en CI vía [`.github/workflows/sdk-tests.yml`](../.github/workflows/sdk-tests.yml) y son **gate de publicación**: si fallan, `publish-sdks.yml` no corre.

| # | Invariante | Verifica |
| --- | ---------- | -------- |
| 1 | Init con opts inválidas | error claro (no falla en silencio) |
| 2 | Log + flush + assert payload | shape del JSON enviado al wire |
| 3 | Queue overflow | al llenar `maxQueueSize` descarta (sin OOM) |
| 4 | Retry on 5xx | re-encola para el siguiente intento; 4xx descarta |
| 5 | Auto-captura de excepciones | `captureException()` compone shape OTel correcto |
| 6 | `close()` flush graceful | no pierde eventos pendientes en shutdown |

Conteos actuales (post 7.D.1):

| SDK     | Carpeta              | Comando local       | Tests |
| ------- | -------------------- | ------------------- | ----- |
| Node    | [`node/`](./node)    | `npm test`          | 12    |
| Next.js | [`nextjs/`](./nextjs)| `npm test`          | 10    |
| Expo    | [`expo/`](./expo)    | `npm test`          | 10    |
| Python  | [`python/`](./python)| `pytest tests/`     | 12    |
| Go      | [`go/`](./go)        | `go test ./...`     | 12    |
| Flutter | [`flutter/`](./flutter)| `flutter test`    | 11    |
| Kotlin  | [`kotlin/`](./kotlin)| `./gradlew test`    | 10    |

Los SDKs Node/Next.js/Expo necesitan correr `npm install && npm run build` antes (el `test` script ya los encadena). Kotlin necesita gradle wrapper; el job de CI lo bootstrappea si falta. Para HTTP mocks cada lenguaje usa lo natural: `http.createServer` en Node, `http.server` de stdlib en Python, `httptest.NewServer` en Go, `HttpServer.bind` de `dart:io` en Flutter, `com.sun.net.httpserver.HttpServer` del JDK en Kotlin.

## Filosofía

- **Sin lock-in**: si decides cambiar a otro backend OTLP, los SDKs no te atan. El endpoint nativo es opcional, OTel es el camino estándar.
- **Sin captura mágica de queries de DB ni de requests HTTP** en este MVP — eso ya lo hacen los SDKs oficiales de OpenTelemetry. Para algo más rico, combina Faro con un OTel SDK.
- **Pocas líneas, mantenible**: cada SDK cabe en ~300 líneas de código. Lo que el SDK no haga, lo hace OTel.
