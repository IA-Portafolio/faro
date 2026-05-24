# ADR-0009: Endurecimiento de seguridad del backend

- **Estado**: Accepted
- **Fecha**: 2026-05-24
- **Autores**: @victalejo
- **Reemplaza**: parcialmente a [ADR-0005](0005-no-native-auth.md) en lo relativo a auth del dashboard.

## Contexto

ADR-0005 (mayo 2026) declaraba "sin auth nativa", delegando la
autenticación del dashboard a un proxy externo (Cloudflare Access,
Tailscale, oauth2-proxy). Esa decisión era coherente cuando Faro era
**solo** un colector de telemetría server-side: el peor caso era
"alguien con acceso a la red ve métricas". Hoy:

1. **El RUM/browser SDK existe** (`@iaportafolio/nextjs` para apps
   Next.js). El token de ingesta para browser **es público** — viaja en
   el bundle JS que sirve el cliente. Cualquiera puede sniffearlo y
   mandar eventos falsos.
2. **Los logs cargan PII** (emails, JWTs, IPs, headers `Authorization`)
   que las apps loguean por accidente. Una vez en ClickHouse quedan
   indefinidamente — no hay TTL agresivo ni una capa de redaction.
3. **El dashboard sirve a usuarios humanos autenticados**: el setup
   "proxy delante" sigue siendo válido para muchos casos, pero un
   atacante con XSS reflejado en el body de un log podría exfiltrar la
   cookie del proxy si no hay CSP. Defense-in-depth.
4. **Cualquier cambio al backend pasa por queries con input de usuario**.
   `escape_sql()` artesanal cubría `\` y `'` pero no comentarios SQL,
   nombres de columna con backticks, ni los corner cases del parser de
   ClickHouse.

Un audit interno (PRs 3.1–3.10 de mayo) identificó los gaps. Esta ADR
captura las decisiones arquitectónicas que salieron de ese trabajo.

## Decisión

Adoptamos **siete cambios coordinados**, todos opt-in donde tiene sentido
y backward-compatible para instancias preexistentes:

| # | Capa | Cambio | Default |
|---|------|--------|---------|
| 1 | DB | Parametrización server-side de **todos** los queries con user input (sintaxis `{name:Type}` de ClickHouse). `escape_sql()` eliminado. | Siempre on |
| 2 | HTTP | CSP estricto + HSTS + X-Frame-Options + X-Content-Type-Options + Referrer-Policy sobre el router del **dashboard** (no sobre ingest). | CSP/X-Frame/Referrer on; HSTS gated por `FARO_ENABLE_HSTS=true` |
| 3 | Auth | **Auth nativa** con email + password (Argon2id), cookie HttpOnly + Secure + SameSite=Lax, sesiones revocables. | On si hay user creado (bootstrap vía `FARO_BOOTSTRAP_ADMIN_*`) |
| 4 | Auth | **2FA TOTP** (RFC 6238) opcional con 10 recovery codes one-shot, rate limit 5 intentos/min/user. | Off; opt-in por usuario desde `/settings/security` |
| 5 | Ingest | **PII redaction** server-side configurable por proyecto (7 built-ins + custom regex). Aplica al `body` de logs y valores de atributos antes de persistir. | Off; opt-in por proyecto desde `/settings/projects/:slug/redaction` |
| 6 | Ingest | **Origin allowlist** por proyecto: si está activa y el request trae `Origin`, debe matchear. Wildcard de un solo nivel de subdominio. | Off (fail-open); opt-in por proyecto desde `/settings/projects/:slug/origins` |
| 7 | CI | `cargo audit --deny warnings` con ignores explícitos en el workflow. Cubre `vulnerability` + `unmaintained` + `yanked` + `unsound` + `notice`. | Siempre on |

## Alternativas consideradas

### Para auth nativa (cambio #3, donde supersedimos ADR-0005)

