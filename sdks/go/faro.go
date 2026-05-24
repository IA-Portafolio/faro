// Package faro es el SDK de Go para Faro (https://github.com/iaportafolio/faro).
package faro

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"regexp"
	"runtime/debug"
	"strings"
	"sync"
	"time"
)

type Severity string

const (
	SevTrace Severity = "TRACE"
	SevDebug Severity = "DEBUG"
	SevInfo  Severity = "INFO"
	SevWarn  Severity = "WARN"
	SevError Severity = "ERROR"
	SevFatal Severity = "FATAL"
)

// Options configura el SDK.
type Options struct {
	Endpoint        string            // https://faro.iaportafolio.com
	Token           string            // token de ingesta del proyecto
	Service         string            // service.name
	Environment     string            // deployment.environment
	Release         string            // service.version
	Attributes      map[string]string // atributos por defecto
	FlushInterval   time.Duration     // por defecto 750ms
	MaxBatchSize    int               // por defecto 200
	MaxQueueSize    int               // por defecto 10_000
	HTTPTimeout     time.Duration     // por defecto 5s
	HTTPClient      *http.Client      // inyectable
	OnInternalError func(err error)   // se llama ante fallos en segundo plano; por defecto va a stderr

	// Scrubbing + beforeSend (ver sdks/README.md → Privacidad / hooks).
	// ScrubFields: claves cuyo valor se reemplaza por "[REDACTED]" (match case-insensitive por substring).
	// Si nil → DefaultScrubFields. Pasar []string{} explícito para desactivar (no recomendado).
	ScrubFields []string
	// DisableHeaderScrub desactiva el añadido automático de "authorization","cookie","set-cookie"
	// a ScrubFields. Por defecto (false) los headers se redactan — defaults seguros.
	DisableHeaderScrub bool
	// ScrubPatterns: presets aplicados a values string y a Message. Valores válidos: "email","jwt","credit-card","api-key".
	// Si nil → ["jwt","api-key"]. Pasar []string{} explícito para desactivar todos.
	ScrubPatterns []string
	// BeforeSend se llama post-scrub con la Entry exacta que se enviará. Devolver nil descarta el evento.
	BeforeSend func(entry *Entry) *Entry
	// TraceContext permite extraer trace_id/span_id desde un context.Context.
	// Útil para integraciones OpenTelemetry sin arrastrar la dependencia al SDK:
	//   TraceContext: func(ctx context.Context) faro.TraceContext {
	//     sc := trace.SpanContextFromContext(ctx)
	//     return faro.TraceContext{TraceID: sc.TraceID().String(), SpanID: sc.SpanID().String()}
	//   }
	TraceContext func(ctx context.Context) TraceContext
}

// DefaultScrubFields son las claves redactadas por defecto.
var DefaultScrubFields = []string{
	"password", "token", "secret", "authorization", "cookie", "set-cookie", "api_key", "apikey",
}

var headerScrubFields = []string{"authorization", "cookie", "set-cookie"}

const redacted = "[REDACTED]"

var scrubRegexes = map[string]*regexp.Regexp{
	"email":       regexp.MustCompile(`[\w.+-]+@[\w-]+(?:\.[\w-]+)+`),
	"jwt":         regexp.MustCompile(`\beyJ[\w-]+\.[\w-]+\.[\w-]+\b`),
	"credit-card": regexp.MustCompile(`\b(?:\d[ -]?){13,19}\b`),
	"api-key":     regexp.MustCompile(`\b(?:sk-|ghp_|ghs_|gho_|github_pat_|xoxb-|xoxp-|xoxs-|AKIA|ASIA|AIza)[\w-]{12,}\b`),
}

// Entry es un evento de log único, listo para enviarse por la red.
type Entry struct {
	Level      Severity          `json:"level"`
	Message    string            `json:"message"`
	Timestamp  string            `json:"timestamp,omitempty"`
	TraceID    string            `json:"trace_id,omitempty"`
	SpanID     string            `json:"span_id,omitempty"`
	Attributes map[string]string `json:"attributes,omitempty"`
}

