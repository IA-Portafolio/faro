// OTLP tracing setup para el SDK de Go.
//
// Inicializa el TracerProvider de OTel con BatchSpanProcessor + un exporter
// OTLP/HTTP/JSON propio (el oficial `otlptracehttp` solo viene en protobuf y
// nuestro backend Faro habla JSON en /v1/traces). Después de Init, cualquier
// código que use `go.opentelemetry.io/otel/trace` o las instrumentaciones
// `otelhttp`/`otelgrpc` exporta automáticamente a Faro.
//
// Diseño:
//   - Singleton: una sola inicialización por proceso; las siguientes son no-op.
//   - El provider se guarda a nivel de paquete para ForceFlush mid-lifetime
//     (necesario para Client.Flush y tests, sin esperar al batch tick).
//   - El exporter custom serializa ReadOnlySpan→OTLP/JSON con `json` stdlib;
//     ~150 líneas vs traer 30MB de stubs protobuf.
package faro

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"sync"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/sdk/resource"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	semconv "go.opentelemetry.io/otel/semconv/v1.26.0"
	"go.opentelemetry.io/otel/trace"
)

const tracerName = "github.com/IA-Portafolio/faro/sdks/go"

// TracingOptions configura la inicialización OTel.
type TracingOptions struct {
	Endpoint           string            // base de Faro, p. ej. https://faro.iaportafolio.com
	Token              string            // bearer token de ingesta del proyecto
	Service            string            // service.name
	TracesEndpoint     string            // override del path completo. Default: ${Endpoint}/v1/traces
	Environment        string            // mapeado a deployment.environment
	Release            string            // mapeado a service.version
	ResourceAttributes map[string]string // atributos extra del Resource
	HTTPClient         *http.Client      // inyectable; si nil, http.DefaultClient
	OnInternalError    func(err error)   // logger para fallos de export
}

var (
	otelProviderMu sync.Mutex
	otelProvider   *sdktrace.TracerProvider
	otelTracer     trace.Tracer
)

// InitTracing inicializa el TracerProvider de OTel apuntando a Faro. Idempotente.
//
// Tras esta llamada, las librerías auto-instrumentadas (otelhttp, otelgrpc, etc.)
// exportan a Faro automáticamente. También se setea el TextMapPropagator a W3C
// tracecontext + baggage para que los headers traceparent se propaguen.
func InitTracing(opts TracingOptions) (bool, error) {
	otelProviderMu.Lock()
	defer otelProviderMu.Unlock()
	if otelProvider != nil {
		return false, nil
	}
	if opts.Endpoint == "" || opts.Token == "" || opts.Service == "" {
		return false, fmt.Errorf("InitTracing: Endpoint, Token y Service son obligatorios")
	}

	base := opts.Endpoint
	for len(base) > 0 && base[len(base)-1] == '/' {
		base = base[:len(base)-1]
	}
	url := opts.TracesEndpoint
	if url == "" {
		url = base + "/v1/traces"
	}

	httpClient := opts.HTTPClient
	if httpClient == nil {
		httpClient = http.DefaultClient
	}
	onErr := opts.OnInternalError
	if onErr == nil {
		onErr = func(error) {}
	}

	exporter := &faroJSONSpanExporter{
		url:        url,
		token:      opts.Token,
		httpClient: httpClient,
		onError:    onErr,
	}

	resAttrs := []attribute.KeyValue{
		semconv.ServiceName(opts.Service),
	}
	if opts.Release != "" {
		resAttrs = append(resAttrs, semconv.ServiceVersion(opts.Release))
	}
	if opts.Environment != "" {
		// OTel 1.26+ usa deployment.environment.name; emitimos también el legacy
		// `deployment.environment` para compat con el indexador de Faro.
		resAttrs = append(resAttrs,
			attribute.String("deployment.environment", opts.Environment),
			attribute.String("deployment.environment.name", opts.Environment),
		)
	}
	for k, v := range opts.ResourceAttributes {
		resAttrs = append(resAttrs, attribute.String(k, v))
	}
	// Schemaless: evita el conflicto de Schema URL entre nuestro semconv (1.26.0) y
	// el que viene en `resource.Default()` (1.41.0+) según la versión del SDK
	// instalada. El trade-off: no traemos los atributos runtime de Default
	// (telemetry.sdk.*, process.runtime.*, service.instance.id) — para Faro no
	// los necesitamos en el lado de ingesta.
	res := resource.NewSchemaless(resAttrs...)

	provider := sdktrace.NewTracerProvider(
		sdktrace.WithResource(res),
		sdktrace.WithBatcher(exporter),
	)
	otel.SetTracerProvider(provider)
	otel.SetTextMapPropagator(propagation.NewCompositeTextMapPropagator(
		propagation.TraceContext{},
		propagation.Baggage{},
	))
	otelProvider = provider
	otelTracer = provider.Tracer(tracerName)
	return true, nil
}

