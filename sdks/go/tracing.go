// Tracing del SDK Go — API nativa respaldada por @opentelemetry/sdk.
//
// v0.2.0: la API pública (StartSpan / WithSpan / Span / ContextWithSpan /
// SpanFromContext) se conserva pero por dentro envuelve `go.opentelemetry.io/otel/trace`.
// Esto permite combinar instrumentación manual del SDK con las
// auto-instrumentaciones estándar (otelhttp, otelgrpc, otelsql, otelpgx) en
// un mismo pipeline de export hacia /v1/traces del backend Faro.
//
// Cómo se enchufa con otelhttp:
//
//	import "go.opentelemetry.io/contrib/instrumentation/net/http/otelhttp"
//	client := &http.Client{Transport: otelhttp.NewTransport(http.DefaultTransport)}
//	// → cada request del client genera un span CLIENT auto-emitido por Faro.
//
//	handler = otelhttp.NewHandler(handler, "myapp")
//	// → cada request entrante genera un span SERVER.
package faro

import (
	"context"
	"fmt"
	"runtime/debug"
	"time"

	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/trace"
)

// SpanKind reproduce el enum OTLP. Los valores numéricos coinciden con OTLP
// (1=INTERNAL, 2=SERVER, …) — son los mismos de `go.opentelemetry.io/otel/trace.SpanKind`.
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
	Kind       SpanKind       // default SpanKindInternal
	Attributes map[string]any // se stringifican antes de pasar a OTel
	// Parent fuerza un padre explícito. Si zero, hereda del ctx activo.
	Parent    SpanParent
	StartTime time.Time // default time.Now()
}

// SpanParent permite pasar un padre explícito (otro span, un traceparent, o un
// par trace_id/span_id desde un sistema externo).
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

// Span envuelve `trace.Span` exponiendo la API histórica de Faro (TraceID/SpanID
// como hex string, Traceparent(), AddEvent con map). Thread-safe — toda
// concurrencia la maneja OTel.
type Span struct {
	otel  trace.Span
	ended bool
}

// TraceID devuelve el trace_id en hex (32 chars).
func (s *Span) TraceID() string {
	if s == nil {
		return ""
	}
	return s.otel.SpanContext().TraceID().String()
}

// SpanID devuelve el span_id en hex (16 chars).
func (s *Span) SpanID() string {
	if s == nil {
		return ""
	}
	return s.otel.SpanContext().SpanID().String()
}

// Traceparent devuelve el header W3C traceparent listo para propagar a HTTP
// outbound. flags=01 si el span está sampled, 00 si no.
func (s *Span) Traceparent() string {
	if s == nil {
		return ""
	}
	sc := s.otel.SpanContext()
	flags := "00"
	if sc.IsSampled() {
		flags = "01"
	}
	return fmt.Sprintf("00-%s-%s-%s", sc.TraceID().String(), sc.SpanID().String(), flags)
}

// SetAttribute añade/sobrescribe un atributo. No-op si el span ya cerró.
func (s *Span) SetAttribute(key string, value any) {
	if s == nil || s.ended {
		return
	}
	s.otel.SetAttributes(attribute.String(key, stringify(value)))
}

// SetAttributes añade varios atributos a la vez.
func (s *Span) SetAttributes(attrs map[string]any) {
	if s == nil || s.ended {
		return
	}
	kvs := make([]attribute.KeyValue, 0, len(attrs))
	for k, v := range attrs {
		kvs = append(kvs, attribute.String(k, stringify(v)))
	}
	s.otel.SetAttributes(kvs...)
}

// AddEvent añade un evento al span (timestamped, con attributes opcionales).
func (s *Span) AddEvent(name string, attrs map[string]any) {
	if s == nil || s.ended {
		return
	}
	if len(attrs) == 0 {
		s.otel.AddEvent(name)
		return
	}
	kvs := make([]attribute.KeyValue, 0, len(attrs))
	for k, v := range attrs {
		kvs = append(kvs, attribute.String(k, stringify(v)))
	}
	s.otel.AddEvent(name, trace.WithAttributes(kvs...))
}

// SetStatus marca el status del span. message es opcional (solo se usa para ERROR).
func (s *Span) SetStatus(code SpanStatusCode, message string) {
	if s == nil || s.ended {
		return
	}
	switch code {
	case StatusOK:
		s.otel.SetStatus(codes.Ok, message)
	case StatusError:
		s.otel.SetStatus(codes.Error, message)
	}
}

// RecordException marca el span con status=ERROR y graba el error como evento
// + atributos `exception.*` (paridad con los otros SDKs).
func (s *Span) RecordException(err error) {
	if s == nil || s.ended || err == nil {
		return
	}
	s.otel.SetStatus(codes.Error, err.Error())
	s.otel.SetAttributes(
		attribute.String("exception.type", fmt.Sprintf("%T", err)),
		attribute.String("exception.message", err.Error()),
		attribute.String("exception.stacktrace", string(debug.Stack())),
	)
	// OTel también tiene RecordError; lo llamamos por paridad.
	s.otel.RecordError(err)
}

