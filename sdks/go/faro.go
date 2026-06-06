// Package faro es el SDK de Go para Faro (https://github.com/iaportafolio/faro).
package faro

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"reflect"
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

	// FeatureFlagRefreshInterval es la cadencia con la que el SDK refresca las
	// feature flags desde `GET /api/v1/ingest/feature-flags`. Por defecto 30s.
	// El primer refresh ocurre tras el primer tick (no hay fetch inicial).
	FeatureFlagRefreshInterval time.Duration

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

// FlagContext son los inputs de evaluación de una feature flag. DistinctID
// permite evaluar para un usuario concreto (cae a distinctID/anonymousID del
// cliente si va vacío) y Properties alimenta el matching de condiciones.
type FlagContext struct {
	DistinctID string
	Properties map[string]any
}

// flagDef es la representación interna (post-normalización) de una feature flag
// recibida por la red. Rollout ya viene con clamp 0..100 y Conditions es el
// objeto de condiciones o nil.
type flagDef struct {
	Key        string
	Rollout    int
	Conditions map[string]any
}

// featureFlagsResponse espeja el JSON de `GET /api/v1/ingest/feature-flags`.
type featureFlagsResponse struct {
	Project string `json:"project"`
	Flags   []struct {
		Key        string         `json:"key"`
		Rollout    int            `json:"rollout_percentage"`
		Conditions map[string]any `json:"conditions"`
	} `json:"flags"`
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

	// Estado de feature flags. Lo protegemos con un RWMutex porque
	// `IsFeatureEnabled` (lecturas frecuentes desde request handlers) compite
	// con `RefreshFeatureFlags` (escrituras periódicas del ticker en segundo
	// plano). `featureExposureSeen` dedup-a la emisión de `$feature_exposure`.
	flagsMu             sync.RWMutex
	featureFlags        map[string]flagDef
	featureFlagsProject string
	featureExposureSeen map[string]struct{}
	flagsStop           chan struct{}
	flagsStopOnce       sync.Once
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
	if opts.FeatureFlagRefreshInterval == 0 {
		opts.FeatureFlagRefreshInterval = 30 * time.Second
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

		featureFlags:        map[string]flagDef{},
		featureExposureSeen: map[string]struct{}{},
		flagsStop:           make(chan struct{}),
	}

	// Bootstrap OTel tracing — idempotente, no-op si ya fue inicializado.
	// Los spans se exportan via el BatchSpanProcessor del NodeTracerProvider
	// hacia ${Endpoint}/v1/traces con el bearer token del proyecto.
	if _, err := InitTracing(TracingOptions{
		Endpoint:        opts.Endpoint,
		Token:           opts.Token,
		Service:         opts.Service,
		Environment:     opts.Environment,
		Release:         opts.Release,
		HTTPClient:      opts.HTTPClient,
		OnInternalError: opts.OnInternalError,
	}); err != nil {
		opts.OnInternalError(fmt.Errorf("faro: InitTracing falló: %w", err))
	}

	c.wg.Add(3)
	go c.loop()
	go c.eventsLoop()
	go c.featureFlagsLoop()
	return c, nil
}