// FlushTracing drena spans pending del BatchSpanProcessor. Lo usa Client.Flush.
func FlushTracing(ctx context.Context) error {
	otelProviderMu.Lock()
	p := otelProvider
	otelProviderMu.Unlock()
	if p == nil {
		return nil
	}
	return p.ForceFlush(ctx)
}

// ShutdownTracing drena spans pending y apaga el provider. Tras esto, una
// nueva InitTracing puede registrar un provider distinto sin que las
// instrumentaciones existentes queden colgadas del viejo.
func ShutdownTracing(ctx context.Context) error {
	otelProviderMu.Lock()
	p := otelProvider
	otelProvider = nil
	otelTracer = nil
	otelProviderMu.Unlock()
	if p == nil {
		return nil
	}
	return p.Shutdown(ctx)
}

// GetTracer devuelve el tracer del SDK. Si OTel no está inicializado, devuelve
// el tracer global (que será no-op).
func GetTracer() trace.Tracer {
	otelProviderMu.Lock()
	defer otelProviderMu.Unlock()
	if otelTracer != nil {
		return otelTracer
	}
	return otel.Tracer(tracerName)
}

// ---------- FaroJsonSpanExporter — OTLP/HTTP/JSON ----------

type faroJSONSpanExporter struct {
	url        string
	token      string
	httpClient *http.Client
	onError    func(error)
}

