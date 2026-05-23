// Package faro is the Go SDK for Faro (https://github.com/iaportafolio/faro).
package faro

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
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

// Options configures the SDK.
type Options struct {
	Endpoint         string            // https://faro.iaportafolio.com
	Token            string            // project ingest token
	Service          string            // service.name
	Environment      string            // deployment.environment
	Release          string            // service.version
	Attributes       map[string]string // default attrs
	FlushInterval    time.Duration     // default 750ms
	MaxBatchSize     int               // default 200
	MaxQueueSize     int               // default 10_000
	HTTPTimeout      time.Duration     // default 5s
	HTTPClient       *http.Client      // injectable
	OnInternalError  func(err error)   // called on background failures; defaults to stderr
}

// Entry is a single log event ready for the wire.
type Entry struct {
	Level      Severity          `json:"level"`
	Message    string            `json:"message"`
	Timestamp  string            `json:"timestamp,omitempty"`
	TraceID    string            `json:"trace_id,omitempty"`
	SpanID     string            `json:"span_id,omitempty"`
	Attributes map[string]string `json:"attributes,omitempty"`
}

type Client struct {
	opts   Options
	ch     chan Entry
	wg     sync.WaitGroup
	closed chan struct{}
	once   sync.Once
}

// New starts a Faro client and returns it. It also spawns a background flusher.
func New(opts Options) (*Client, error) {
	if opts.Endpoint == "" {
		opts.Endpoint = os.Getenv("FARO_ENDPOINT")
	}
	if opts.Token == "" {
		opts.Token = os.Getenv("FARO_TOKEN")
	}
	if opts.Endpoint == "" || opts.Token == "" {
		return nil, fmt.Errorf("faro: Endpoint and Token are required")
	}
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
	c := &Client{
		opts:   opts,
		ch:     make(chan Entry, opts.MaxQueueSize),
		closed: make(chan struct{}),
	}
	c.wg.Add(1)
	go c.loop()
	return c, nil
}

// Log enqueues an entry. Non-blocking: drops if the queue is full.
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
	select {
	case c.ch <- entry:
	default:
		c.opts.OnInternalError(fmt.Errorf("queue full, dropping event"))
	}
}

func (c *Client) Info(msg string, attrs map[string]any)  { c.Log(SevInfo, msg, attrs) }
func (c *Client) Warn(msg string, attrs map[string]any)  { c.Log(SevWarn, msg, attrs) }
func (c *Client) Error(msg string, attrs map[string]any) { c.Log(SevError, msg, attrs) }

// CaptureException reports an error with stack trace + optional tags.
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

// Recover should be deferred at goroutine entry points. It captures panics,
// reports them to Faro, then re-panics so normal Go semantics apply.
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

// Flush blocks until either the queue is empty or the deadline passes.
func (c *Client) Flush(timeout time.Duration) error {
	deadline := time.Now().Add(timeout)
	for time.Now().Before(deadline) && len(c.ch) > 0 {
		time.Sleep(50 * time.Millisecond)
	}
	if len(c.ch) > 0 {
		return fmt.Errorf("flush timed out with %d events pending", len(c.ch))
	}
	return nil
}

// Close stops the background flusher. Safe to call multiple times.
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
		c.send(batch)
		batch = batch[:0]
	}

	for {
		select {
		case <-c.closed:
			// Drain remaining queue then exit.
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

func (c *Client) send(batch []Entry) {
	body, err := json.Marshal(struct {
		Service string  `json:"service"`
		Logs    []Entry `json:"logs"`
	}{Service: c.opts.Service, Logs: batch})
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("marshal: %w", err))
		return
	}
	req, err := http.NewRequest(http.MethodPost, c.opts.Endpoint+"/api/v1/ingest/logs", bytes.NewReader(body))
	if err != nil {
		c.opts.OnInternalError(err)
		return
	}
	req.Header.Set("Content-Type", "application/json")
	req.Header.Set("Authorization", "Bearer "+c.opts.Token)
	resp, err := c.opts.HTTPClient.Do(req)
	if err != nil {
		c.opts.OnInternalError(fmt.Errorf("ingest: %w", err))
		return
	}
	defer resp.Body.Close()
	if resp.StatusCode >= 400 {
		c.opts.OnInternalError(fmt.Errorf("ingest HTTP %d", resp.StatusCode))
	}
}

// HTTPMiddleware wraps an http.Handler and captures panics + 5xx responses.
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
		next.ServeHTTP(w, r)
	})
}

// ---------- Package-level singleton helpers ----------

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

func Log(level Severity, msg string, attrs map[string]any)   { defaultClient.Log(level, msg, attrs) }
func Info(msg string, attrs map[string]any)                  { defaultClient.Info(msg, attrs) }
func Warn(msg string, attrs map[string]any)                  { defaultClient.Warn(msg, attrs) }
func Error(msg string, attrs map[string]any)                 { defaultClient.Error(msg, attrs) }
func CaptureException(err error, tags map[string]string)     { defaultClient.CaptureException(err, tags) }
func Flush(timeout time.Duration) error                      { return defaultClient.Flush(timeout) }
func Close(ctx context.Context) error                        { return defaultClient.Close(ctx) }

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
