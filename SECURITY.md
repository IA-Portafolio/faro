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

Faro no implementa autenticación en la API del dashboard ni aislamiento
multi-tenant (es intencional, ver README → "Limitaciones"). **No expongas
una instancia directamente a internet sin un proxy de autenticación delante.**
Cualquier reporte basado en "el dashboard es accesible sin login" será
cerrado como _working as designed_.