// ProductEvent es un evento de producto al estilo Segment/PostHog. Se envía a
// `POST /api/v1/ingest/events` y persiste en `faro.product_events`. El campo
// `Type` discrimina la semántica: `track` (custom), `identify`, `alias`. Los
// SDKs mobile/web añaden `page`/`screen` — el SDK de Go (server-side) solo
// expone los tres relevantes para backend.
type ProductEvent struct {
	Type           string         `json:"type"`
	Name           string         `json:"name"`
	Timestamp      string         `json:"timestamp"`
	DistinctID     string         `json:"distinct_id"`
	AnonymousID    string         `json:"anonymous_id"`
	SessionID      string         `json:"session_id"`
	Properties     map[string]any `json:"properties"`
	UserProperties map[string]any `json:"user_properties"`
	Context        map[string]any `json:"context"`
	Source         string         `json:"source"`
	TraceID        string         `json:"trace_id,omitempty"`
	SpanID         string         `json:"span_id,omitempty"`
}

// TraceContext contiene los IDs de correlación W3C tracecontext.
type TraceContext struct {
	TraceID string
	SpanID  string
}

type traceContextKey struct{}

var traceparentRE = regexp.MustCompile(`^[\da-fA-F]{2}-([\da-fA-F]{32})-([\da-fA-F]{16})-[\da-fA-F]{2}(?:-.+)?$`)

// WithTraceContext guarda trace_id/span_id en un context.Context para que
// TrackContext pueda adjuntarlos automáticamente al evento.
func WithTraceContext(ctx context.Context, trace TraceContext) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	return context.WithValue(ctx, traceContextKey{}, normalizeTraceContext(trace))
}

// WithTraceparent parsea un header W3C traceparent y lo guarda en el contexto.
func WithTraceparent(ctx context.Context, traceparent string) context.Context {
	if ctx == nil {
		ctx = context.Background()
	}
	trace, ok := TraceContextFromTraceparent(traceparent)
	if !ok {
		return ctx
	}
	return WithTraceContext(ctx, trace)
}

// TraceContextFromTraceparent parsea el header W3C traceparent.
func TraceContextFromTraceparent(traceparent string) (TraceContext, bool) {
	match := traceparentRE.FindStringSubmatch(strings.TrimSpace(traceparent))
	if match == nil {
		return TraceContext{}, false
	}
	trace := normalizeTraceContext(TraceContext{
		TraceID: match[1],
		SpanID:  match[2],
	})
	if trace.TraceID == "" || trace.SpanID == "" {
		return TraceContext{}, false
	}
	return trace, true
}

type Client struct {
	opts         Options
	ch           chan Entry
	eventsCh     chan ProductEvent
	wg           sync.WaitGroup
	closed       chan struct{}
	once         sync.Once
	scrubNeedles []string // ya en lowercase
	scrubRegexes []*regexp.Regexp

	// Estado de identidad para product events. Lo protegemos con un mutex
	// porque `Track`/`Identify`/`Alias` se pueden llamar desde goroutines
	// concurrentes (típico en backends con request handlers paralelos).
	identityMu     sync.Mutex
	distinctID     string
	anonymousID    string
	userProperties map[string]any
}

// New arranca un cliente de Faro y lo devuelve. También lanza un flusher en segundo plano.
func New(opts Options) (*Client, error) {
	if opts.Endpoint == "" {
		opts.Endpoint = os.Getenv("FARO_ENDPOINT")
	}
	if opts.Token == "" {
		opts.Token = os.Getenv("FARO_TOKEN")
	}
	if opts.Endpoint == "" || opts.Token == "" {
		return nil, fmt.Errorf("faro: Endpoint y Token son obligatorios")
	}
	// Perfil de defaults: "server" (sdks/README.md → Perfiles de defaults).
	if opts.FlushInterval == 0 {
		opts.FlushInterval = 750 * time.Millisecond
	}
	if opts.MaxBatchSize == 0 {
		opts.MaxBatchSize = 200
	}
	if opts.MaxQueueSize == 0 {
		opts.MaxQueueSize = 10_000
	}
	if opts.HTTPTimeout == 0 {
		opts.HTTPTimeout = 5 * time.Second
	}
	if opts.HTTPClient == nil {
		opts.HTTPClient = &http.Client{Timeout: opts.HTTPTimeout}
	}
	if opts.OnInternalError == nil {
		opts.OnInternalError = func(err error) {
			fmt.Fprintf(os.Stderr, "[faro] %v\n", err)
		}
	}
	opts.Endpoint = strings.TrimRight(opts.Endpoint, "/")

	// Scrubbing: construye needles y regex efectivos a partir de los presets.
	if opts.ScrubFields == nil {
		opts.ScrubFields = DefaultScrubFields
	}
	if opts.ScrubPatterns == nil {
		opts.ScrubPatterns = []string{"jwt", "api-key"}
	}
	needleSet := make(map[string]struct{}, len(opts.ScrubFields)+3)
	for _, f := range opts.ScrubFields {
		needleSet[strings.ToLower(f)] = struct{}{}
	}
	if !opts.DisableHeaderScrub {
		for _, f := range headerScrubFields {
			needleSet[f] = struct{}{}
		}
	}
	needles := make([]string, 0, len(needleSet))
	for n := range needleSet {
		needles = append(needles, n)
	}
	regexes := make([]*regexp.Regexp, 0, len(opts.ScrubPatterns))
	for _, p := range opts.ScrubPatterns {
		if rx, ok := scrubRegexes[p]; ok {
			regexes = append(regexes, rx)
		}
	}

	c := &Client{
		opts:           opts,
		ch:             make(chan Entry, opts.MaxQueueSize),
		eventsCh:       make(chan ProductEvent, opts.MaxQueueSize),
		closed:         make(chan struct{}),
		scrubNeedles:   needles,
		scrubRegexes:   regexes,
		anonymousID:    fmt.Sprintf("anon_%d_%d", time.Now().UnixNano(), rand63()),
		userProperties: map[string]any{},
	}
	c.wg.Add(2)
	go c.loop()
	go c.eventsLoop()
	return c, nil
}

