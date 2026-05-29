// Tracing del SDK Go — API nativa que produce OTLP/JSON contra /v1/traces.
//
// Diseño (paridad con @iaportafolio/node y faro_sdk):
//   - StartSpan(ctx, name, opts) → (newCtx, *Span). El span hijo hereda del padre
//     activo en el contexto. Defer span.End() para encolar.
//   - WithSpan(ctx, name, fn, opts) → corre fn con el span activo, lo cierra
//     automáticamente. Si fn retorna error, lo marca como ERROR + recordException.
//   - GinTracingMiddleware() / HTTPTracingMiddleware(next) crean span SERVER
//     por request, respetan W3C traceparent entrante y lo propagan en la respuesta.
//
// El flush a /v1/traces va por un canal/worker dedicado (separado de los de
// logs y events) para no acoplar latencia entre pipelines.
package faro

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"net/http"
	"runtime/debug"
	"sync"
	"time"
)

// SpanKind reproduce el enum OTLP. Los valores numéricos coinciden con los
// del .proto (INTERNAL=1, SERVER=2, …) para no tener que mapear en `_send_spans`.
type SpanKind int

const (
	SpanKindInternal SpanKind = 1
	SpanKindServer   SpanKind = 2
	SpanKindClient   SpanKind = 3
	SpanKindProducer SpanKind = 4
	SpanKindConsumer SpanKind = 5
)

// SpanStatusCode reproduce el enum OTLP de Status.
type SpanStatusCode int

const (
	StatusUnset SpanStatusCode = 0
	StatusOK    SpanStatusCode = 1
	StatusError SpanStatusCode = 2
)

// SpanOptions parametriza StartSpan / WithSpan.
type SpanOptions struct {
	Kind       SpanKind          // default SpanKindInternal
	Attributes map[string]any    // se stringifican antes de enviarse
	// Parent: si != "" o no-cero, fuerza ese padre. Si Parent == "explicit-root",
	// es root forzado. Si nil/zero → hereda del context activo.
	Parent     SpanParent
	StartTime  time.Time // default time.Now()
}

// SpanParent permite pasar un padre explícito (otro span, un traceparent, o un
// pair trace_id/span_id desde un sistema externo).
type SpanParent struct {
	TraceID     string
	SpanID      string
	Traceparent string
	// ForceRoot: true → ignora cualquier contexto activo y crea un nuevo trace.
	ForceRoot bool
}

func (p SpanParent) isZero() bool {
	return p.TraceID == "" && p.SpanID == "" && p.Traceparent == "" && !p.ForceRoot
}

type spanEvent struct {
	Name       string
	TimeMs     int64
	Attributes map[string]string
}

// Span representa un span en construcción. Hilo-seguro para los métodos públicos.
type Span struct {
	mu             sync.Mutex
	client         *Client
	traceID        string
	spanID         string
	parentSpanID   string
	name           string
	kind           SpanKind
	startTimeMs    int64
	endTimeMs      int64
	attributes     map[string]string
	events         []spanEvent
	statusCode     SpanStatusCode
	statusMessage  string
	ended          bool
}

// TraceID devuelve el trace_id en hex (32 chars).
func (s *Span) TraceID() string { return s.traceID }

// SpanID devuelve el span_id en hex (16 chars).
func (s *Span) SpanID() string { return s.spanID }

// Traceparent devuelve el header W3C traceparent listo para propagar a HTTP
// outbound. flags=01 (sampled) — el sampling se hace upstream del exporter.
func (s *Span) Traceparent() string {
	return fmt.Sprintf("00-%s-%s-01", s.traceID, s.spanID)
}

// SetAttribute añade/sobrescribe un atributo. No-op si el span ya cerró.
func (s *Span) SetAttribute(key string, value any) {
	if s == nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.ended {
		return
	}
	s.attributes[key] = stringify(value)
}

// SetAttributes mezcla un mapa de atributos. No-op si el span ya cerró.
func (s *Span) SetAttributes(attrs map[string]any) {
	if s == nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.ended {
		return
	}
	for k, v := range attrs {
		s.attributes[k] = stringify(v)
	}
}

