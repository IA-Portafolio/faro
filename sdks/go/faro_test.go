// Tests unitarios del SDK Go — 4 invariantes mínimas:
//
//  1. queue cap descarta cuando se llena
//  2. retry on 5xx
//  3. BeforeSend filtra (nil → descartar) y transforma
//  4. scrubbing aplica ScrubFields + ScrubPatterns
//
// httptest.NewServer cubre el server-side sin tocar red real.
package faro

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"
)

// ---- helper: server que captura batches ----

type captureServer struct {
	mu       sync.Mutex
	batches  []map[string]any
	calls    atomic.Int32
	nextCode atomic.Int32 // si != 0, devuelve este status; si 0, 200
}

func newCaptureServer() *captureServer {
	return &captureServer{}
}

func (c *captureServer) handler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		c.calls.Add(1)
		body, _ := io.ReadAll(r.Body)
		var parsed map[string]any
		_ = json.Unmarshal(body, &parsed)
		c.mu.Lock()
		c.batches = append(c.batches, parsed)
		c.mu.Unlock()
		code := int(c.nextCode.Load())
		if code == 0 {
			code = 200
		}
		w.WriteHeader(code)
		w.Write([]byte(`{"ok":true}`))
	})
}

func (c *captureServer) snapshot() []map[string]any {
	c.mu.Lock()
	defer c.mu.Unlock()
	out := make([]map[string]any, len(c.batches))
	copy(out, c.batches)
	return out
}

// ---- 1. queue cap ----

func TestQueueCap(t *testing.T) {
	// Endpoint inalcanzable + un solo internal-error sink que cuenta descartes.
	var dropped atomic.Int32
	client, err := New(Options{
		Endpoint:      "http://127.0.0.1:1",
		Token:         "tk",
		Service:       "queue-cap",
		FlushInterval: 100 * time.Second, // sin auto-flush
		MaxQueueSize:  5,
		OnInternalError: func(err error) {
			if strings.Contains(err.Error(), "cola llena") {
				dropped.Add(1)
			}
		},
	})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	defer client.Close(context.Background())

	for i := 0; i < 200; i++ {
		client.Log(SevInfo, "evento", nil)
	}
	if dropped.Load() == 0 {
		t.Fatalf("con MaxQueueSize=5 y 200 logs debería haber descartes; got %d", dropped.Load())
	}
}

// ---- 2. retry sobre 5xx ----