// rand63 devuelve un int63 pseudo-aleatorio sin necesidad de math/rand para no
// arrastrar la dep ni preocuparse por seeds. Suficiente para anonymous IDs
// (no es criptográfico).
func rand63() int64 {
	// Time.Now nanos + simple xor con un cycle-counter aproximación.
	return time.Now().UnixNano() ^ int64(time.Now().Nanosecond())<<13
}

// Log encola una entrada. No bloquea: descarta si la cola está llena.
func (c *Client) Log(level Severity, msg string, attrs map[string]any) {
	if c == nil {
		return
	}
	merged := make(map[string]string, len(c.opts.Attributes)+len(attrs)+2)
	for k, v := range c.opts.Attributes {
		merged[k] = v
	}
	if c.opts.Environment != "" {
		merged["deployment.environment"] = c.opts.Environment
	}
	if c.opts.Release != "" {
		merged["service.version"] = c.opts.Release
	}
	for k, v := range attrs {
		merged[k] = stringify(v)
	}
	entry := Entry{
		Level:      level,
		Message:    msg,
		Timestamp:  time.Now().UTC().Format(time.RFC3339Nano),
		Attributes: merged,
	}
	c.scrubEntry(&entry)
	if c.opts.BeforeSend != nil {
		out := c.opts.BeforeSend(&entry)
		if out == nil {
			return
		}
		entry = *out
	}
	select {
	case c.ch <- entry:
	default:
		c.opts.OnInternalError(fmt.Errorf("cola llena, evento descartado"))
	}
}

func (c *Client) scrubEntry(e *Entry) {
	for k, v := range e.Attributes {
		kLower := strings.ToLower(k)
		matched := false
		for _, n := range c.scrubNeedles {
			if strings.Contains(kLower, n) {
				e.Attributes[k] = redacted
				matched = true
				break
			}
		}
		if !matched && len(c.scrubRegexes) > 0 {
			for _, rx := range c.scrubRegexes {
				v = rx.ReplaceAllString(v, redacted)
			}
			e.Attributes[k] = v
		}
	}
	if len(c.scrubRegexes) > 0 {
		for _, rx := range c.scrubRegexes {
			e.Message = rx.ReplaceAllString(e.Message, redacted)
		}
	}
}

func (c *Client) Info(msg string, attrs map[string]any)    { c.Log(SevInfo, msg, attrs) }
func (c *Client) Warn(msg string, attrs map[string]any)    { c.Log(SevWarn, msg, attrs) }
func (c *Client) Warning(msg string, attrs map[string]any) { c.Log(SevWarn, msg, attrs) } // alias de Warn (paridad logging.WARNING)
func (c *Client) Error(msg string, attrs map[string]any)   { c.Log(SevError, msg, attrs) }

// CaptureException reporta un error con stack trace y tags opcionales.
func (c *Client) CaptureException(err error, tags map[string]string) {
	if c == nil || err == nil {
		return
	}
	attrs := make(map[string]any, len(tags)+3)
	attrs["exception.type"] = typeName(err)
	attrs["exception.message"] = err.Error()
	attrs["exception.stacktrace"] = string(debug.Stack())
	for k, v := range tags {
		attrs[k] = v
	}
	c.Log(SevError, err.Error(), attrs)
}