// AddEvent añade un span event con timestamp (default time.Now()).
func (s *Span) AddEvent(name string, attributes map[string]any) {
	if s == nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.ended {
		return
	}
	attrs := make(map[string]string, len(attributes))
	for k, v := range attributes {
		attrs[k] = stringify(v)
	}
	s.events = append(s.events, spanEvent{
		Name:       name,
		TimeMs:     time.Now().UnixMilli(),
		Attributes: attrs,
	})
}

// SetStatus cambia el código de status y opcionalmente el mensaje.
func (s *Span) SetStatus(code SpanStatusCode, message string) {
	if s == nil {
		return
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.ended {
		return
	}
	s.statusCode = code
	if message != "" {
		s.statusMessage = message
	}
}

// RecordException marca el span como ERROR y añade los attrs exception.*.
// Útil para reportar errores sin re-lanzarlos.
func (s *Span) RecordException(err error) {
	if s == nil || err == nil {
		return
	}
	s.SetStatus(StatusError, err.Error())
	s.SetAttribute("exception.type", typeName(err))
	s.SetAttribute("exception.message", err.Error())
	s.SetAttribute("exception.stacktrace", string(debug.Stack()))
}

// End cierra el span y lo encola para envío. Idempotente.
func (s *Span) End() {
	if s == nil {
		return
	}
	s.mu.Lock()
	if s.ended {
		s.mu.Unlock()
		return
	}
	s.ended = true
	s.endTimeMs = time.Now().UnixMilli()
	s.mu.Unlock()
	s.client.enqueueSpan(s)
}

// Ended devuelve true si End() ya se llamó.
func (s *Span) Ended() bool {
	if s == nil {
		return true
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.ended
}

// ---------- Context helpers ----------

type spanContextKey struct{}

// ContextWithSpan guarda un *Span en el contexto para que `SpanFromContext` y
// `StartSpan` (en su llamada hija) puedan recuperarlo como padre.
func ContextWithSpan(ctx context.Context, span *Span) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	return context.WithValue(ctx, spanContextKey{}, span)
}

// SpanFromContext devuelve el span activo en el contexto, o nil.
func SpanFromContext(ctx context.Context) *Span {
	if ctx == nil {
		return nil
	}
	s, _ := ctx.Value(spanContextKey{}).(*Span)
	return s
}

// ---------- Client tracing methods ----------

// StartSpan crea un span hijo del activo en ctx (o root si no hay) y devuelve
// el nuevo context con el span activado. El caller debe llamar a `span.End()`.
func (c *Client) StartSpan(ctx context.Context, name string, opts SpanOptions) (context.Context, *Span) {
	if c == nil {
		return ctx, nil
	}
	if ctx == nil {
		ctx = context.Background()
	}

	var traceID, parentSpanID string
	switch {
	case opts.Parent.ForceRoot:
		traceID = newTraceID()
	case opts.Parent.Traceparent != "":
		if tc, ok := TraceContextFromTraceparent(opts.Parent.Traceparent); ok {
			traceID = tc.TraceID
			parentSpanID = tc.SpanID
		} else {
			traceID = newTraceID()
		}
	case opts.Parent.TraceID != "" && opts.Parent.SpanID != "":
		tc := normalizeTraceContext(TraceContext{TraceID: opts.Parent.TraceID, SpanID: opts.Parent.SpanID})
		if tc.TraceID != "" {
			traceID = tc.TraceID
			parentSpanID = tc.SpanID
		} else {
			traceID = newTraceID()
		}
	default:
		// Hereda del span activo en ctx, si hay.
		if active := SpanFromContext(ctx); active != nil {
			traceID = active.traceID
			parentSpanID = active.spanID
		} else {
			traceID = newTraceID()
		}
	}

	kind := opts.Kind
	if kind == 0 {
		kind = SpanKindInternal
	}
	startMs := opts.StartTime.UnixMilli()
	if opts.StartTime.IsZero() {
		startMs = time.Now().UnixMilli()
	}
	attrs := make(map[string]string, len(opts.Attributes))
	for k, v := range opts.Attributes {
		attrs[k] = stringify(v)
	}
	span := &Span{
		client:       c,
		traceID:      traceID,
		spanID:       newSpanID(),
		parentSpanID: parentSpanID,
		name:         name,
		kind:         kind,
		startTimeMs:  startMs,
		attributes:   attrs,
	}
	return ContextWithSpan(ctx, span), span
}

// WithSpan ejecuta fn con un span activo. Si fn retorna error, lo registra
// con RecordException y marca status=ERROR. Cierra el span siempre.
func (c *Client) WithSpan(ctx context.Context, name string, fn func(ctx context.Context, span *Span) error, opts SpanOptions) error {
	ctx, span := c.StartSpan(ctx, name, opts)
	defer span.End()
	if err := fn(ctx, span); err != nil {
		span.RecordException(err)
		return err
	}
	return nil
}

// enqueueSpan agrega un span cerrado a la cola. No bloquea: descarta si llena.
func (c *Client) enqueueSpan(span *Span) {
	if c == nil || span == nil {
		return
	}
	select {
	case c.spansCh <- span:
	default:
		c.opts.OnInternalError(fmt.Errorf("cola de spans llena, descartado"))
	}
}

// ---------- Internal: spans worker + OTLP/JSON ----------

func (c *Client) spansLoop() {
	defer c.wg.Done()
	ticker := time.NewTicker(c.opts.FlushInterval)
	defer ticker.Stop()

	batch := make([]*Span, 0, c.opts.MaxBatchSize)
	flush := func() {
		if len(batch) == 0 {
			return
		}
		ok := c.sendSpans(batch)
		if !ok {
			for _, s := range batch {
				select {
				case c.spansCh <- s:
				default:
					c.opts.OnInternalError(fmt.Errorf("cola de spans llena al reintentar, descartado"))
				}
			}
		}
		batch = batch[:0]
	}

	for {
		select {
		case <-c.closed:
			for {
				select {
				case s := <-c.spansCh:
					batch = append(batch, s)
					if len(batch) >= c.opts.MaxBatchSize {
						flush()
					}
				default:
					flush()
					return
				}
			}
		case s := <-c.spansCh:
			batch = append(batch, s)
			if len(batch) >= c.opts.MaxBatchSize {
				flush()
			}
		case <-ticker.C:
			flush()
		}
	}
}

// otlpKV es la representación JSON del KeyValue de OTLP — sólo stringValue.
type otlpKV struct {
	Key   string      `json:"key"`
	Value otlpAnyVal  `json:"value"`
}

type otlpAnyVal struct {
	StringValue string `json:"stringValue"`
}

func otlpAttrs(m map[string]string) []otlpKV {
	out := make([]otlpKV, 0, len(m))
	for k, v := range m {
		out = append(out, otlpKV{Key: k, Value: otlpAnyVal{StringValue: v}})
	}
	return out
}

type otlpStatus struct {
	Code    int    `json:"code"`
	Message string `json:"message,omitempty"`
}

type otlpEvent struct {
	TimeUnixNano string   `json:"timeUnixNano"`
	Name         string   `json:"name"`
	Attributes   []otlpKV `json:"attributes,omitempty"`
}

type otlpSpan struct {
	TraceID            string      `json:"traceId"`
	SpanID             string      `json:"spanId"`
	ParentSpanID       string      `json:"parentSpanId,omitempty"`
	Name               string      `json:"name"`
	Kind               int         `json:"kind"`
	StartTimeUnixNano  string      `json:"startTimeUnixNano"`
	EndTimeUnixNano    string      `json:"endTimeUnixNano"`
	Attributes         []otlpKV    `json:"attributes,omitempty"`
	Events             []otlpEvent `json:"events,omitempty"`
	Status             *otlpStatus `json:"status,omitempty"`
}

type otlpScope struct {
	Name string `json:"name"`
}

type otlpResource struct {
	Attributes []otlpKV `json:"attributes"`
}

type otlpScopeSpans struct {
	Scope otlpScope  `json:"scope"`
	Spans []otlpSpan `json:"spans"`
}

type otlpResourceSpans struct {
	Resource   otlpResource     `json:"resource"`
	ScopeSpans []otlpScopeSpans `json:"scopeSpans"`
}

type otlpTracesRequest struct {
	ResourceSpans []otlpResourceSpans `json:"resourceSpans"`
}

func (c *Client) buildSpansPayload(batch []*Span) otlpTracesRequest {
	resourceAttrs := []otlpKV{
		{Key: "service.name", Value: otlpAnyVal{StringValue: c.opts.Service}},
	}
	if c.opts.Environment != "" {
		resourceAttrs = append(resourceAttrs, otlpKV{
			Key: "deployment.environment", Value: otlpAnyVal{StringValue: c.opts.Environment},
		})
	}
	if c.opts.Release != "" {
		resourceAttrs = append(resourceAttrs, otlpKV{
			Key: "service.version", Value: otlpAnyVal{StringValue: c.opts.Release},
		})
	}
	for k, v := range c.opts.Attributes {
		resourceAttrs = append(resourceAttrs, otlpKV{
			Key: k, Value: otlpAnyVal{StringValue: v},
		})
	}

	spans := make([]otlpSpan, 0, len(batch))
	for _, s := range batch {
		s.mu.Lock()
		os := otlpSpan{
			TraceID:           s.traceID,
			SpanID:            s.spanID,
			ParentSpanID:      s.parentSpanID,
			Name:              s.name,
			Kind:              int(s.kind),
			StartTimeUnixNano: fmt.Sprintf("%d", s.startTimeMs*1_000_000),
			EndTimeUnixNano:   fmt.Sprintf("%d", s.endTimeMs*1_000_000),
			Attributes:        otlpAttrs(s.attributes),
		}
		if len(s.events) > 0 {
			os.Events = make([]otlpEvent, len(s.events))
			for i, e := range s.events {
				os.Events[i] = otlpEvent{
					TimeUnixNano: fmt.Sprintf("%d", e.TimeMs*1_000_000),
					Name:         e.Name,
					Attributes:   otlpAttrs(e.Attributes),
				}
			}
		}
		if s.statusCode != StatusUnset {
			os.Status = &otlpStatus{
				Code:    int(s.statusCode),
				Message: s.statusMessage,
			}
		}
		s.mu.Unlock()
		spans = append(spans, os)
	}
	return otlpTracesRequest{
		ResourceSpans: []otlpResourceSpans{
			{
				Resource: otlpResource{Attributes: resourceAttrs},
				ScopeSpans: []otlpScopeSpans{
					{Scope: otlpScope{Name: "github.com/IA-Portafolio/faro/sdks/go"}, Spans: spans},
				},
			},
		},
	}
}

func (c *Client) sendSpans(batch []*Span) bool {
	body, err := json.Marshal(c.buildSpansPayload(batch))
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("marshal spans: %w", err))
		return true
	}
	req, err := http.NewRequest(http.MethodPost, c.opts.Endpoint+"/v1/traces", bytes.NewReader(body))
	if err != nil {
		c.opts.OnInternalError(err)
		return true
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.opts.Token)
	resp, err := c.opts.HTTPClient.Do(req)
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("ingest traces: %w", err))
		return false
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		c.opts.OnInternalError(fmt.Errorf("ingest traces HTTP %d", resp.StatusCode))
		return resp.StatusCode < 500
	}
	return true
}

// ---------- ID generation ----------

func newTraceID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

func newSpanID() string {
	var b [8]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

// ---------- Package-level helpers ----------

// StartSpan es el helper a nivel de paquete equivalente a defaultClient.StartSpan.
func StartSpan(ctx context.Context, name string, opts SpanOptions) (context.Context, *Span) {
	if defaultClient == nil {
		return ctx, nil
	}
	return defaultClient.StartSpan(ctx, name, opts)
}

// WithSpan es el helper a nivel de paquete equivalente a defaultClient.WithSpan.
func WithSpan(ctx context.Context, name string, fn func(ctx context.Context, span *Span) error, opts SpanOptions) error {
	if defaultClient == nil {
		return fn(ctx, nil)
	}
	return defaultClient.WithSpan(ctx, name, fn, opts)
}

// ActiveSpan devuelve el span activo en ctx (alias semántico de SpanFromContext).
func ActiveSpan(ctx context.Context) *Span { return SpanFromContext(ctx) }