// ExportSpans implementa sdktrace.SpanExporter. Convierte ReadOnlySpan a
// OTLP/JSON ExportTraceServiceRequest y postea con bearer auth.
func (e *faroJSONSpanExporter) ExportSpans(ctx context.Context, spans []sdktrace.ReadOnlySpan) error {
	if len(spans) == 0 {
		return nil
	}
	payload := buildTracesJSON(spans)
	body, err := json.Marshal(payload)
	if err != nil {
		return fmt.Errorf("faro/tracing: marshal spans: %w", err)
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, e.url, bytes.NewReader(body))
	if err != nil {
		return err
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+e.token)
	resp, err := e.httpClient.Do(req)
	if err != nil {
		return fmt.Errorf("faro/tracing: traces POST: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 500 {
		return fmt.Errorf("faro/tracing: traces HTTP %d", resp.StatusCode)
	}
	if resp.StatusCode >= 400 {
		// 4xx no se reintenta — sería token inválido / batch malformado.
		e.onError(fmt.Errorf("faro/tracing: traces HTTP %d (descartado)", resp.StatusCode))
	}
	return nil
}

// Shutdown libera recursos del exporter.
func (e *faroJSONSpanExporter) Shutdown(_ context.Context) error { return nil }

// ---------- Serialización OTLP/JSON ----------

type jsonValue struct {
	StringValue string   `json:"stringValue,omitempty"`
	IntValue    *string  `json:"intValue,omitempty"` // OTLP/JSON: int64 viaja como string
	BoolValue   *bool    `json:"boolValue,omitempty"`
	DoubleValue *float64 `json:"doubleValue,omitempty"`
}

type jsonKV struct {
	Key   string    `json:"key"`
	Value jsonValue `json:"value"`
}

type jsonStatus struct {
	Code    int    `json:"code"`
	Message string `json:"message,omitempty"`
}

type jsonEvent struct {
	TimeUnixNano string   `json:"timeUnixNano"`
	Name         string   `json:"name"`
	Attributes   []jsonKV `json:"attributes,omitempty"`
}

type jsonSpan struct {
	TraceID           string      `json:"traceId"`
	SpanID            string      `json:"spanId"`
	ParentSpanID      string      `json:"parentSpanId,omitempty"`
	Name              string      `json:"name"`
	Kind              int         `json:"kind"`
	StartTimeUnixNano string      `json:"startTimeUnixNano"`
	EndTimeUnixNano   string      `json:"endTimeUnixNano"`
	Attributes        []jsonKV    `json:"attributes,omitempty"`
	Events            []jsonEvent `json:"events,omitempty"`
	Status            *jsonStatus `json:"status,omitempty"`
}

type jsonScope struct {
	Name    string `json:"name"`
	Version string `json:"version,omitempty"`
}

type jsonResource struct {
	Attributes []jsonKV `json:"attributes"`
}

type jsonScopeSpans struct {
	Scope jsonScope  `json:"scope"`
	Spans []jsonSpan `json:"spans"`
}

type jsonResourceSpans struct {
	Resource   jsonResource     `json:"resource"`
	ScopeSpans []jsonScopeSpans `json:"scopeSpans"`
}

type jsonTracesRequest struct {
	ResourceSpans []jsonResourceSpans `json:"resourceSpans"`
}

func buildTracesJSON(spans []sdktrace.ReadOnlySpan) jsonTracesRequest {
	// Agrupar por (Resource, InstrumentationScope) según la spec OTLP.
	type scopeKey struct{ name, version string }
	type group struct {
		res    *resource.Resource
		scopes map[scopeKey][]sdktrace.ReadOnlySpan
	}
	byRes := map[*resource.Resource]*group{}
	for _, sp := range spans {
		res := sp.Resource()
		g, ok := byRes[res]
		if !ok {
			g = &group{res: res, scopes: map[scopeKey][]sdktrace.ReadOnlySpan{}}
			byRes[res] = g
		}
		sc := sp.InstrumentationScope()
		k := scopeKey{name: sc.Name, version: sc.Version}
		g.scopes[k] = append(g.scopes[k], sp)
	}

	var rs []jsonResourceSpans
	for _, g := range byRes {
		var ss []jsonScopeSpans
		for k, spansInScope := range g.scopes {
			out := make([]jsonSpan, 0, len(spansInScope))
			for _, sp := range spansInScope {
				out = append(out, spanToJSON(sp))
			}
			ss = append(ss, jsonScopeSpans{
				Scope: jsonScope{Name: k.name, Version: k.version},
				Spans: out,
			})
		}
		rs = append(rs, jsonResourceSpans{
			Resource:   jsonResource{Attributes: resourceAttrsToJSON(g.res)},
			ScopeSpans: ss,
		})
	}
	return jsonTracesRequest{ResourceSpans: rs}
}

func spanToJSON(sp sdktrace.ReadOnlySpan) jsonSpan {
	sc := sp.SpanContext()
	parentID := ""
	if p := sp.Parent(); p.IsValid() {
		parentID = p.SpanID().String()
	}
	// OTel Go's SpanKind values: 0=Unspecified, 1=Internal, 2=Server, 3=Client, 4=Producer, 5=Consumer.
	// OTLP wire: 0=UNSPECIFIED, 1=INTERNAL, … — coinciden, sin offset.
	kind := int(sp.SpanKind())
	if kind == 0 {
		kind = int(trace.SpanKindInternal)
	}
	out := jsonSpan{
		TraceID:           sc.TraceID().String(),
		SpanID:            sc.SpanID().String(),
		ParentSpanID:      parentID,
		Name:              sp.Name(),
		Kind:              kind,
		StartTimeUnixNano: fmt.Sprintf("%d", sp.StartTime().UnixNano()),
		EndTimeUnixNano:   fmt.Sprintf("%d", sp.EndTime().UnixNano()),
		Attributes:        kvSliceToJSON(sp.Attributes()),
	}
	if evs := sp.Events(); len(evs) > 0 {
		out.Events = make([]jsonEvent, 0, len(evs))
		for _, ev := range evs {
			out.Events = append(out.Events, jsonEvent{
				TimeUnixNano: fmt.Sprintf("%d", ev.Time.UnixNano()),
				Name:         ev.Name,
				Attributes:   kvSliceToJSON(ev.Attributes),
			})
		}
	}
	status := sp.Status()
	if status.Code != 0 { // 0 = Unset (no Status block en el wire)
		// OTel Go usa Code enum invertido respecto a OTLP wire:
		//   OTel Go: Unset=0, Error=1, Ok=2
		//   OTLP    : UNSET=0, OK=1,    ERROR=2
		// Mapeamos para que los consumers (incluyendo el backend Faro) vean
		// el código wire-correcto.
		otlpCode := 0
		switch status.Code {
		case 1: // OTel Go Error
			otlpCode = 2 // OTLP ERROR
		case 2: // OTel Go Ok
			otlpCode = 1 // OTLP OK
		}
		out.Status = &jsonStatus{
			Code:    otlpCode,
			Message: status.Description,
		}
	}
	return out
}

func resourceAttrsToJSON(res *resource.Resource) []jsonKV {
	if res == nil {
		return nil
	}
	return kvSliceToJSON(res.Attributes())
}

func kvSliceToJSON(attrs []attribute.KeyValue) []jsonKV {
	if len(attrs) == 0 {
		return nil
	}
	out := make([]jsonKV, 0, len(attrs))
	for _, kv := range attrs {
		out = append(out, jsonKV{
			Key:   string(kv.Key),
			Value: wrapAttrValue(kv.Value),
		})
	}
	return out
}

func wrapAttrValue(v attribute.Value) jsonValue {
	switch v.Type() {
	case attribute.BOOL:
		b := v.AsBool()
		return jsonValue{BoolValue: &b}
	case attribute.INT64:
		s := fmt.Sprintf("%d", v.AsInt64())
		return jsonValue{IntValue: &s}
	case attribute.FLOAT64:
		f := v.AsFloat64()
		return jsonValue{DoubleValue: &f}
	case attribute.STRING:
		return jsonValue{StringValue: v.AsString()}
	case attribute.BOOLSLICE, attribute.INT64SLICE, attribute.FLOAT64SLICE, attribute.STRINGSLICE:
		// Slices: encodeamos como string JSON de los valores para mantener el wire simple.
		return jsonValue{StringValue: v.Emit()}
	default:
		return jsonValue{StringValue: v.Emit()}
	}
}
