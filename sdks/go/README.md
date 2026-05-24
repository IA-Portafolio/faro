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

## Cierre

```go
ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
defer cancel()
faro.Close(ctx)
```

## Opciones cross-SDK

`Warning()` (alias de `Warn()`), `ScrubFields`/`DisableHeaderScrub`/`ScrubPatterns` y el hook `BeforeSend` están disponibles con la misma semántica que en el resto de SDKs. Ver [API uniforme entre SDKs](../README.md#api-uniforme-entre-sdks).
