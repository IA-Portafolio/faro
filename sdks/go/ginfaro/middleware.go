// Package ginfaro provee middlewares de Gin para Faro.
//
//	r := gin.New()
//	r.Use(ginfaro.Tracing()) // crea un span SERVER por request
//
// El span hereda el W3C traceparent entrante (header `traceparent`) y propaga
// el del span actual en el response. Logs emitidos con `faro.InfoContext(c.Request.Context(), ...)`
// dentro del handler auto-heredan trace_id/span_id.
package ginfaro

import (
	"fmt"

	"github.com/IA-Portafolio/faro/sdks/go"
	"github.com/gin-gonic/gin"
)

// TracingOptions parametriza el middleware.
type TracingOptions struct {
	// SpanName: nombre del span. Default `<METHOD> <route>`. Recibe el ctx Gin
	// para tomar `FullPath()` (el patrón del route, p.ej. "/users/:id"), no la
	// URL ya rellena — eso baja la cardinalidad de los nombres de span.
	SpanName func(c *gin.Context) string
}

// Tracing devuelve un gin.HandlerFunc que crea un span SERVER por request.
func Tracing(opts ...TracingOptions) gin.HandlerFunc {
	var o TracingOptions
	if len(opts) > 0 {
		o = opts[0]
	}
	return func(c *gin.Context) {
		req := c.Request

		// Nombre del span: por defecto usamos el route pattern de Gin si está disponible
		// (lo registra c.FullPath() cuando el router ya hizo el match). En el primer
		// middleware el match todavía no ocurrió; usamos la URL como fallback.
		name := ""
		if o.SpanName != nil {
			name = o.SpanName(c)
		}
		if name == "" {
			route := c.FullPath()
			if route == "" {
				route = req.URL.Path
			}
			name = fmt.Sprintf("%s %s", req.Method, route)
		}

		parent := faro.SpanParent{Traceparent: req.Header.Get("traceparent")}
		ctx, span := faro.StartSpan(req.Context(), name, faro.SpanOptions{
			Kind: faro.SpanKindServer,
			Attributes: map[string]any{
				"http.method": req.Method,
				"http.target": req.URL.RequestURI(),
				"http.route":  c.FullPath(),
				"net.peer.ip": c.ClientIP(),
			},
			Parent: parent,
		})
		if span != nil {
			// Propaga al response para que downstream (otros servicios) tomen este como padre.
			c.Header("traceparent", span.Traceparent())
		}
		c.Request = req.WithContext(ctx)

		c.Next()

		if span == nil {
			return
		}
		status := c.Writer.Status()
		span.SetAttribute("http.status_code", status)
		if len(c.Errors) > 0 {
			// Gin acumula errores en c.Errors — el último suele ser el más relevante.
			span.RecordException(c.Errors.Last().Err)
		} else if status >= 500 {
			span.SetStatus(faro.StatusError, fmt.Sprintf("HTTP %d", status))
		} else {
			span.SetStatus(faro.StatusOK, "")
		}
		span.End()
	}
}
