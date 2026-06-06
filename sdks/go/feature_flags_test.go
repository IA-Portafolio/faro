// Tests de feature flags del SDK Go. Cubren:
//
//  1. Vectores dorados del hash sticky (FNV-1a 32-bit), idénticos a Node/Next.
//  2. Flag con rollout=100 → IsFeatureEnabled true + se encola $feature_exposure
//     con variant "B".
//  3. Flag con conditions.properties no satisfechas → false y sin exposición.
//
// httptest.NewServer sirve /api/v1/ingest/feature-flags y /api/v1/ingest/events.
package faro

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

// ---- 1. vectores dorados del hash ----

func TestStickyBucketVectoresDorados(t *testing.T) {
	cases := []struct {
		in   string
		want int
	}{
		{"proj:new-checkout:user_42", 9},
		{"acme:flag-a:anon_x", 54},
		{"myproj:dark-mode:user_1", 75},
		{"p:k:abcdefghij", 49},
		{"demo:exp1:user_42", 34},
	}
	for _, c := range cases {
		if got := stickyBucket(c.in); got != c.want {
			t.Errorf("stickyBucket(%q) = %d, want %d", c.in, got, c.want)
		}
	}
}

// flagServer sirve el endpoint de feature flags con una respuesta fija y
// captura los POST a /ingest/events para inspeccionar exposiciones.
type flagServer struct {
	cap     *captureServer
	flagsJS string
}

func newFlagServer(flagsJS string) *flagServer {
	return &flagServer{cap: newCaptureServer(), flagsJS: flagsJS}
}

func (f *flagServer) handler() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/api/v1/ingest/feature-flags" {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = io.WriteString(w, f.flagsJS)
			return
		}
		// El resto (events/logs) lo maneja el captureServer.
		f.cap.handler().ServeHTTP(w, r)
	})
}

// ---- 2. rollout=100 → habilitada + exposición variant B ----

