# Política de seguridad

## Versiones soportadas

Faro está en desarrollo activo. Solo la rama `main` recibe parches de seguridad.

| Versión | Soporte |
| ------- | ------- |
| `main`  | ✅      |
| < `0.1` | ❌      |

## Cómo reportar una vulnerabilidad

**No abras un issue público.** En su lugar, usa una de estas vías:

1. **GitHub Security Advisories** (preferido) — abre un reporte privado desde
   <https://github.com/IA-Portafolio/faro/security/advisories/new>. Esto crea
   un canal cifrado solo visible para los mantenedores.
2. **Email** — `victoralejocj@gmail.com` con asunto `[faro][security]`.

Por favor incluye:

- Componente afectado (backend, frontend, un SDK específico, infra).
- Versión / commit del hallazgo.
- Pasos para reproducir o PoC.
- Impacto estimado (lectura/escritura de datos, RCE, DoS, etc.).

## Tiempos de respuesta

| Etapa             | Plazo objetivo |
| ----------------- | -------------- |
| Acuse de recibo   | 72 horas       |
| Triage inicial    | 7 días         |
| Parche o mitigación | depende de severidad — crítico 7 días, alto 30 días |

Coordinaremos una fecha de divulgación pública contigo antes de publicar el
fix. Si reportas una vulnerabilidad válida y quieres crédito, te
mencionaremos en el advisory.

## Alcance

Está dentro de alcance cualquier código de este repositorio, incluyendo:

- Backend Rust (`backend/`)
- Frontend SvelteKit (`frontend/`)
- SDKs publicados (`sdks/*`)
- Workflows de CI/CD (`.github/workflows/`)
- Manifiestos de despliegue (`docker-compose*.yml`, `infra/`)

**Fuera de alcance:**

- La instancia operada en `faro.iaportafolio.com` — esa es una despliegue
  específico, no el producto. Reportes de configuración de esa instancia
  van por el mismo canal pero no califican como vulnerabilidad del proyecto.
- Vulnerabilidades en dependencias upstream (ClickHouse, Rust crates, etc.)
  — reportalas al proyecto correspondiente. Si nos afectan transitivamente,
  abriremos nuestro propio advisory enlazando al upstream.

## Recordatorio operacional

### Autenticación del dashboard

Faro tiene **auth nativa** desde 2026-05 (ver [ADR-0009](docs/adr/0009-security-hardening.md)).
El bootstrap se hace vía variables de entorno:

```bash
FARO_BOOTSTRAP_ADMIN_EMAIL=admin@example.com
FARO_BOOTSTRAP_ADMIN_PASSWORD=...         # opcional; si no se setea se genera uno y se loguea
FARO_BOOTSTRAP_ADMIN_NAME=Admin
```

Sin estas variables y sin users existentes, el dashboard queda no-loginable
(falla cerrado, no abierto).

**2FA TOTP opcional**: cada admin puede activarlo desde `/settings/security`.
Recomendado para cuentas con poder de crear users y rotar tokens.

### Aislamiento multi-tenant

Faro sigue siendo **single-tenant por instancia**. Los proyectos agrupan
datos lógicamente, pero un user del dashboard ve todos los proyectos. Para
separación dura, desplegá una instancia por tenant.

### Defense-in-depth recomendado en producción

| Capa | Recomendación |
|------|---------------|
| TLS | Reverse proxy (Caddy, nginx, Cloudflare) con HTTPS válido. Encender `FARO_ENABLE_HSTS=true`. |
| Auth | El admin bootstrap + 2FA TOTP cubre el dashboard. Para acceso REST programático, considerar un proxy de auth adicional. |
| Ingest token | Rotar periódicamente desde `/settings/projects/:slug`. El RUM/browser SDK requiere [allowlist de orígenes](docs/adr/0009-security-hardening.md#para-origin-check-cambio-6). |
| Datos sensibles | Activar [PII redaction](docs/adr/0009-security-hardening.md#para-pii-redaction-cambio-5) por proyecto. Aplica antes de escribir en ClickHouse — un dump comprometido no expone secretos. |
| CSP | Activa por default en el router del dashboard. Bloquea XSS reflejado + beaconing de exfil. |

Para el detalle completo y las decisiones, ver [ADR-0009](docs/adr/0009-security-hardening.md).