- **Mantener "sin auth nativa" + delegar todo al proxy** — opción de
  ADR-0005. Sigue siendo válida para muchos deploys, pero deja en el
  aire los gaps #4 (2FA) y #6 (origin check para RUM): un proxy de auth
  no sabe nada del modelo de proyectos/tokens internos de Faro. Tampoco
  resuelve la rotación de sesión post-password-change. La complejidad de
  agregar auth nativa **bien hecha** (Argon2id, sesiones hash-only,
  cookie HttpOnly + Secure) es ~500 líneas en `auth.rs`; mantenible.
- **OAuth con un solo IdP** — más barato que mantener users, pero acopla
  Faro a un proveedor y no resuelve el problema de RUM origin check
  porque ese chequeo es a nivel de proyecto, no de user.
- **HTTP Basic Auth** — descartado en ADR-0005, sigue descartado: no es
  rotable sin cambiar config, no soporta MFA, deja credentials en logs
  del proxy.

### Para CSP (cambio #2)

- **Aplicar a TODOS los endpoints (incluyendo ingest)** — bytes
  desperdiciados en respuestas a SDKs (~250 bytes/POST en CSP, miles de
  POSTs/segundo). Sin valor: los SDKs no son browsers.
- **CSP idéntico para dashboard y `/docs`** — la referencia API en
  `/docs` (Scalar; antes Swagger UI) carga su bundle desde
  `cdn.jsdelivr.net` y usa `<script>` inline con `data-url`. Un CSP
  `script-src 'self'` lo deja en blanco. Mantenemos dos políticas:
  estricta para JSON API, relajada (`'unsafe-inline'` + whitelist del CDN)
  solo para `/docs`. Ver `backend/src/api/security.rs::SCALAR_CSP`.

### Para 2FA (cambio #4)

- **2FA obligatorio por defecto** — rompe deploys existentes que ya
  tienen users sin enrolar. Lo dejamos opt-in por user; documentamos
  fuertemente que admins deberían enrolar.
- **SMS / Email como segundo factor** — caros y débiles (SIM swap,
  inbox compromise). TOTP es offline, gratis, y es lo que esperan los
  authenticators que ya tiene la gente.
- **WebAuthn / Passkeys** — mejor seguridad que TOTP pero requiere UX
  más compleja (recovery, sync entre dispositivos del mismo user) que
  está fuera del scope inicial. TOTP + recovery codes cubre el caso
  común con dependencias mínimas (`totp-rs` + `qrcode`).

### Para PII redaction (cambio #5)

- **Redaction en el SDK** — el SDK no sabe qué cuenta como PII para esta
  org. Política central > política distribuida.
- **Redaction en lectura (en el dashboard)** — los datos sensibles ya
  están en disco; si la DB se filtra, igual están comprometidos. La
  redaction en el path de ingest los borra antes de tocar disco.
- **Redaction con NLP / clasificadores** — overkill para v1; arranca con
  regex built-ins curados y custom rules por proyecto.

### Para origin check (cambio #6)

- **CORS en lugar de origin allowlist** — distintos problemas. CORS
  controla qué orígenes el browser **deja leer** la respuesta; nuestro
  problema es que cualquiera puede **escribir** con el token público.
  Hay que chequear `Origin` en el servidor.
- **Token rotativo firmado por la app** — más fuerte pero requiere que
  la app del cliente tenga un secreto server-side que firme tokens
  cortos para el browser. Mayor cambio en el contrato del SDK. La
  whitelist resuelve el 90% del caso con cero código del lado del SDK.
- **Wildcard arbitrario (`api.*.com`)** — habilitaría bypass por
  `api.evil.com`. Solo soportamos wildcard de un nivel de subdominio
  (`https://*.example.com`); cualquier otro patrón se rechaza al guardar.

### Para `cargo audit` (cambio #7)

- **Mantener `rustsec/audit-check` action** — sólo falla por CVE
  clásico. `unmaintained` / `yanked` / `unsound` pasan en silencio
  aunque la action los imprima. Justamente lo que queremos cazar.