func TestIsFeatureEnabledRollout100EmiteExposicion(t *testing.T) {
	fs := newFlagServer(`{
		"project": "demo",
		"flags": [
			{"key": "new-ui", "rollout_percentage": 100, "conditions": {}}
		]
	}`)
	srv := httptest.NewServer(fs.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint:                   srv.URL,
		Token:                      "tk",
		Service:                    "ff-test",
		FlushInterval:              50 * time.Millisecond,
		FeatureFlagRefreshInterval: 30 * time.Second, // sin auto-tick durante el test
		OnInternalError:            func(error) {},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close(context.Background())

	// Carga las flags de forma síncrona (sin esperar al ticker).
	c.RefreshFeatureFlags(context.Background())

	if !c.IsFeatureEnabled("new-ui", FlagContext{DistinctID: "user_1"}) {
		t.Fatalf("new-ui con rollout=100 debería estar habilitada")
	}
	// Una flag inexistente siempre es false.
	if c.IsFeatureEnabled("no-existe", FlagContext{DistinctID: "user_1"}) {
		t.Fatalf("flag inexistente debe ser false")
	}

	if err := c.Flush(2 * time.Second); err != nil {
		t.Fatal(err)
	}
	waitFor(t, 2*time.Second, func() bool {
		return len(featureExposureEvents(fs.cap.snapshot())) >= 1
	})

	exposures := featureExposureEvents(fs.cap.snapshot())
	if len(exposures) != 1 {
		t.Fatalf("esperaba exactamente 1 $feature_exposure; got %d (%+v)", len(exposures), exposures)
	}
	e := exposures[0]
	if e["distinct_id"] != "user_1" {
		t.Errorf("distinct_id override = %v, want user_1", e["distinct_id"])
	}
	props := e["properties"].(map[string]any)
	if props["flag_key"] != "new-ui" {
		t.Errorf("flag_key = %v, want new-ui", props["flag_key"])
	}
	if props["variant"] != "B" {
		t.Errorf("variant = %v, want B", props["variant"])
	}
	if props["enabled"] != true {
		t.Errorf("enabled = %v, want true", props["enabled"])
	}

	// Dedup: una segunda evaluación de la misma combinación no debe emitir otro evento.
	c.IsFeatureEnabled("new-ui", FlagContext{DistinctID: "user_1"})
	_ = c.Flush(time.Second)
	if got := len(featureExposureEvents(fs.cap.snapshot())); got != 1 {
		t.Errorf("la exposición debe dedup-arse; got %d eventos", got)
	}
}

// ---- 3. conditions.properties no satisfechas → false sin exposición ----

func TestIsFeatureEnabledCondicionesNoSatisfechas(t *testing.T) {
	fs := newFlagServer(`{
		"project": "demo",
		"flags": [
			{"key": "beta", "rollout_percentage": 100, "conditions": {"properties": {"plan": "pro"}}}
		]
	}`)
	srv := httptest.NewServer(fs.handler())
	defer srv.Close()

	c, err := New(Options{
		Endpoint:                   srv.URL,
		Token:                      "tk",
		Service:                    "ff-cond-test",
		FlushInterval:              50 * time.Millisecond,
		FeatureFlagRefreshInterval: 30 * time.Second,
		OnInternalError:            func(error) {},
	})
	if err != nil {
		t.Fatal(err)
	}
	defer c.Close(context.Background())

	c.RefreshFeatureFlags(context.Background())

	// plan != pro → no cumple condiciones → false, sin exposición.
	if c.IsFeatureEnabled("beta", FlagContext{DistinctID: "user_2", Properties: map[string]any{"plan": "free"}}) {
		t.Fatalf("beta no debe activarse con plan=free")
	}
	// Sin la propiedad → tampoco cumple.
	if c.IsFeatureEnabled("beta", FlagContext{DistinctID: "user_3"}) {
		t.Fatalf("beta no debe activarse sin plan")
	}
	// Con plan=pro → cumple condiciones y rollout=100 → true.
	if !c.IsFeatureEnabled("beta", FlagContext{DistinctID: "user_4", Properties: map[string]any{"plan": "pro"}}) {
		t.Fatalf("beta debe activarse con plan=pro")
	}

	_ = c.Flush(2 * time.Second)
	waitFor(t, 2*time.Second, func() bool {
		return len(featureExposureEvents(fs.cap.snapshot())) >= 1
	})

	exposures := featureExposureEvents(fs.cap.snapshot())
	// Solo user_4 (plan=pro) genera exposición; user_2/user_3 no.
	if len(exposures) != 1 {
		t.Fatalf("esperaba 1 exposición (solo plan=pro); got %d (%+v)", len(exposures), exposures)
	}
	if exposures[0]["distinct_id"] != "user_4" {
		t.Errorf("la única exposición debe ser de user_4; got %v", exposures[0]["distinct_id"])
	}
	if exposures[0]["properties"].(map[string]any)["variant"] != "B" {
		t.Errorf("variant esperado B")
	}
}

// ---- helpers ----

// featureExposureEvents extrae los eventos $feature_exposure de los batches.
func featureExposureEvents(batches []map[string]any) []map[string]any {
	var out []map[string]any
	for _, e := range eventsFromCapture(batches) {
		if e["name"] == "$feature_exposure" {
			out = append(out, e)
		}
	}
	return out
}

// Sanity: el body de feature-flags se decodifica como esperamos.
func TestFeatureFlagsResponseDecode(t *testing.T) {
	var body featureFlagsResponse
	js := `{"project":"p","flags":[{"key":"k","rollout_percentage":42,"conditions":{"properties":{"n":1}}}]}`
	if err := json.Unmarshal([]byte(js), &body); err != nil {
		t.Fatal(err)
	}
	if body.Project != "p" || len(body.Flags) != 1 {
		t.Fatalf("decode inesperado: %+v", body)
	}
	if body.Flags[0].Rollout != 42 {
		t.Errorf("rollout = %d, want 42", body.Flags[0].Rollout)
	}
}
