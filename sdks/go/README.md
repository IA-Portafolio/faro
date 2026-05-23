# faro-go

```bash
go get github.com/iaportafolio/faro-go
```

```go
package main

import (
    "context"
    "errors"
    "net/http"
    "time"

    faro "github.com/iaportafolio/faro-go"
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

## Cierre

```go
ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
defer cancel()
faro.Close(ctx)
```
