package faro

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"
	"time"
)

// traceCaptureServer monta un test server que separa /v1/traces y /api/v1/ingest/logs.
// Diferente del captureServer existente en faro_test.go que solo maneja un endpoint.
type traceCaptureServer struct {
	mu     sync.Mutex
	traces []map[string]any
	logs   []map[string]any
	srv    *httptest.Server
}

func newTraceCaptureServer() *traceCaptureServer {
	c := &traceCaptureServer{}
	c.srv = httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		body, _ := io.ReadAll(r.Body)
		var data map[string]any
		_ = json.Unmarshal(body, &data)
		c.mu.Lock()
		switch r.URL.Path {
		case "/v1/traces":
			c.traces = append(c.traces, data)
		case "/api/v1/ingest/logs":
			c.logs = append(c.logs, data)
		}
		c.mu.Unlock()
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"ok":true}`))
	}))
	return c
}

func (c *traceCaptureServer) Close() { c.srv.Close() }

func (c *traceCaptureServer) waitForLocal(predicate func() bool, timeout time.Duration) bool {
	// El predicate suele llamar a allSpans()/etc que toman c.mu — NO lockear aquí
	// o creamos un deadlock (sync.Mutex no es reentrante en Go).
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) {
		if predicate() {
			return true
		}
		time.Sleep(20 * time.Millisecond)
	}
	return false
}

func (c *traceCaptureServer) allSpans() []map[string]any {
	c.mu.Lock()
	defer c.mu.Unlock()
	var out []map[string]any
	for _, req := range c.traces {
		rs, _ := req["resourceSpans"].([]any)
		for _, rsv := range rs {
			rsmap, _ := rsv.(map[string]any)
			ss, _ := rsmap["scopeSpans"].([]any)
			for _, ssv := range ss {
				ssmap, _ := ssv.(map[string]any)
				spans, _ := ssmap["spans"].([]any)
				for _, sp := range spans {
					if m, ok := sp.(map[string]any); ok {
						out = append(out, m)
					}
				}
			}
		}
	}
	return out
}

func (c *traceCaptureServer) firstResourceAttrs() []any {
	c.mu.Lock()
	defer c.mu.Unlock()
	if len(c.traces) == 0 {
		return nil
	}
	rs, _ := c.traces[0]["resourceSpans"].([]any)
	if len(rs) == 0 {
		return nil
	}
	rsmap, _ := rs[0].(map[string]any)
	res, _ := rsmap["resource"].(map[string]any)
	attrs, _ := res["attributes"].([]any)
	return attrs
}

func getAttr(attrs []any, key string) string {
	for _, a := range attrs {
		m, ok := a.(map[string]any)
		if !ok {
			continue
		}
		if m["key"] != key {
			continue
		}
		v, _ := m["value"].(map[string]any)
		if s, ok := v["stringValue"].(string); ok {
			return s
		}
	}
	return ""
}

func newTestClient(t *testing.T, endpoint string) *Client {
	t.Helper()
	c, err := New(Options{
		Endpoint:      endpoint,
		Token:         "tk",
		Service:       "test",
		Environment:   "prod",
		Release:       "1.0.0",
		FlushInterval: 50 * time.Millisecond,
	})
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	return c
}

func TestStartSpan_EmitsOTLPJSON(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)

	_, span := c.StartSpan(context.Background(), "checkout", SpanOptions{
		Kind:       SpanKindServer,
		Attributes: map[string]any{"http.method": "POST"},
	})
	span.SetAttribute("user.id", 42)
	span.AddEvent("cache.miss", map[string]any{"key": "abc"})
	span.SetStatus(StatusOK, "")
	span.End()

	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool { return len(srv.allSpans()) >= 1 }, 2*time.Second) {
		t.Fatal("span no llegó")
	}

	resAttrs := srv.firstResourceAttrs()
	if getAttr(resAttrs, "service.name") != "test" {
		t.Errorf("service.name = %q", getAttr(resAttrs, "service.name"))
	}
	if getAttr(resAttrs, "deployment.environment") != "prod" {
		t.Errorf("environment missing")
	}

	sp := srv.allSpans()[0]
	if sp["name"] != "checkout" {
		t.Errorf("name = %v", sp["name"])
	}
	if int(sp["kind"].(float64)) != int(SpanKindServer) {
		t.Errorf("kind = %v", sp["kind"])
	}
	tid := sp["traceId"].(string)
	if len(tid) != 32 {
		t.Errorf("traceId len = %d", len(tid))
	}
	sid := sp["spanId"].(string)
	if len(sid) != 16 {
		t.Errorf("spanId len = %d", len(sid))
	}
	if _, ok := sp["parentSpanId"]; ok {
		// el campo NO debe estar (omitempty); si está, no debe ser non-empty
		if s := sp["parentSpanId"].(string); s != "" {
			t.Errorf("root span con parentSpanId=%q", s)
		}
	}
	status, _ := sp["status"].(map[string]any)
	if int(status["code"].(float64)) != int(StatusOK) {
		t.Errorf("status.code = %v", status["code"])
	}
	spanAttrs, _ := sp["attributes"].([]any)
	if getAttr(spanAttrs, "user.id") != "42" {
		t.Errorf("user.id attr missing")
	}
	events, _ := sp["events"].([]any)
	if len(events) == 0 || events[0].(map[string]any)["name"] != "cache.miss" {
		t.Errorf("event cache.miss missing")
	}

	_ = c.Close(context.Background())
}

func TestStartSpan_ContextParent(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)

	ctx, parent := c.StartSpan(context.Background(), "parent", SpanOptions{})
	_, child := c.StartSpan(ctx, "child", SpanOptions{})
	child.End()
	parent.End()
	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool { return len(srv.allSpans()) >= 2 }, 2*time.Second) {
		t.Fatal("spans no llegaron")
	}

	var p, ch map[string]any
	for _, sp := range srv.allSpans() {
		switch sp["name"] {
		case "parent":
			p = sp
		case "child":
			ch = sp
		}
	}
	if ch["traceId"] != p["traceId"] {
		t.Errorf("child no hereda trace_id")
	}
	if ch["parentSpanId"] != p["spanId"] {
		t.Errorf("child.parentSpanId = %v, want %v", ch["parentSpanId"], p["spanId"])
	}

	_ = c.Close(context.Background())
}

func TestWithSpan_RecordsExceptionOnError(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)

	err := c.WithSpan(context.Background(), "boom", func(ctx context.Context, span *Span) error {
		return fmt.Errorf("kaboom")
	}, SpanOptions{})
	if err == nil || !strings.Contains(err.Error(), "kaboom") {
		t.Fatalf("WithSpan no propagó el error: %v", err)
	}
	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool { return len(srv.allSpans()) >= 1 }, 2*time.Second) {
		t.Fatal("span no llegó")
	}

	sp := srv.allSpans()[0]
	status := sp["status"].(map[string]any)
	if int(status["code"].(float64)) != int(StatusError) {
		t.Errorf("status no es ERROR: %v", status)
	}
	attrs := sp["attributes"].([]any)
	if getAttr(attrs, "exception.message") != "kaboom" {
		t.Errorf("exception.message missing")
	}

	_ = c.Close(context.Background())
}

func TestStartSpan_ForceRoot(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)

	ctx, parent := c.StartSpan(context.Background(), "outer", SpanOptions{})
	_, child := c.StartSpan(ctx, "forced-root", SpanOptions{Parent: SpanParent{ForceRoot: true}})
	child.End()
	parent.End()
	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool { return len(srv.allSpans()) >= 2 }, 2*time.Second) {
		t.Fatal("spans no llegaron")
	}
	var p, ch map[string]any
	for _, sp := range srv.allSpans() {
		switch sp["name"] {
		case "outer":
			p = sp
		case "forced-root":
			ch = sp
		}
	}
	if ch["traceId"] == p["traceId"] {
		t.Errorf("ForceRoot debería romper la herencia de trace_id")
	}

	_ = c.Close(context.Background())
}

func TestStartSpan_TraceparentParent(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)

	tp := "00-" + strings.Repeat("a", 32) + "-" + strings.Repeat("b", 16) + "-01"
	_, span := c.StartSpan(context.Background(), "from-tp", SpanOptions{
		Parent: SpanParent{Traceparent: tp},
	})
	span.End()
	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool { return len(srv.allSpans()) >= 1 }, 2*time.Second) {
		t.Fatal("span no llegó")
	}

	sp := srv.allSpans()[0]
	if sp["traceId"] != strings.Repeat("a", 32) {
		t.Errorf("traceId no del traceparent: %v", sp["traceId"])
	}
	if sp["parentSpanId"] != strings.Repeat("b", 16) {
		t.Errorf("parentSpanId no del traceparent: %v", sp["parentSpanId"])
	}

	_ = c.Close(context.Background())
}

func TestLogContext_AutoCorrelates(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)

	ctx, span := c.StartSpan(context.Background(), "handler", SpanOptions{})
	c.InfoContext(ctx, "procesando", map[string]any{"foo": "bar"})
	span.End()
	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool {
		srv.mu.Lock()
		defer srv.mu.Unlock()
		return len(srv.logs) >= 1
	}, 2*time.Second) {
		t.Fatal("log no llegó")
	}

	srv.mu.Lock()
	logBatch := srv.logs[0]
	srv.mu.Unlock()
	logs := logBatch["logs"].([]any)
	log0 := logs[0].(map[string]any)
	if log0["trace_id"] != span.TraceID() {
		t.Errorf("log.trace_id = %v, want %v", log0["trace_id"], span.TraceID())
	}
	if log0["span_id"] != span.SpanID() {
		t.Errorf("log.span_id = %v, want %v", log0["span_id"], span.SpanID())
	}

	_ = c.Close(context.Background())
}

func TestSpan_TraceparentFormat(t *testing.T) {
	c, _ := New(Options{
		Endpoint: "http://127.0.0.1:1", Token: "t", Service: "s",
		FlushInterval: time.Hour,
	})
	defer c.Close(context.Background())
	_, span := c.StartSpan(context.Background(), "x", SpanOptions{})
	tp := span.Traceparent()
	if !strings.HasPrefix(tp, "00-") || !strings.HasSuffix(tp, "-01") {
		t.Errorf("traceparent format inválido: %s", tp)
	}
	parts := strings.Split(tp, "-")
	if len(parts) != 4 || len(parts[1]) != 32 || len(parts[2]) != 16 {
		t.Errorf("traceparent parts inválidos: %v", parts)
	}
}

func TestSpan_RecordException(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)

	_, span := c.StartSpan(context.Background(), "op", SpanOptions{})
	span.RecordException(fmt.Errorf("bad input"))
	span.End()
	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool { return len(srv.allSpans()) >= 1 }, 2*time.Second) {
		t.Fatal("span no llegó")
	}
	sp := srv.allSpans()[0]
	status := sp["status"].(map[string]any)
	if int(status["code"].(float64)) != int(StatusError) {
		t.Errorf("status no es ERROR")
	}
	attrs := sp["attributes"].([]any)
	if getAttr(attrs, "exception.message") != "bad input" {
		t.Errorf("exception.message missing")
	}

	_ = c.Close(context.Background())
}

func TestSpan_End_Idempotent(t *testing.T) {
	srv := newTraceCaptureServer()
	defer srv.Close()
	c := newTestClient(t, srv.srv.URL)
	_, span := c.StartSpan(context.Background(), "once", SpanOptions{})
	span.End()
	span.End() // segunda llamada NO debe encolar otro
	span.End()
	_ = c.Flush(2 * time.Second)
	if !srv.waitForLocal(func() bool { return len(srv.allSpans()) >= 1 }, 1*time.Second) {
		t.Fatal("primer End debería haber encolado")
	}
	time.Sleep(200 * time.Millisecond)
	if n := len(srv.allSpans()); n != 1 {
		t.Errorf("End no es idempotente: %d spans", n)
	}

	_ = c.Close(context.Background())
}