// ---------- Product events API (Segment/PostHog-like) ----------

// Track envía un evento custom de producto. Equivalente a `analytics.track`.
func (c *Client) Track(eventName string, properties map[string]any) {
	c.enqueueEvent("track", eventName, properties, nil, "")
}

// TrackContext envía un evento custom y adjunta trace_id/span_id desde ctx
// cuando contiene W3C tracecontext (por ejemplo vía WithTraceparent) o cuando
// Options.TraceContext sabe extraerlo de una integración OpenTelemetry.
func (c *Client) TrackContext(ctx context.Context, eventName string, properties map[string]any) {
	c.enqueueEventWithTrace("track", eventName, properties, nil, "", c.traceFromContext(ctx))
}

// Identify setea el `distinct_id` para los eventos siguientes y emite un
// `$identify` con los traits. El backend lo usa para mantener `product_users`.
func (c *Client) Identify(userID string, traits map[string]any) {
	if c == nil || userID == "" {
		return
	}
	c.identityMu.Lock()
	c.distinctID = userID
	if traits != nil {
		if c.userProperties == nil {
			c.userProperties = map[string]any{}
		}
		for k, v := range traits {
			c.userProperties[k] = v
		}
	}
	c.identityMu.Unlock()
	c.enqueueEvent("identify", "$identify", nil, traits, "")
}

// Alias fusiona una sesión pre-login (`prevID`) con un usuario post-login (`newID`).
// Tras invocarlo los eventos futuros usarán `newID` como `distinct_id`.
func (c *Client) Alias(prevID, newID string) {
	if c == nil || prevID == "" || newID == "" {
		return
	}
	c.identityMu.Lock()
	c.distinctID = newID
	c.identityMu.Unlock()
	c.enqueueEvent("alias", "$alias", nil, nil, prevID)
}

func (c *Client) enqueueEvent(typ, name string, properties, userPropsOverride map[string]any, anonOverride string) {
	c.enqueueEventWithTrace(typ, name, properties, userPropsOverride, anonOverride, TraceContext{})
}

func (c *Client) enqueueEventWithTrace(typ, name string, properties, userPropsOverride map[string]any, anonOverride string, trace TraceContext) {
	if c == nil {
		return
	}
	c.identityMu.Lock()
	distinct := c.distinctID
	anon := c.anonymousID
	userProps := c.userProperties
	c.identityMu.Unlock()
	if distinct == "" {
		distinct = anon
	}
	if anonOverride != "" {
		anon = anonOverride
	}
	if userPropsOverride != nil {
		userProps = userPropsOverride
	}
	if properties == nil {
		properties = map[string]any{}
	}
	ctx := make(map[string]any, len(c.opts.Attributes)+2)
	for k, v := range c.opts.Attributes {
		ctx[k] = v
	}
	if c.opts.Environment != "" {
		ctx["environment"] = c.opts.Environment
	}
	if c.opts.Release != "" {
		ctx["release"] = c.opts.Release
	}
	event := ProductEvent{
		Type:           typ,
		Name:           name,
		Timestamp:      time.Now().UTC().Format(time.RFC3339Nano),
		DistinctID:     distinct,
		AnonymousID:    anon,
		SessionID:      "",
		Properties:     properties,
		UserProperties: userProps,
		Context:        ctx,
		Source:         "backend",
	}
	trace = normalizeTraceContext(trace)
	if trace.TraceID != "" {
		event.TraceID = trace.TraceID
	}
	if trace.SpanID != "" {
		event.SpanID = trace.SpanID
	}
	select {
	case c.eventsCh <- event:
	default:
		c.opts.OnInternalError(fmt.Errorf("cola de events llena, descartado"))
	}
}

func (c *Client) traceFromContext(ctx context.Context) TraceContext {
	if c == nil {
		return TraceContext{}
	}
	if ctx == nil {
		ctx = context.Background()
	}
	if c.opts.TraceContext != nil {
		trace := normalizeTraceContext(c.opts.TraceContext(ctx))
		if trace.TraceID != "" {
			return trace
		}
	}
	if trace, ok := ctx.Value(traceContextKey{}).(TraceContext); ok {
		return normalizeTraceContext(trace)
	}
	return TraceContext{}
}