// featureFlagsLoop refresca las feature flags en cada tick. No hace fetch
// inicial inmediato (igual que el SDK de Node): el primer refresh ocurre tras
// el primer tick. Termina cuando se señaliza `flagsStop` (lo hace Close).
func (c *Client) featureFlagsLoop() {
	defer c.wg.Done()
	ticker := time.NewTicker(c.opts.FeatureFlagRefreshInterval)
	defer ticker.Stop()
	for {
		select {
		case <-c.flagsStop:
			return
		case <-ticker.C:
			c.RefreshFeatureFlags(context.Background())
		}
	}
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

// LogContext emite un log adjuntando el trace_id/span_id del span activo en
// ctx (si hay). Equivalente a Log pero con auto-correlación.
func (c *Client) LogContext(ctx context.Context, level Severity, msg string, attrs map[string]any) {
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
	// Auto-correlación con el span activo en ctx (Faro o auto-instrumentado).
	if span := SpanFromContext(ctx); span != nil {
		entry.TraceID = span.TraceID()
		entry.SpanID = span.SpanID()
	} else if c.opts.TraceContext != nil {
		tc := normalizeTraceContext(c.opts.TraceContext(ctx))
		if tc.TraceID != "" {
			entry.TraceID = tc.TraceID
			entry.SpanID = tc.SpanID
		}
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

// InfoContext / WarnContext / ErrorContext — variantes context-aware con auto-correlación.
func (c *Client) InfoContext(ctx context.Context, msg string, attrs map[string]any) {
	c.LogContext(ctx, SevInfo, msg, attrs)
}
func (c *Client) WarnContext(ctx context.Context, msg string, attrs map[string]any) {
	c.LogContext(ctx, SevWarn, msg, attrs)
}
func (c *Client) ErrorContext(ctx context.Context, msg string, attrs map[string]any) {
	c.LogContext(ctx, SevError, msg, attrs)
}

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

// enqueueEventWithDistinct encola un evento sobrescribiendo el `distinct_id`
// (lo usa feature exposure con FlagContext.DistinctID o el id resuelto).
func (c *Client) enqueueEventWithDistinct(typ, name string, properties map[string]any, distinctOverride string) {
	c.enqueueEventFull(typ, name, properties, nil, "", distinctOverride, TraceContext{})
}

func (c *Client) enqueueEventWithTrace(typ, name string, properties, userPropsOverride map[string]any, anonOverride string, trace TraceContext) {
	c.enqueueEventFull(typ, name, properties, userPropsOverride, anonOverride, "", trace)
}

func (c *Client) enqueueEventFull(typ, name string, properties, userPropsOverride map[string]any, anonOverride, distinctOverride string, trace TraceContext) {
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
	if distinctOverride != "" {
		distinct = distinctOverride
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

// ---------- Feature flags ----------

// RefreshFeatureFlags trae el set de feature flags desde
// `GET /api/v1/ingest/feature-flags` y reemplaza el estado interno bajo lock.
// NUNCA panica hacia el usuario: ante cualquier fallo (HTTP no-2xx, body
// inválido, red caída) deja un diag log vía OnInternalError y conserva las
// flags actuales. El parámetro ctx permite cancelación/timeout del request.
func (c *Client) RefreshFeatureFlags(ctx context.Context) {
	if c == nil {
		return
	}
	if ctx == nil {
		ctx = context.Background()
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, c.opts.Endpoint+"/api/v1/ingest/feature-flags", nil)
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("feature flags: %w", err))
		return
	}
	req.Header.Set("Authorization", "Bearer "+c.opts.Token)
	resp, err := c.opts.HTTPClient.Do(req)
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("feature flags: %w", err))
		return
	}
	defer resp.Body.Close()
	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		c.opts.OnInternalError(fmt.Errorf("feature flags HTTP %d", resp.StatusCode))
		return
	}
	var body featureFlagsResponse
	if err := json.NewDecoder(resp.Body).Decode(&body); err != nil {
		c.opts.OnInternalError(fmt.Errorf("feature flags response inválida: %w", err))
		return
	}
	next := make(map[string]flagDef, len(body.Flags))
	for _, f := range body.Flags {
		if f.Key == "" {
			continue
		}
		next[f.Key] = flagDef{
			Key:        f.Key,
			Rollout:    clampRollout(f.Rollout),
			Conditions: f.Conditions, // map o nil
		}
	}
	c.flagsMu.Lock()
	c.featureFlags = next
	c.featureFlagsProject = body.Project
	c.flagsMu.Unlock()
}

// IsFeatureEnabled evalúa una feature flag para el contexto dado. Devuelve
// false si la flag no existe o si sus condiciones no se cumplen. El rollout es
// "sticky" por (project, key, id): el mismo id cae siempre en el mismo bucket.
// Emite un evento `$feature_exposure` (dedup-ado) por cada combinación nueva.
func (c *Client) IsFeatureEnabled(key string, ctx FlagContext) bool {
	if c == nil {
		return false
	}
	c.flagsMu.RLock()
	flag, ok := c.featureFlags[key]
	project := c.featureFlagsProject
	c.flagsMu.RUnlock()
	if !ok {
		return false
	}
	if !matchesConditions(flag, ctx) {
		return false
	}
	rollout := clampRollout(flag.Rollout)
	id := ctx.DistinctID
	if id == "" {
		c.identityMu.Lock()
		id = c.distinctID
		anon := c.anonymousID
		c.identityMu.Unlock()
		if id == "" {
			id = anon
		}
	}
	enabled := rollout >= 100 || (rollout > 0 && stickyBucket(fmt.Sprintf("%s:%s:%s", project, key, id)) < rollout)
	c.trackFeatureExposure(project, key, id, enabled)
	return enabled
}

// matchesConditions devuelve true si el contexto satisface conditions.properties
// de la flag. Si no hay propiedades requeridas, siempre coincide. La igualdad es
// por valor (reflect.DeepEqual); ojo que los números de JSON llegan como float64.
func matchesConditions(flag flagDef, ctx FlagContext) bool {
	required, _ := flag.Conditions["properties"].(map[string]any)
	if required == nil {
		return true
	}
	for k, expected := range required {
		if !reflect.DeepEqual(ctx.Properties[k], expected) {
			return false
		}
	}
	return true
}