- **`audit.toml` con `[advisories] ignore = [...]`** — la versión
  0.22+ de cargo-audit cambió el comportamiento y no lee el dotfile
  desde el cwd como las viejas. Usamos `--ignore` CLI en el workflow:
  el ignore queda en el diff de cada PR review, no escondido.

## Consecuencias

### Positivas

- **El dashboard ya no requiere proxy externo obligatorio** para
  proteger contra acceso anónimo (sigue siendo recomendado por
  defense-in-depth — TLS termination, WAF, IP allowlist).
- **PII no toca disco** cuando redaction está activa. Un dump
  comprometido de ClickHouse no expone secretos.
- **Token RUM filtrado deja de ser fatal**: aún sin allowlist el ataque
  está limitado (rate limit por proyecto + sólo puede mandar eventos,
  no leer), pero con allowlist queda neutralizado.
- **Tokens TOTP brute-force no son viables**: 5 intentos/min/user
  contra 10⁶ códigos.
- **CVE nuevas en deps no entran a producción**: el `cargo audit`
  estricto las atrapa en PR.
- **Cookie comprometida tiene blast radius limitado**:
  HttpOnly impide robo desde JS, CSP `connect-src 'self'` impide
  beaconing si hay XSS, rotación de sesión post-password-change la
  invalida.

### Negativas / costo asumido

- **Bootstrap de un deploy nuevo es más involucrado**: hay que setear
  `FARO_BOOTSTRAP_ADMIN_EMAIL` + `FARO_BOOTSTRAP_ADMIN_PASSWORD` (o
  copiar el password random que se genera). Documentado en README +
  `docs/deployment.md`.
- **Migraciones**: las tablas de auth (`user_sessions`,
  `user_recovery_codes`, `user_login_challenges`) y las columnas nuevas
  en `projects` (`redaction_rules`, `allowed_origins`) y `users`
  (`totp_secret`, `totp_enabled`) requieren correr
  `clickhouse/migrations/00{2,7,8,9}-*.sql`. Las migraciones son
  idempotentes (`ADD COLUMN IF NOT EXISTS`, `CREATE TABLE IF NOT
  EXISTS`).
- **HSTS apagado por default** evita lock-out en dev pero hay que
  acordarse de encenderlo en prod (`FARO_ENABLE_HSTS=true`).
- **Rate limit TOTP in-memory por proceso**: si Faro pasa a múltiples
  réplicas el limiter se vuelve permisivo (5 intentos × N réplicas).
  Hoy Faro corre como una sola réplica; cuando esto cambie, migrar a
  Redis con la misma interfaz.
- **`utoipa-axum 0.1.3` arrastra `paste` unmaintained** —
  RUSTSEC-2024-0436 está en la lista `--ignore` con tracking. Subir a
  `utoipa-axum 0.2.x` (bump major) cuando se haga el próximo barrido.

### Trabajo de seguimiento

- **ADR-0005**: marcada como `Superseded by ADR-0009` en lo relativo a
  la decisión de "sin auth nativa". El resto del contexto (proxy de
  red como defense-in-depth) sigue válido.
- **`SECURITY.md`**: actualizado eliminando la cláusula
  "working-as-designed" sobre el dashboard sin login, y agregando una
  sección sobre redaction / origin check como mitigaciones disponibles.
- **`docs/deployment.md`**: añadir checklist de hardening para
  producción (`FARO_ENABLE_HSTS=true`, activar redaction y origin
  check por proyecto, exigir 2FA a admins).
- **Subir `utoipa-axum` a 0.2.x** en el próximo PR de deps para
  eliminar el ignore de RUSTSEC-2024-0436.
- **Métrica `faro_auth_failures_total{reason}`** para detectar
  brute-force en aggregate (hoy solo se loguea por intento individual).
- **Tests de integración** que ejerciten el path completo de
  login-con-2FA contra ClickHouse real (los unit tests cubren los
  helpers; falta end-to-end).
