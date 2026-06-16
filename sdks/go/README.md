# faro-go

> **Perfil de defaults:** `server` — flush 750ms · batch 200 · queue 10 000. Ver [perfiles](../README.md#perfiles-de-defaults).

```bash
go get github.com/IA-Portafolio/faro/sdks/go
```

```go
package main

import (
    "context"
    "errors"
    "net/http"
    "time"

    faro "github.com/IA-Portafolio/faro/sdks/go"
)

func main() {
    if err := faro.Init(faro.Options{
        Endpoint:    "https://faro.iaportafolio.com",
        Token:       "...",                          // /projects → SDK
        Service:     "checkout",
        Environment: "production",
        Release:     "v1.4.2",
        Attributes:  map[string]string{"region": "eu-west-1"},
    }); err != nil {
        panic(err)
    }
    defer faro.Close(context.Background())

    faro.Info("servidor arrancado", map[string]any{"port": 8080})

    mux := http.NewServeMux()
    mux.HandleFunc("/charge", func(w http.ResponseWriter, r *http.Request) {
        if err := charge(r); err != nil {
            faro.CaptureException(err, map[string]string{
                "order_id":    r.URL.Query().Get("order_id"),
                "http.method": r.Method,
            })
            http.Error(w, "fallo", http.StatusInternalServerError)
            return
        }
        w.WriteHeader(http.StatusNoContent)
    })

    // Envuelve tu mux para capturar panics de cualquier handler:
    http.ListenAndServe(":8080", faro.Default().HTTPMiddleware(mux))
}

func charge(r *http.Request) error { return errors.New("timeout del upstream") }
```

## Captura de panics en goroutines

Go no tiene un "handler global de panics" como Node o Python. Defer en cada entry point:

```go
go func() {
    defer faro.Default().Recover(map[string]string{"worker": "billing"})
    procesar()
}()
```

`Recover` reporta el panic y luego lo re-lanza para que el comportamiento de Go no cambie.

## Auto-correlación con traces

Go pasa el trace activo por `context.Context`. Usa `TrackContext(ctx, ...)` para que Faro adjunte `trace_id`/`span_id`. El middleware de Faro copia el header W3C `traceparent` al contexto del request:

```go
mux.HandleFunc("/checkout", func(w http.ResponseWriter, r *http.Request) {
    faro.TrackContext(r.Context(), "checkout_completed", map[string]any{"amount": 99.50})
    w.WriteHeader(http.StatusNoContent)
})

http.ListenAndServe(":8080", faro.Default().HTTPMiddleware(mux))
```

Si ya usas OpenTelemetry, puedes conectar tu extractor sin añadir una dependencia al SDK:

```go
faro.Init(faro.Options{
    Endpoint: "...",
    Token: "...",
    Service: "checkout",
    TraceContext: func(ctx context.Context) faro.TraceContext {
        sc := trace.SpanContextFromContext(ctx)
        return faro.TraceContext{TraceID: sc.TraceID().String(), SpanID: sc.SpanID().String()}
    },
})
```

## Tracing (OpenTelemetry)

El SDK trae su propio exporter OTLP/HTTP/JSON y un `TracerProvider` inicializable
con una llamada. Tras `InitTracing`, cualquier librería auto-instrumentada
(`otelhttp`, `otelgrpc`, `otelsql`, `otelpgx`) exporta a Faro automáticamente:

```go
faro.InitTracing(faro.TracingOptions{
    Endpoint:    "https://faro.iaportafolio.com",
    Token:       "...",
    Service:     "checkout",
    Environment: "production",
    Release:     "v1.4.2",
})
defer faro.ShutdownTracing(context.Background())
```

Spans manuales:

```go
ctx, span := faro.StartSpan(ctx, "db-query", faro.SpanOptions{
    Kind: faro.SpanKindInternal,
    Attributes: map[string]any{"db.system": "postgresql"},
})
defer span.End()

// o con WithSpan
err := faro.WithSpan(ctx, "charge-order", func(ctx context.Context, span *faro.Span) error {
    return charge(order)
}, faro.SpanOptions{Kind: faro.SpanKindInternal})
```

API disponible: `StartSpan(ctx, name, opts)`, `WithSpan(ctx, name, fn, opts)`,
`InitTracing(opts)`, `FlushTracing(ctx)`, `ShutdownTracing(ctx)`, `GetTracer()`,
`SpanFromContext(ctx)`, `ContextWithSpan(ctx, span)`, `WithTraceparent(ctx, header)`.

### Middleware de Gin

Para Gin, el subpaquete `ginfaro` abre un span SERVER por request:

```go
import "github.com/IA-Portafolio/faro/sdks/go/ginfaro"

r := gin.New()
r.Use(ginfaro.Tracing()) // crea un span SERVER por request
```

El span hereda el `traceparent` entrante y lo propaga al response. Logs emitidos
con `faro.InfoContext(c.Request.Context(), ...)` dentro del handler auto-heredan
`trace_id`/`span_id`.

### Auto-instrumentación con otelhttp

Para propagar traces en requests HTTP salientes sin instrumentación manual:

```go
import "go.opentelemetry.io/contrib/instrumentation/net/http/otelhttp"

client := &http.Client{
    Transport: otelhttp.NewTransport(http.DefaultTransport),
}
// cada Do() del client genera un span CLIENT auto-emitido a Faro
```

## Opciones de init

### `faro.Options`

| Campo | Default | Descripción |
| ------ | ------- | ----------- |
| `FlushInterval` | `750ms` | Cadencia de flush. |
| `MaxBatchSize` | `200` | Eventos por POST (logs/events). |
| `MaxQueueSize` | `10000` | Cap del canal. Al llenarse bloquea o descarta. |
| `HTTPTimeout` | `5s` | Timeout del cliente HTTP. |
| `HTTPClient` | `http.DefaultClient` | Inyectable para tests o custom transport. |

### `faro.TracingOptions`

| Campo | Default | Descripción |
| ------ | ------- | ----------- |
| `TracesEndpoint` | `${Endpoint}/v1/traces` | Override del path completo de traces. |
| `ResourceAttributes` | `nil` | Atributos extra del Resource OTel (p. ej. `region`, `cloud.provider`). |
| `Environment` | — | Mapeado a `deployment.environment.name`. |
| `Release` | — | Mapeado a `service.version`. |
| `HTTPClient` | `http.DefaultClient` | Inyectable. |
| `OnInternalError` | `nil` | Callback para fallos internos del exporter. |

## Product analytics

```go
faro.Track("checkout_completed", map[string]any{"amount": 99.50})
faro.TrackContext(ctx, "checkout_completed", map[string]any{"amount": 99.50}) // con trace

faro.Identify("user_42", map[string]any{"email": "a@b.com", "plan": "pro"})
faro.Alias("anon_abc123", "user_42")
```

Ver [API uniforme](../README.md#api-uniforme-entre-sdks) para la semántica de
`anonymous_id`/`distinct_id`/`session_id`.

## Cierre

```go
ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
defer cancel()
faro.Close(ctx)
```

## Opciones cross-SDK

`Warning()` (alias de `Warn()`), `ScrubFields`/`DisableHeaderScrub`/`ScrubPatterns` y el hook `BeforeSend` están disponibles con la misma semántica que en el resto de SDKs. Ver [API uniforme entre SDKs](../README.md#api-uniforme-entre-sdks).
