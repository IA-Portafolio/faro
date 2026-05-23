# ADR-0005: Sin autenticación nativa en el dashboard

- **Estado**: Accepted
- **Fecha**: 2026-05-23
- **Autores**: @victalejo

## Contexto

Faro tiene dos superficies HTTP visibles:

1. **Ingesta** (`/api/v1/ingest/logs` y `:4318` OTLP) — necesita
   autenticación porque cualquier internet rando podría llenar la DB
   con basura.
2. **Dashboard + API REST de lectura** (`/`, `/api/v1/logs`, etc.) —
   puede contener datos sensibles dependiendo de qué emiten los
   clientes.

Para (1) usamos un token Bearer estático (`FARO_INGEST_TOKEN`). Para
(2) tenemos que decidir: implementar auth propia (signup, sesiones,
RBAC) o delegar a la capa de red.

## Decisión

Faro **no implementa autenticación propia para el dashboard**.
Cualquiera con acceso de red al puerto `:3000` o a la API REST `:8080`
puede leer todo. La autenticación es responsabilidad operacional del
deploy: poner un proxy de auth (Cloudflare Access, Authelia,
oauth2-proxy, Tailscale serve, etc.) delante.

Documentado explícitamente en README → Limitaciones y reforzado en
`SECURITY.md`.

## Alternativas consideradas

- **Auth propia (sessions cookie-based)** — N usuarios, recovery de
  passwords, lockout, MFA, RBAC. Es un subproyecto entero. Mantener
  esa superficie es responsabilidad seria (CVEs frecuentes en
  bibliotecas de auth, complejidad de tokens, etc.).
- **OAuth con un solo IdP** (Google, GitHub) — más barato que rodar
  auth propia, pero acopla a un proveedor y aún requiere implementar
  sesiones, autorización, callback, refresh.
- **SAML/SSO** — overkill para self-hosted typical.
- **HTTP Basic Auth en el backend** — quema en el agua y deja un
  mecanismo de auth débil en el path crítico.

## Consecuencias

### Positivas
- Cero código de auth que mantener, parchar y testear.
- Operador elige el mecanismo de auth apropiado para su red
  (Cloudflare Access es muy distinto a Tailscale).
- Reduce la superficie de ataque del propio servicio.

### Negativas / costo asumido
- **Reportes de "el dashboard no requiere login" se cierran como
  working-as-designed** (ver `SECURITY.md`).
- Multi-tenant es imposible sin auth — pero Faro no busca ser
  multi-tenant. Cada equipo despliega su propia instancia.
- Onboarding requiere documentar bien la "primera milla" de poner
  un proxy delante; un usuario que despliegue y exponga directamente
  a internet tiene una mala experiencia.

### Trabajo de seguimiento
- Mantener `SECURITY.md` y `docs/deployment.md` claros sobre la
  necesidad del proxy.
- Considerar un middleware opcional `FARO_DASHBOARD_TOKEN` (Bearer
  estático para la API REST de lectura) si aparecen muchos usuarios
  reportando lo mismo. Es 30 líneas de código; el riesgo es darle
  falsa sensación de seguridad — un token estático no es auth real.