// Recover debe ir con defer en los puntos de entrada de goroutines. Captura los panics,
// los reporta a Faro y luego vuelve a lanzar el panic para mantener la semántica normal de Go.
func (c *Client) Recover(tags map[string]string) {
	if r := recover(); r != nil {
		var err error
		if e, ok := r.(error); ok {
			err = e
		} else {
			err = fmt.Errorf("panic: %v", r)
		}
		c.CaptureException(err, tags)
		_ = c.Flush(2 * time.Second)
		panic(r)
	}
}

// Flush bloquea hasta que ambas colas (logs + events) se vacíen o se cumpla el deadline.
func (c *Client) Flush(timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) && (len(c.ch) > 0 || len(c.eventsCh) > 0) {
		time.Sleep(50 * time.Millisecond)
	}
	if len(c.ch) > 0 || len(c.eventsCh) > 0 {
		return fmt.Errorf("timeout de flush con %d logs y %d events pendientes", len(c.ch), len(c.eventsCh))
	}
	return nil
}

// Close detiene el flusher en segundo plano. Es seguro llamarlo varias veces.
func (c *Client) Close(ctx context.Context) error {
	if c == nil {
		return nil
	}
	c.once.Do(func() {
		close(c.closed)
	})
	done := make(chan struct{})
	go func() {
		c.wg.Wait()
		close(done)
	}()
	select {
	case <-done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}

func (c *Client) loop() {
	defer c.wg.Done()
	ticker := time.NewTicker(c.opts.FlushInterval)
	defer ticker.Stop()

	batch := make([]Entry, 0, c.opts.MaxBatchSize)
	flush := func() {
		if len(batch) == 0 {
			return
		}
		ok := c.send(batch)
		if !ok {
			// 5xx o red caída: re-encolar para reintentar en el siguiente tick.
			// El canal puede estar lleno → caemos en la misma regla que Log(): descartar.
			for _, e := range batch {
				select {
				case c.ch <- e:
				default:
					c.opts.OnInternalError(fmt.Errorf("cola llena al reintentar, evento descartado"))
				}
			}
		}
		batch = batch[:0]
	}

	for {
		select {
		case <-c.closed:
			// Drena la cola restante y luego sale.
			for {
				select {
				case e := <-c.ch:
					batch = append(batch, e)
					if len(batch) >= c.opts.MaxBatchSize {
						flush()
					}
				default:
					flush()
					return
				}
			}
		case e := <-c.ch:
			batch = append(batch, e)
			if len(batch) >= c.opts.MaxBatchSize {
				flush()
			}
		case <-ticker.C:
			flush()
		}
	}
}

// eventsLoop es la versión events del loop principal. Misma cadencia y backoff
// que `loop()`, pero contra un canal de `ProductEvent` y `sendEvents`.
func (c *Client) eventsLoop() {
	defer c.wg.Done()
	ticker := time.NewTicker(c.opts.FlushInterval)
	defer ticker.Stop()

	batch := make([]ProductEvent, 0, c.opts.MaxBatchSize)
	flush := func() {
		if len(batch) == 0 {
			return
		}
		ok := c.sendEvents(batch)
		if !ok {
			for _, e := range batch {
				select {
				case c.eventsCh <- e:
				default:
					c.opts.OnInternalError(fmt.Errorf("cola de events llena al reintentar, descartado"))
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
				case e := <-c.eventsCh:
					batch = append(batch, e)
					if len(batch) >= c.opts.MaxBatchSize {
						flush()
					}
				default:
					flush()
					return
				}
			}
		case e := <-c.eventsCh:
			batch = append(batch, e)
			if len(batch) >= c.opts.MaxBatchSize {
				flush()
			}
		case <-ticker.C:
			flush()
		}
	}
}

// sendEvents espeja `send` pero contra `/ingest/events`.
func (c *Client) sendEvents(batch []ProductEvent) bool {
	body, err := json.Marshal(struct {
		Service string         `json:"service"`
		Events  []ProductEvent `json:"events"`
	}{Service: c.opts.Service, Events: batch})
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("marshal events: %w", err))
		return true
	}
	req, err := http.NewRequest(http.MethodPost, c.opts.Endpoint+"/api/v1/ingest/events", bytes.NewReader(body))
	if err != nil {
		c.opts.OnInternalError(err)
		return true
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.opts.Token)
	resp, err := c.opts.HTTPClient.Do(req)
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("ingest events: %w", err))
		return false
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		c.opts.OnInternalError(fmt.Errorf("ingest events HTTP %d", resp.StatusCode))
		return resp.StatusCode < 500
	}
	return true
}