func TestRetryOn5xx(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	cap.nextCode.Store(503)
	client, err := New(Options{
		Endpoint:        srv.URL,
		Token:           "tk",
		Service:         "retry-test",
		FlushInterval:   50 * time.Millisecond,
		OnInternalError: func(error) {}, // silencia
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())

	client.Log(SevInfo, "se reintenta", nil)

	// Espera al primer intento (503)
	waitFor(t, 2*time.Second, func() bool { return cap.calls.Load() >= 1 })
	first := cap.calls.Load()

	cap.nextCode.Store(200)
	waitFor(t, 2*time.Second, func() bool { return cap.calls.Load() > first })

	if cap.calls.Load() <= first {
		t.Fatalf("tras 5xx → 200 debe haber un reintento; calls=%d", cap.calls.Load())
	}
}

// ---- 3. BeforeSend ----

func TestBeforeSendNilDescarta(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	client, err := New(Options{
		Endpoint:      srv.URL,
		Token:         "tk",
		Service:       "bs-discard",
		FlushInterval: 50 * time.Millisecond,
		BeforeSend: func(e *Entry) *Entry {
			if strings.Contains(e.Message, "descarta-me") {
				return nil
			}
			return e
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())

	client.Log(SevInfo, "guarda-me", nil)
	client.Log(SevInfo, "descarta-me", nil)
	client.Log(SevInfo, "también guarda-me", nil)

	if err := client.Flush(2 * time.Second); err != nil {
		t.Fatalf("Flush: %v", err)
	}
	waitFor(t, 2*time.Second, func() bool { return len(cap.snapshot()) >= 1 })

	batches := cap.snapshot()
	if len(batches) < 1 {
		t.Fatalf("server debería haber recibido algo")
	}
	logs := batches[0]["logs"].([]any)
	var msgs []string
	for _, l := range logs {
		msgs = append(msgs, l.(map[string]any)["message"].(string))
	}
	expected := []string{"guarda-me", "también guarda-me"}
	if !equalSlices(msgs, expected) {
		t.Fatalf("BeforeSend no filtró; messages=%v expected=%v", msgs, expected)
	}
}

func TestBeforeSendPuedeTransformar(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	client, err := New(Options{
		Endpoint:      srv.URL,
		Token:         "tk",
		Service:       "bs-mutate",
		FlushInterval: 50 * time.Millisecond,
		BeforeSend: func(e *Entry) *Entry {
			if e.Attributes == nil {
				e.Attributes = map[string]string{}
			}
			e.Attributes["injected"] = "yes"
			return e
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())

	client.Log(SevInfo, "hola", nil)
	_ = client.Flush(2 * time.Second)
	waitFor(t, 2*time.Second, func() bool { return len(cap.snapshot()) >= 1 })

	logs := cap.snapshot()[0]["logs"].([]any)
	attrs := logs[0].(map[string]any)["attributes"].(map[string]any)
	if attrs["injected"] != "yes" {
		t.Fatalf("BeforeSend no añadió attribute; got %v", attrs)
	}
}

// ---- 4. scrubbing ----

func TestScrubFieldsRedactaPorClave(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	client, err := New(Options{
		Endpoint:      srv.URL,
		Token:         "tk",
		Service:       "scrub-fields",
		FlushInterval: 50 * time.Millisecond,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())

	client.Log(SevInfo, "login", map[string]any{
		"user.password":                     "p4ssw0rd",
		"http.request.header.authorization": "Bearer abc",
		"safe.field":                        "visible",
	})
	_ = client.Flush(2 * time.Second)
	waitFor(t, 2*time.Second, func() bool { return len(cap.snapshot()) >= 1 })

	logs := cap.snapshot()[0]["logs"].([]any)
	attrs := logs[0].(map[string]any)["attributes"].(map[string]any)
	if attrs["user.password"] != "[REDACTED]" {
		t.Errorf("user.password no redactado: %v", attrs["user.password"])
	}
	if attrs["http.request.header.authorization"] != "[REDACTED]" {
		t.Errorf("auth header no redactado: %v", attrs["http.request.header.authorization"])
	}
	if attrs["safe.field"] != "visible" {
		t.Errorf("safe.field debería estar intacto, got: %v", attrs["safe.field"])
	}
}

func TestScrubPatternsRedactaJWTyApiKey(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	client, err := New(Options{
		Endpoint:      srv.URL,
		Token:         "tk",
		Service:       "scrub-patterns",
		FlushInterval: 50 * time.Millisecond,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer client.Close(context.Background())

	client.Log(SevInfo, "auth con eyJabc.def.ghi y key sk-abcdefghijklmnop", map[string]any{
		"embedded": "ghp_aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
	})
	_ = client.Flush(2 * time.Second)
	waitFor(t, 2*time.Second, func() bool { return len(cap.snapshot()) >= 1 })

	log := cap.snapshot()[0]["logs"].([]any)[0].(map[string]any)
	msg := log["message"].(string)
	if strings.Contains(msg, "eyJabc") {
		t.Errorf("JWT debe estar redactado en message: %q", msg)
	}
	if strings.Contains(msg, "sk-abcdef") {
		t.Errorf("sk-* debe estar redactado en message: %q", msg)
	}
	attrs := log["attributes"].(map[string]any)
	if attrs["embedded"] != "[REDACTED]" {
		t.Errorf("ghp_* en attribute debe estar redactado: %v", attrs["embedded"])
	}
}

// ---- 5. init con opts inválidas ----

func TestInitSinEndpointDevuelveError(t *testing.T) {
	// Evita que una FARO_ENDPOINT del entorno "rescate" el caso.
	t.Setenv("FARO_ENDPOINT", "")
	t.Setenv("FARO_TOKEN", "")
	_, err := New(Options{Token: "tk", Service: "s"})
	if err == nil {
		t.Fatal("se esperaba error por falta de Endpoint, got nil")
	}
	if !strings.Contains(err.Error(), "Endpoint") || !strings.Contains(err.Error(), "obligatorios") {
		t.Errorf("mensaje no contiene 'Endpoint' u 'obligatorios': %q", err.Error())
	}
}

func TestInitSinTokenDevuelveError(t *testing.T) {
	t.Setenv("FARO_ENDPOINT", "")
	t.Setenv("FARO_TOKEN", "")
	_, err := New(Options{Endpoint: "http://x", Service: "s"})
	if err == nil {
		t.Fatal("se esperaba error por falta de Token, got nil")
	}
	if !strings.Contains(err.Error(), "Token") || !strings.Contains(err.Error(), "obligatorios") {
		t.Errorf("mensaje no contiene 'Token' u 'obligatorios': %q", err.Error())
	}
}

func TestInitAceptaEnvVars(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()
	// Hueco común: olvidar que New() también lee FARO_ENDPOINT/FARO_TOKEN.
	t.Setenv("FARO_ENDPOINT", srv.URL)
	t.Setenv("FARO_TOKEN", "tk-from-env")
	c, err := New(Options{Service: "env-test"})
	if err != nil {
		t.Fatalf("env vars válidas pero New devolvió error: %v", err)
	}
	defer c.Close(context.Background())
	if c == nil {
		t.Fatal("client nil")
	}
}

// ---- 6. log + flush + assert payload (shape del wire) ----

type capturedRequest struct {
	method      string
	path        string
	auth        string
	contentType string
	body        map[string]any
}

func TestPayloadShapeDelWire(t *testing.T) {
	mu := sync.Mutex{}
	var seen []capturedRequest
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		var parsed map[string]any
		_ = json.Unmarshal(body, &parsed)
		mu.Lock()
		seen = append(seen, capturedRequest{
			method:      r.Method,
			path:        r.URL.Path,
			auth:        r.Header.Get("Authorization"),
			contentType: r.Header.Get("Content-Type"),
			body:        parsed,
		})
		mu.Unlock()
		w.WriteHeader(200)
		w.Write([]byte("{}"))
	}))
	defer srv.Close()

	c, err := New(Options{
		Endpoint:      srv.URL,
		Token:         "mi-token",
		Service:       "payload-test",
		Environment:   "prod",
		Release:       "v1.2.3",
		Attributes:    map[string]string{"region": "eu-west-1"},
		FlushInterval: 50 * time.Millisecond,
	})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close(context.Background())

	c.Log(SevWarn, "algo raro", map[string]any{
		"http.status_code": 500,
		"user.id":          "u42",
	})
	_ = c.Flush(2 * time.Second)
	waitFor(t, 2*time.Second, func() bool {
		mu.Lock()
		defer mu.Unlock()
		return len(seen) >= 1
	})

	mu.Lock()
	defer mu.Unlock()
	if len(seen) != 1 {
		t.Fatalf("esperaba 1 POST, got %d", len(seen))
	}
	req := seen[0]
	if req.method != "POST" {
		t.Errorf("method %q != POST", req.method)
	}
	if req.path != "/api/v1/ingest/logs" {
		t.Errorf("path %q != /api/v1/ingest/logs", req.path)
	}
	if req.auth != "Bearer mi-token" {
		t.Errorf("auth %q != Bearer mi-token", req.auth)
	}
	if req.contentType != "application/json" {
		t.Errorf("content-type %q != application/json", req.contentType)
	}

	if req.body["service"] != "payload-test" {
		t.Errorf("service %v != payload-test", req.body["service"])
	}
	logs, ok := req.body["logs"].([]any)
	if !ok || len(logs) != 1 {
		t.Fatalf("logs no es []any o len != 1: %v", req.body["logs"])
	}
	entry := logs[0].(map[string]any)
	if entry["level"] != "WARN" {
		t.Errorf("level %v != WARN", entry["level"])
	}
	if entry["message"] != "algo raro" {
		t.Errorf("message %v != 'algo raro'", entry["message"])
	}
	if ts, _ := entry["timestamp"].(string); !strings.Contains(ts, "T") {
		t.Errorf("timestamp sin 'T': %q", ts)
	}

	attrs := entry["attributes"].(map[string]any)
	checks := map[string]string{
		"region":                 "eu-west-1",
		"deployment.environment": "prod",
		"service.version":        "v1.2.3",
		"http.status_code":       "500", // serializado a string
		"user.id":                "u42",
	}
	for k, want := range checks {
		if attrs[k] != want {
			t.Errorf("attr[%s] = %v, want %s", k, attrs[k], want)
		}
	}
}

// ---- 7. auto-captura de excepciones (Recover sobre panic) ----

func TestRecoverCapturaPanic(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint:        srv.URL,
		Token:           "tk",
		Service:         "auto-capture",
		FlushInterval:   50 * time.Millisecond,
		OnInternalError: func(error) {},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close(context.Background())

	// Recover re-panica tras capturar (preserva la semántica de Go); lo atrapamos
	// en un defer externo para que el test no muera.
	func() {
		defer func() {
			rec := recover()
			if rec == nil {
				t.Fatal("Recover() debería re-panicar para preservar semántica de Go")
			}
		}()
		defer c.Recover(map[string]string{"job": "nightly"})
		panic("¡boom sintético!")
	}()

	// Recover dispara Flush internamente. Damos un pelín más por seguridad.
	waitFor(t, 2*time.Second, func() bool { return len(cap.snapshot()) >= 1 })

	batches := cap.snapshot()
	if len(batches) < 1 {
		t.Fatal("el server debió recibir el evento de auto-captura")
	}
	entry := batches[0]["logs"].([]any)[0].(map[string]any)
	if entry["level"] != "ERROR" {
		t.Errorf("level %v != ERROR", entry["level"])
	}
	attrs := entry["attributes"].(map[string]any)
	if attrs["exception.type"] == "" {
		t.Errorf("exception.type vacío: %v", attrs["exception.type"])
	}
	if msg, _ := attrs["exception.message"].(string); !strings.Contains(msg, "boom sintético") {
		t.Errorf("exception.message no contiene texto del panic: %q", msg)
	}
	if attrs["job"] != "nightly" {
		t.Errorf("tag 'job' no propagado: %v", attrs["job"])
	}
}

// ---- 8. Close() graceful: no pierde eventos en cola ----

func TestCloseDrenaLaCola(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint: srv.URL,
		Token:    "tk",
		Service:  "close-test",
		// Intervalo lejano: si NO fuera por Close(), no llegaría nada.
		FlushInterval: 100 * time.Second,
	})
	if err != nil {
		t.Fatal(err)
	}

	for i := 0; i < 7; i++ {
		c.Log(SevInfo, "evento", map[string]any{"i": i})
	}

	// Close drena por el caso "<-c.closed" del loop y luego espera al wg.
	ctx, cancel := context.WithTimeout(context.Background(), 3*time.Second)
	defer cancel()
	if err := c.Close(ctx); err != nil {
		t.Fatalf("Close devolvió error: %v", err)
	}

	batches := cap.snapshot()
	total := 0
	for _, b := range batches {
		total += len(b["logs"].([]any))
	}
	if total != 7 {
		t.Fatalf("Close debe drenar los 7 eventos; got %d", total)
	}
}

// ---- 8. Product events API: Track / Identify / Alias ----

// Helper: filtra los batches que parecen ser de /ingest/events (tienen "events")
// y devuelve la lista plana de eventos.
func eventsFromCapture(batches []map[string]any) []map[string]any {
	var out []map[string]any
	for _, b := range batches {
		raw, ok := b["events"].([]any)
		if !ok {
			continue
		}
		for _, e := range raw {
			if m, ok := e.(map[string]any); ok {
				out = append(out, m)
			}
		}
	}
	return out
}

func TestTrackEnviaEventoAEndpointEvents(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint:        srv.URL,
		Token:           "tk",
		Service:         "track-test",
		FlushInterval:   50 * time.Millisecond,
		OnInternalError: func(error) {},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close(context.Background())

	c.Track("checkout_completed", map[string]any{"amount": 99.5, "currency": "USD"})
	if err := c.Flush(2 * time.Second); err != nil {
		t.Fatal(err)
	}

	waitFor(t, 2*time.Second, func() bool {
		return len(eventsFromCapture(cap.snapshot())) >= 1
	})
	events := eventsFromCapture(cap.snapshot())
	if len(events) < 1 {
		t.Fatalf("esperaba al menos 1 event; capturado: %+v", cap.snapshot())
	}
	e := events[0]
	if e["type"] != "track" || e["name"] != "checkout_completed" {
		t.Errorf("type/name inesperado: %v / %v", e["type"], e["name"])
	}
	props := e["properties"].(map[string]any)
	if props["amount"] != 99.5 || props["currency"] != "USD" {
		t.Errorf("properties no match: %+v", props)
	}
	if !strings.HasPrefix(e["distinct_id"].(string), "anon_") {
		t.Errorf("pre-identify distinct_id debe empezar con anon_; got %v", e["distinct_id"])
	}
	if e["distinct_id"] != e["anonymous_id"] {
		t.Errorf("pre-identify: distinct_id debe == anonymous_id")
	}
	if e["source"] != "backend" {
		t.Errorf("source = %v, want backend", e["source"])
	}
}

func TestTrackContextAdjuntaTraceContext(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint:        srv.URL,
		Token:           "tk",
		Service:         "trace-context-test",
		FlushInterval:   50 * time.Millisecond,
		OnInternalError: func(error) {},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close(context.Background())

	ctx := WithTraceparent(
		context.Background(),
		"00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01",
	)
	c.TrackContext(ctx, "checkout_completed", nil)
	if err := c.Flush(2 * time.Second); err != nil {
		t.Fatal(err)
	}

	waitFor(t, 2*time.Second, func() bool {
		return len(eventsFromCapture(cap.snapshot())) >= 1
	})
	events := eventsFromCapture(cap.snapshot())
	if events[0]["trace_id"] != "4bf92f3577b34da6a3ce929d0e0e4736" {
		t.Fatalf("trace_id = %v", events[0]["trace_id"])
	}
	if events[0]["span_id"] != "00f067aa0ba902b7" {
		t.Fatalf("span_id = %v", events[0]["span_id"])
	}
}

func TestIdentifyFijaDistinctIDParaEventosSiguientes(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint:        srv.URL,
		Token:           "tk",
		Service:         "identify-test",
		FlushInterval:   50 * time.Millisecond,
		OnInternalError: func(error) {},
	})
	if err != nil {
		t.Fatal(err)
	}

	c.Identify("user_42", map[string]any{"email": "a@b.com", "plan": "pro"})
	c.Track("after_login", nil)
	// Close hace flush antes de retornar.
	if err := c.Close(context.Background()); err != nil {
		t.Fatal(err)
	}

	events := eventsFromCapture(cap.snapshot())
	var ident, trk map[string]any
	for _, e := range events {
		switch e["type"] {
		case "identify":
			ident = e
		case "track":
			trk = e
		}
	}
	if ident == nil {
		t.Fatal("debe haber un evento identify")
	}
	if ident["distinct_id"] != "user_42" {
		t.Errorf("identify.distinct_id = %v, want user_42", ident["distinct_id"])
	}
	up := ident["user_properties"].(map[string]any)
	if up["email"] != "a@b.com" || up["plan"] != "pro" {
		t.Errorf("user_properties no match: %+v", up)
	}
	if trk == nil {
		t.Fatal("el track tras identify debe llegar también")
	}
	if trk["distinct_id"] != "user_42" {
		t.Errorf("track tras identify: distinct_id = %v, want user_42", trk["distinct_id"])
	}
}

func TestAliasFusionaSesionPreYPostLogin(t *testing.T) {
	cap := newCaptureServer()
	srv := httptest.NewServer(cap.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint:        srv.URL,
		Token:           "tk",
		Service:         "alias-test",
		FlushInterval:   50 * time.Millisecond,
		OnInternalError: func(error) {},
	})
	if err != nil {
		t.Fatal(err)
	}

	c.Alias("anon_old", "user_99")
	c.Track("post_alias", nil)
	if err := c.Close(context.Background()); err != nil {
		t.Fatal(err)
	}

	events := eventsFromCapture(cap.snapshot())
	var ali, trk map[string]any
	for _, e := range events {
		switch e["type"] {
		case "alias":
			ali = e
		case "track":
			trk = e
		}
	}
	if ali == nil {
		t.Fatal("debe haber un evento alias")
	}
	if ali["anonymous_id"] != "anon_old" {
		t.Errorf("alias.anonymous_id = %v, want anon_old (el PREV id)", ali["anonymous_id"])
	}
	if ali["distinct_id"] != "user_99" {
		t.Errorf("alias.distinct_id = %v, want user_99", ali["distinct_id"])
	}
	if trk["distinct_id"] != "user_99" {
		t.Errorf("post-alias track: distinct_id = %v, want user_99", trk["distinct_id"])
	}
}

// ---- helpers ----

func waitFor(t *testing.T, max time.Duration, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(max)
	for time.Now().Before(deadline) {
		if cond() {
			return
		}
		time.Sleep(20 * time.Millisecond)
	}
}

func equalSlices(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}
