# ADR-0008: Compatibilidad SDK ↔ backend vía Faro-Protocol-Version

- **Estado**: Accepted
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

## Contexto

Tenemos **7 SDKs** publicados independientemente en 5 registries
distintos (npm x3, PyPI, Go modules, pub.dev, Maven Central). Cada uno
se versiona y publica por separado. El backend evoluciona en su propia
cadencia.

Hoy **no hay nada** que detecte si un SDK desactualizado está enviando
payloads que el backend ya no soporta, o al revés. Un dev que instale
`@iaportafolio/node@0.1.0` contra un backend que ya está en `v0.5.0`
con un breaking change descubre el problema en producción.

Las opciones que se discutieron en el meta-listado de PRs:

- Un campo `min_supported_sdk` por lenguaje en `/healthz` —
  desproporcionado: requiere mantener 7 versiones en el backend.
- Un header `Faro-Protocol-Version` como contrato único — todos los
  SDKs se mapean al mismo entero. Más simple de razonar y de evolucionar.

## Decisión

Introducimos un **único entero de protocolo wire** llamado
`Faro-Protocol-Version`. Empieza en `1` y se incrementa **solo** cuando
hay un breaking change en el contrato de ingesta (no por bug fixes ni
por campos nuevos opcionales).

### Backend

- Declara su rango soportado en constantes en `crate::versions`:
  `PROTOCOL_CURRENT`, `PROTOCOL_MIN_SUPPORTED`, `PROTOCOL_MAX_SUPPORTED`.
- `/healthz` devuelve `{ status, version, protocol: { current, min_supported, max_supported } }`.
- En cada request de ingesta, lee `Faro-Protocol-Version` y lo
  clasifica como `Ok` / `Deprecated` / `Unsupported`.
- **Política actual** (esta PR): solo loggea — no rechaza. Esto nos
  da observabilidad de qué versiones están en uso real antes de subir
  el mínimo.
- **Política futura** (próxima PR, gated por env):
  `Unsupported` → 400 con `Faro-Compat: unsupported`. `Deprecated` →
  acepta con header de respuesta `Faro-Compat: deprecated`.

### SDKs

- Constante `FARO_PROTOCOL_VERSION = "1"` en cada SDK.
- Headers que se envían en cada request:
  - `Faro-Protocol-Version: <n>` (obligatorio).
  - `Faro-SDK-Name: <lang>` (telemetría).
  - `Faro-SDK-Version: <semver>` (telemetría).
- Al inicializar el cliente, hacen `GET /healthz`. Si la versión local
  del SDK declara un protocolo fuera del rango `[min_supported,
  max_supported]` del backend, emiten **warning** al logger del SDK
  con instrucciones de actualizar.

### Política de evolución del protocolo

| Tipo de cambio                                  | Bump protocolo |
| ----------------------------------------------- | -------------- |
| Campo nuevo opcional en payload                 | NO             |
| Endpoint nuevo                                  | NO             |
| Cambio de bug que altera respuesta              | NO             |
| Campo requerido renombrado / removido           | SÍ             |
| Cambio de semántica de un campo existente       | SÍ             |
| Cambio en el contrato de auth (token format)    | SÍ             |

Cuando hay un bump, el backend debe soportar **al menos N-1** durante
una ventana de transición (default: 90 días) antes de subir
`PROTOCOL_MIN_SUPPORTED`.

## Alternativas consideradas

- **`min_supported_sdk` por lenguaje** — N+1 versiones a mantener,
  difícil de mapear conceptualmente ("¿el SDK Python 0.2.5 corresponde
  al SDK Node de qué?").
- **Negociación dinámica** (cliente y server intercambian capacidades)
  — overkill para el caso de uso. Un entero monotónico es suficiente.
- **Versionar el path** (`/api/v2/ingest/logs`) — funciona pero acopla
  el versionado del protocolo al routing del backend y obliga a
  mantener handlers duplicados.
- **No hacer nada** — opción válida pero la sentencia es: ya tienes 7
  surfaces que pueden quedarse atrás; sin un mecanismo, la primera
  vez que rompas el wire es un incidente.

## Consecuencias

### Positivas

- **Detección temprana**: un SDK desactualizado loggea warning al
  init, antes de que falle en producción.
- **Telemetría de adopción**: los logs de `log_sdk_compat` muestran
  qué SDKs y versiones están en uso real → datos para decidir cuándo
  subir el mínimo.
- **Bump cost real, no implícito**: subir `PROTOCOL_CURRENT` requiere
  una decisión consciente con plan de transición. Esto desincentiva
  cambios gratuitos de wire.
- **Un solo número** — fácil de razonar, comunicar y testear.

### Negativas / costo asumido

- **Disciplina**: hay que recordar bumpear el entero cuando toca, y
  bumpear la constante en los 7 SDKs cuando un SDK añade soporte para
  un protocolo nuevo. Mitigamos con la tabla "¿bump o no?" arriba.
- **`Faro-Protocol-Version` faltante asume current** — los clientes
  curl ad-hoc y los SDKs viejos sin la constante seguirán funcionando
  bien. Es intencional, pero significa que no se puede detectar al
  100% de los clientes desactualizados.
- **Tres headers extra por request** — ~80 bytes overhead. Negligible.

### Trabajo de seguimiento

Esta ADR cierra con el **scaffolding** del PR
`feat/sdk-version-validation`:

- ✅ `crate::versions` con constantes, `HealthResponse`, `CompatStatus`
  y `classify_protocol()`.
- ✅ `/healthz` extendido con info de protocolo.
- ✅ `log_sdk_compat()` llamado en `ingest_logs` (solo warn, no
  rechaza).
- ✅ Tests unitarios de `classify_protocol`.

Queda por hacer (PRs incrementales):

- [ ] **SDKs**: añadir `FARO_PROTOCOL_VERSION` + headers + GET
      /healthz al init. Un PR pequeño por SDK.
- [ ] **Backend**: política de rechazo gated por
      `FARO_REJECT_UNSUPPORTED_PROTOCOL=true` (default off).
- [ ] **Backend**: extender `log_sdk_compat` a los otros endpoints de
      ingesta (OTLP traces/metrics/logs).
- [ ] **CI**: contract tests — cada SDK corre suite contra una imagen
      Docker del backend del repo, verifica que el wire funciona.
- [ ] **Métrica**: contador por `(sdk_name, sdk_version, status)` para
      visualizar adopción en la propia UI de Faro.
- [ ] **Docs**: sección "Versionado del protocolo" en README explicando
      qué significa el header y cómo se evoluciona.
