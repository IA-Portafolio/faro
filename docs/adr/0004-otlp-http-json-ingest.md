# ADR-0004: OTLP/HTTP+JSON como contrato de ingesta

- **Estado**: Accepted
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

## Contexto

OpenTelemetry define tres encodings de transporte: OTLP/HTTP+protobuf,
OTLP/HTTP+JSON y OTLP/gRPC. Tenemos que elegir cuál(es) implementar
en el listener `:4318` del backend, balanceando esfuerzo de
implementación, compatibilidad con SDKs upstream y facilidad de
debuggeo.

## Decisión

Implementamos **únicamente OTLP/HTTP+JSON**. Los SDKs deben configurar
`OTEL_EXPORTER_OTLP_PROTOCOL=http/json` para enviar a Faro. El path
sigue la convención estándar de OTLP:

- `POST /v1/logs`
- `POST /v1/traces`
- `POST /v1/metrics`

## Alternativas consideradas

- **OTLP/HTTP+protobuf** — el formato más eficiente en bytes y CPU.
  Pero requiere `prost`/`tonic`, ~6 archivos `.proto` versionados a
  upstream OTEL, y debuggear payloads requiere descodificar protobuf.
  Para un proyecto self-hosted con tráfico moderado, no compensa.
- **OTLP/gRPC** — todavía más eficiente, pero implica soportar HTTP/2,
  un puerto adicional (típicamente :4317), y un cliente de gRPC en el
  evaluador de alertas si quisiéramos consumirlo internamente. Mismo
  argumento: overhead injustificado.

## Consecuencias

### Positivas
- Implementación trivial con `axum` + `serde_json` — un handler por
  endpoint y deserialización derivada.
- Payloads inspeccionables con `curl` y `jq`, lo que facilita debug
  de SDKs nuevos.
- Compatible con todos los SDKs oficiales de OTel — solo requieren
  la env var correspondiente.

### Negativas / costo asumido
- ~3-5x más bytes en el wire que protobuf. Para Faro single-VM con
  tráfico modesto es aceptable; si una instancia llega a saturarse
  por bandwidth de ingesta, se evalúa añadir protobuf.
- Algunos SDKs (especialmente Java/Spring) tienen el protocolo
  protobuf más sólidamente probado — pequeñas incompatibilidades JSON
  pueden aparecer.

### Trabajo de seguimiento
- Añadir soporte de protobuf como _opcional_ si un cliente real
  reporta problemas de bandwidth.
- Documentar explícitamente la limitación en READMEs de cada SDK.
