# ADR-0010: Añadir OTLP/gRPC como segundo transporte de ingesta

- **Estado**: Accepted
- **Fecha**: 2026-05-24
- **Autores**: @victalejo
- **Reemplaza**: parcialmente a [ADR-0004](0004-otlp-http-json-ingest.md) en lo
  relativo a "únicamente OTLP/HTTP+JSON".

## Contexto

ADR-0004 (mayo 2026) decidió implementar **solo** OTLP/HTTP+JSON en `:4318`
argumentando que protobuf/gRPC añadían dependencias y complejidad sin
beneficio para un proyecto self-hosted de tráfico moderado. La decisión era
correcta para el escenario en que Faro recibía telemetría de SDKs **propios**
(los `@iaportafolio/*` que publicamos), donde controlamos el transporte.

Lo que cambió:

1. **Faro debe poder recibir de SDKs oficiales de OpenTelemetry** —
   Java/Spring, .NET, Python, Go, Ruby. Esos SDKs por **default** exportan a
   OTLP/gRPC en `:4317`; cambiar `OTEL_EXPORTER_OTLP_PROTOCOL=http/json` es
   posible pero (a) no todo el mundo lo sabe, (b) varios stacks (especialmente
   Java) tienen el path JSON menos probado y aparecen bugs sutiles de
   serialización que en gRPC no existen.
2. **Compatibilidad con colectores intermedios** — un setup típico de empresa
   usa OpenTelemetry Collector como proxy/agregador antes de mandar a un
   backend. El Collector habla gRPC por default. Pedirle que use HTTP/JSON es
   fricción innecesaria.
3. **`opentelemetry-proto` con `gen-tonic` ya trae los stubs generados** — la
   parte cara que ADR-0004 quería evitar (versionar archivos `.proto` y
   regenerarlos) la asume el crate upstream. Para nosotros son ~10 líneas en
   `Cargo.toml` y un módulo `ingest/otlp_grpc.rs` que mapea las requests gRPC
   a las mismas `LogRow` / `SpanRow` / `MetricRow` que ya consume el writer.

## Decisión

Levantamos un **segundo listener** en `:4317` con OTLP/gRPC. Implementa los
tres servicios estándar (`logs`, `traces`, `metrics`) usando `tonic` +
`opentelemetry-proto`. Reusa la misma autenticación por token de proyecto
(extraído del `authorization` metadata gRPC en vez del header HTTP), los
mismos canales `mpsc` hacia el writer, y los mismos counters de
`faro_ingest_records_total`.

HTTP/JSON en `:4318` **no se elimina** — sigue siendo el path soportado y
recomendado para SDKs propios y curl ad-hoc.

## Alternativas consideradas

- **Seguir solo con HTTP/JSON** (status quo de ADR-0004) — documentar
  fuertemente que los SDKs oficiales requieren cambiar el protocolo. Funciona
  pero cierra la puerta a "instalar el SDK oficial y apuntar a Faro" sin
  configuración extra, que es el caso que un onboarding nuevo espera.
- **Solo OTLP/gRPC** (eliminando JSON) — bajaría el bytes-on-wire pero rompe
  los SDKs `@iaportafolio/*` actuales (que mandan JSON), los ejemplos de
  `curl` de la documentación, y la facilidad de debug con `jq`. No vale la
  pena.
- **Pasar todo por un OpenTelemetry Collector embebido como sidecar** —
  agrega un proceso al compose y un binario de Go al stack. Para single-VM
  self-hosted es sobreingeniería.

## Consecuencias

### Positivas

- **SDKs oficiales de OTel funcionan sin configurar nada** apuntando a
  `:4317`. Eso es lo que un usuario nuevo espera y reduce la fricción de
  adopción.
- **Compatibilidad con OpenTelemetry Collector** en deploys que ya lo tengan.
- **Misma superficie de auth y storage** — el handler gRPC reusa
  `state.resolve_project_token()` y los mismos canales del writer. No hay
  ruta paralela que pueda divergir.

### Negativas / costo asumido

- **+4 deps**: `tonic`, `opentelemetry-proto`, `prost`, `prost-types`. ~3 MB
  compilados, ~30 s extra en build limpio.
- **Un puerto más expuesto**: `:4317` se suma al docker-compose. Operadores
  con firewalls estrictos tienen que abrirlo (o cerrar si no lo usan).
- **HTTP/2 obligatorio**: gRPC requiere HTTP/2. Si alguien pone un proxy L7
  legacy delante (cosa rara hoy), puede no funcionar — documentado en
  `docs/deployment.md`.
- **`opentelemetry-proto` arrastra `paste` unmaintained** vía sus macros de
  prost. Tracked en el `--ignore` de `cargo audit` (RUSTSEC-2024-0436) hasta
  que el ecosistema lo migre.

### Trabajo de seguimiento

- **[ADR-0004](0004-otlp-http-json-ingest.md)**: marcada como `Superseded by
  ADR-0010` en lo relativo a la decisión "únicamente JSON". El resto del
  contexto (por qué JSON sigue siendo válido) sigue siendo correcto.
- **Documentar en READMEs de SDKs oficiales** ejemplos apuntando a `:4317`
  además de `:4318` (el equipo Java agradece).
- **Métricas de gRPC**: hoy los handlers gRPC incrementan
  `faro_ingest_records_total` igual que los HTTP, pero el layer
  `axum-prometheus` no mide tonic (porque no pasa por axum). Si queremos
  histogramas de latencia por servicio gRPC hay que añadirlos a mano con
  `metrics::histogram!` en cada handler.
- **Reflection service** (`tonic-reflection`) opcional para que `grpcurl`
  pueda introspectar los servicios sin tener los `.proto` localmente — útil
  para debug pero suma ~200 KB; lo dejamos para si aparece la necesidad.