// trackFeatureExposure encola un evento de producto `$feature_exposure` la
// primera vez que se ve una combinación (project, flag, distinctID, variant).
// variant es "B" si la flag quedó habilitada y "A" si no.
func (c *Client) trackFeatureExposure(project, flagKey, distinctID string, enabled bool) {
	variant := "A"
	if enabled {
		variant = "B"
	}
	dedup := fmt.Sprintf("%s:%s:%s:%s", project, flagKey, distinctID, variant)
	c.flagsMu.Lock()
	if _, seen := c.featureExposureSeen[dedup]; seen {
		c.flagsMu.Unlock()
		return
	}
	c.featureExposureSeen[dedup] = struct{}{}
	c.flagsMu.Unlock()
	c.enqueueEventWithDistinct("track", "$feature_exposure", map[string]any{
		"flag_key": flagKey,
		"variant":  variant,
		"enabled":  enabled,
	}, distinctID)
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

// Flush bloquea hasta que las colas (logs + events) se vacíen y los spans pending
// del BatchSpanProcessor de OTel se drenen, o se cumpla el deadline.
func (c *Client) Flush(timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) && (len(c.ch) > 0 || len(c.eventsCh) > 0) {
		time.Sleep(50 * time.Millisecond)
	}
	// Drena los spans pending. Le pasamos el remaining como deadline al ctx.
	ctx, cancel := context.WithDeadline(context.Background(), deadline)
	defer cancel()
	_ = FlushTracing(ctx)
	if len(c.ch) > 0 || len(c.eventsCh) > 0 {
		return fmt.Errorf("timeout de flush con %d logs y %d events pendientes", len(c.ch), len(c.eventsCh))
	}
	return nil
}

// Close detiene el flusher en segundo plano y apaga el provider de OTel.
// Es seguro llamarlo varias veces.
func (c *Client) Close(ctx context.Context) error {
	if c == nil {
		return nil
	}
	c.once.Do(func() {
		close(c.closed)
	})
	// Señaliza al ticker de feature flags para que su goroutine termine.
	c.flagsStopOnce.Do(func() {
		close(c.flagsStop)
	})
	done := make(chan struct{})
	go func() {
		c.wg.Wait()
		close(done)
	}()
	select {
	case <-done:
		// Apagar el provider de OTel para drenar spans pending + cerrar exporter.
		return ShutdownTracing(ctx)
	case <-ctx.Done():
		_ = ShutdownTracing(context.Background())
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
func LogContext(ctx context.Context, level Severity, msg string, attrs map[string]any) {
	defaultClient.LogContext(ctx, level, msg, attrs)
}
func InfoContext(ctx context.Context, msg string, attrs map[string]any) {
	defaultClient.InfoContext(ctx, msg, attrs)
}
func WarnContext(ctx context.Context, msg string, attrs map[string]any) {
	defaultClient.WarnContext(ctx, msg, attrs)
}
func ErrorContext(ctx context.Context, msg string, attrs map[string]any) {
	defaultClient.ErrorContext(ctx, msg, attrs)
}
func CaptureException(err error, tags map[string]string) { defaultClient.CaptureException(err, tags) }
func Track(eventName string, properties map[string]any)  { defaultClient.Track(eventName, properties) }
func TrackContext(ctx context.Context, eventName string, properties map[string]any) {
	defaultClient.TrackContext(ctx, eventName, properties)
}
func Identify(userID string, traits map[string]any) { defaultClient.Identify(userID, traits) }
func Alias(prevID, newID string)                    { defaultClient.Alias(prevID, newID) }
func RefreshFeatureFlags(ctx context.Context)       { defaultClient.RefreshFeatureFlags(ctx) }
func IsFeatureEnabled(key string, ctx FlagContext) bool {
	return defaultClient.IsFeatureEnabled(key, ctx)
}
func Flush(timeout time.Duration) error { return defaultClient.Flush(timeout) }
func Close(ctx context.Context) error   { return defaultClient.Close(ctx) }

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

// clampRollout acota n al rango [0, 100].
func clampRollout(n int) int {
	if n < 0 {
		return 0
	}
	if n > 100 {
		return 100
	}
	return n
}

// stickyBucket mapea s a un bucket [0, 100) de forma determinista usando
// FNV-1a de 32 bits (mismo algoritmo que el SDK de Node). Para inputs ASCII
// el resultado es idéntico entre runtimes.
func stickyBucket(s string) int {
	var h uint32 = 0x811c9dc5
	for _, r := range s {
		h ^= uint32(r)
		h *= 0x01000193
	}
	return int(h % 100)
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