// End cierra el span y lo encola para export (vía BatchSpanProcessor). Idempotente.
func (s *Span) End() {
	if s == nil || s.ended {
		return
	}
	s.ended = true
	s.otel.End()
}

// Ended devuelve true si End() ya se llamó.
func (s *Span) Ended() bool {
	if s == nil {
		return true
	}
	return s.ended
}

// ---------- Context helpers ----------
//
// La fuente de verdad del span activo es el ContextManager de OTel
// (`trace.SpanFromContext` / `trace.ContextWithSpan`). Nuestros wrappers
// aceptan tanto *Span de Faro como cualquier span OTel (los de auto-instrumentación).

// ContextWithSpan guarda un *Span en el contexto. Internamente delega a
// `trace.ContextWithSpan` para que el ContextManager de OTel también lo vea.
func ContextWithSpan(ctx context.Context, span *Span) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	if span == nil {
		return ctx
	}
	return trace.ContextWithSpan(ctx, span.otel)
}

// SpanFromContext devuelve el span activo en el contexto, o nil.
// Si hay un span OTel auto-instrumentado (otelhttp, etc.), lo devuelve envuelto
// en un *Span de Faro — así código existente que llama a .Traceparent() etc. sigue funcionando.
func SpanFromContext(ctx context.Context) *Span {
	if ctx == nil {
		return nil
	}
	otelSpan := trace.SpanFromContext(ctx)
	if !otelSpan.SpanContext().IsValid() {
		return nil
	}
	return &Span{otel: otelSpan}
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

	// Resolver parent context.
	switch {
	case opts.Parent.ForceRoot:
		// Quitar cualquier span activo del context → OTel creará root.
		ctx = trace.ContextWithSpan(ctx, noopOtelSpan{})
	case opts.Parent.Traceparent != "":
		if tc, ok := TraceContextFromTraceparent(opts.Parent.Traceparent); ok {
			ctx = injectExternalParent(ctx, tc.TraceID, tc.SpanID)
		}
	case opts.Parent.TraceID != "" && opts.Parent.SpanID != "":
		ctx = injectExternalParent(ctx, opts.Parent.TraceID, opts.Parent.SpanID)
	}

	kind := opts.Kind
	if kind == 0 {
		kind = SpanKindInternal
	}

	startOpts := []trace.SpanStartOption{
		trace.WithSpanKind(faroSpanKindToOtel(kind)),
	}
	if !opts.StartTime.IsZero() {
		startOpts = append(startOpts, trace.WithTimestamp(opts.StartTime))
	}
	if len(opts.Attributes) > 0 {
		attrs := make([]attribute.KeyValue, 0, len(opts.Attributes))
		for k, v := range opts.Attributes {
			attrs = append(attrs, attribute.String(k, stringify(v)))
		}
		startOpts = append(startOpts, trace.WithAttributes(attrs...))
	}

	tracer := GetTracer()
	newCtx, otelSpan := tracer.Start(ctx, name, startOpts...)
	return newCtx, &Span{otel: otelSpan}
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

// ---------- Helpers ----------

func faroSpanKindToOtel(k SpanKind) trace.SpanKind {
	switch k {
	case SpanKindServer:
		return trace.SpanKindServer
	case SpanKindClient:
		return trace.SpanKindClient
	case SpanKindProducer:
		return trace.SpanKindProducer
	case SpanKindConsumer:
		return trace.SpanKindConsumer
	default:
		return trace.SpanKindInternal
	}
}

// injectExternalParent crea un SpanContext "remoto" con los IDs dados y lo
// pone en el contexto, para que el próximo `tracer.Start` lo use como padre.
func injectExternalParent(ctx context.Context, traceIDHex, spanIDHex string) context.Context {
	traceID, err := trace.TraceIDFromHex(traceIDHex)
	if err != nil {
		return ctx
	}
	spanID, err := trace.SpanIDFromHex(spanIDHex)
	if err != nil {
		return ctx
	}
	sc := trace.NewSpanContext(trace.SpanContextConfig{
		TraceID:    traceID,
		SpanID:     spanID,
		TraceFlags: trace.FlagsSampled,
		Remote:     true,
	})
	return trace.ContextWithRemoteSpanContext(ctx, sc)
}

// noopOtelSpan se usa para "limpiar" el span activo del contexto cuando
// SpanOptions.Parent.ForceRoot está en true. trace.ContextWithSpan con un span
// invalid logra que `tracer.Start` no encuentre un padre y cree root.
type noopOtelSpan struct{ trace.Span }

func (noopOtelSpan) SpanContext() trace.SpanContext { return trace.SpanContext{} }
func (noopOtelSpan) IsRecording() bool              { return false }
func (noopOtelSpan) End(...trace.SpanEndOption)     {}

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