// send devuelve true si el batch debe considerarse "entregado" (2xx o 4xx);
// false si fue 5xx o error de red — el caller re-encola.
func (c *Client) send(batch []Entry) bool {
	body, err := json.Marshal(struct {
		Service string  `json:"service"`
		Logs    []Entry `json:"logs"`
	}{Service: c.opts.Service, Logs: batch})
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("marshal: %w", err))
		return true // batch malformado, reintentar acumularía basura
	}
	req, err := http.NewRequest(http.MethodPost, c.opts.Endpoint+"/api/v1/ingest/logs", bytes.NewReader(body))
	if err != nil {
		c.opts.OnInternalError(err)
		return true
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.opts.Token)
	resp, err := c.opts.HTTPClient.Do(req)
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("ingest: %w", err))
		return false // red caída → reintentar
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		c.opts.OnInternalError(fmt.Errorf("ingest HTTP %d", resp.StatusCode))
		// 4xx → batch malformado / auth inválida; descartar. 5xx → reintentar.
		return resp.StatusCode < 500
	}
	return true
}

// HTTPMiddleware envuelve un http.Handler y captura panics + respuestas 5xx.
func (c *Client) HTTPMiddleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if rec := recover(); rec != nil {
				err, ok := rec.(error)
				if !ok {
					err = fmt.Errorf("%v", rec)
				}
				c.CaptureException(err, map[string]string{
					"http.method": r.Method,
					"http.path":   r.URL.Path,
				})
				http.Error(w, "Internal Server Error", http.StatusInternalServerError)
			}
		}()
		if traceparent := r.Header.Get("traceparent"); traceparent != "" {
			r = r.WithContext(WithTraceparent(r.Context(), traceparent))
		}
		next.ServeHTTP(w, r)
	})
}

// ---------- Helpers singleton a nivel de paquete ----------

var defaultClient *Client

func Init(opts Options) error {
	c, err := New(opts)
	if err != nil {
		return err
	}
	defaultClient = c
	return nil
}

func Default() *Client { return defaultClient }

func Log(level Severity, msg string, attrs map[string]any) { defaultClient.Log(level, msg, attrs) }
func Info(msg string, attrs map[string]any)                { defaultClient.Info(msg, attrs) }
func Warn(msg string, attrs map[string]any)                { defaultClient.Warn(msg, attrs) }
func Warning(msg string, attrs map[string]any)             { defaultClient.Warning(msg, attrs) }
func Error(msg string, attrs map[string]any)               { defaultClient.Error(msg, attrs) }
func CaptureException(err error, tags map[string]string)   { defaultClient.CaptureException(err, tags) }
func Track(eventName string, properties map[string]any)    { defaultClient.Track(eventName, properties) }
func TrackContext(ctx context.Context, eventName string, properties map[string]any) {
	defaultClient.TrackContext(ctx, eventName, properties)
}
func Identify(userID string, traits map[string]any) { defaultClient.Identify(userID, traits) }
func Alias(prevID, newID string)                    { defaultClient.Alias(prevID, newID) }
func Flush(timeout time.Duration) error             { return defaultClient.Flush(timeout) }
func Close(ctx context.Context) error               { return defaultClient.Close(ctx) }

// ---------- helpers ----------

func stringify(v any) string {
	switch x := v.(type) {
	case nil:
		return ""
	case string:
		return x
	case fmt.Stringer:
		return x.String()
	case error:
		return x.Error()
	default:
		b, err := json.Marshal(v)
		if err != nil {
			return fmt.Sprintf("%v", v)
		}
		return string(b)
	}
}

func normalizeTraceContext(trace TraceContext) TraceContext {
	trace.TraceID = normalizeTraceID(trace.TraceID, 32)
	trace.SpanID = normalizeTraceID(trace.SpanID, 16)
	return trace
}

func normalizeTraceID(value string, length int) string {
	value = strings.ToLower(strings.TrimSpace(value))
	if len(value) != length {
		return ""
	}
	allZero := true
	for _, ch := range value {
		if (ch < '0' || ch > '9') && (ch < 'a' || ch > 'f') {
			return ""
		}
		if ch != '0' {
			allZero = false
		}
	}
	if allZero {
		return ""
	}
	return value
}

func typeName(err error) string {
	if err == nil {
		return ""
	}
	t := fmt.Sprintf("%T", err)
	if i := strings.LastIndex(t, "."); i >= 0 {
		return t[i+1:]
	}
	return t
}
