# Infraestructura · CI/CD de Faro

> **Por qué corre todo en self-hosted**: el plan GitHub Actions del repo
> tiene presupuesto cero. Tanto `deploy.yml` como el resto (`ci.yml`,
> `codeql.yml`, `docs.yml`, `publish-sdks.yml`, `release-images.yml`,
> `sdk-tests.yml`, `validate-tag.yml`) usan el mismo runner en
> `infra-iaportafolio`, distinguidos por label:
>
> - `faro-deploy` — exclusivo del workflow `deploy.yml` (concurrency=1).
> - `faro-ci` — el resto. Comparten un único agente runner, los jobs
>   se serializan si llegan en paralelo.

## 1 · Self-hosted runner

El workflow `deploy.yml` y el resto del CI se ejecutan dentro de
`infra-iaportafolio`. Sirve para evitar abrir SSH público (deploy) y
para no pagar minutos de GitHub Actions (CI). Todo el trabajo lo hace
el runner localmente.

### Instalación nueva (primera vez)

1. En GitHub abre **<https://github.com/IA-Portafolio/faro/settings/actions/runners/new>**. Elige Linux x64. Verás un token estilo `AABBCC...` (válido ~1 hora).
2. Copia el token y entra al server:

   ```bash
   ssh infra-iaportafolio
   REG_TOKEN="AABBCC..." bash /opt/faro/infra/runner-install.sh
   ```

   El script:
   - Crea `/opt/actions-runner/`
   - Descarga el runner v2.319.x
   - Lo registra en el repo con labels `self-hosted,linux,faro-deploy,faro-ci`
   - Añade el user `victalejo` al grupo `docker` si falta
   - Lo instala como servicio systemd y lo arranca

3. Verifica en **Settings → Actions → Runners** que aparece `faro-infra` en estado **Idle (online)** con los 4 labels.

### Migración: agregar `faro-ci` al runner ya existente

El runner que ya tenés sólo trae `self-hosted,linux,faro-deploy`. Para
que también acepte jobs de CI hay que sumarle `faro-ci`. Tres caminos:

**A) Vía GitHub UI (más rápido)**:
   Settings → Actions → Runners → click `faro-infra` → botón
   **Edit labels** → agregar `faro-ci` → Save. El runner sigue
   corriendo, no requiere reinicio.

**B) Vía REST API** (si preferís CLI desde tu workstation):

   ```bash
   gh api -X POST /repos/IA-Portafolio/faro/actions/runners/<RUNNER_ID>/labels \
     -f labels[]=faro-ci
   ```

   `RUNNER_ID` lo sacás con
   `gh api /repos/IA-Portafolio/faro/actions/runners | jq '.runners[] | {id,name,labels}'`.

**C) Re-registrar** (último recurso — pierde el ID actual):

   ```bash
   ssh infra-iaportafolio
   sudo /opt/actions-runner/svc.sh stop
   cd /opt/actions-runner && sudo -u victalejo ./config.sh remove --token "<remove-token>"
   REG_TOKEN="<token nuevo>" bash /opt/faro/infra/runner-install.sh
   ```

   El script ya viene con `RUNNER_LABELS` incluyendo `faro-ci`.

Tras cualquiera de las 3, el próximo push a `main` debería disparar
los workflows que estaban bloqueados por el billing.

### Pre-requisitos del runner para `faro-ci`

CI necesita más herramientas que deploy. Verificá que el server tenga:

| Herramienta | Para qué | Cómo instalar |
| --- | --- | --- |
| `docker` + `docker compose` | services: clickhouse en ci.yml, build de imágenes | ya está (corre Faro de producción) |
| `git`, `bash`, `curl`, `wget`, `tar`, `jq` | utilidades comunes de los workflows | `apt-get install -y git curl wget jq` |
| `build-essential`, `pkg-config`, `libssl-dev` | compilación de crates Rust con bindings C | `apt-get install -y build-essential pkg-config libssl-dev` |
| Rust toolchain | `dtolnay/rust-toolchain` lo descarga por job; pero rustup pre-instalado acelera | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh -s -- -y` |
| Node.js + npm | `actions/setup-node` lo descarga; **NO** instalar globalmente para no chocar con la versión del repo (`.nvmrc`) | nada |
| Python, Go, Java/Gradle | `actions/setup-*` los descargan en `_work/_tool/` por job | nada |

El runner descarga binarios a `/opt/actions-runner/_work/_tool/` y los
cachea entre jobs — espacio de disco a tener en cuenta (~5-10 GB tras
varias corridas). Para limpiar: `rm -rf /opt/actions-runner/_work/_tool/*`.

### Colisiones de puerto a tener en cuenta

El job `backend` y `migrations` levantan ClickHouse como `services:`.
Por defecto GitHub mapea los puertos del container al host del runner.
**El host tiene Faro de producción usando `8123/9000`** — esos workflows
ya están reconfigurados para usar `18123/19000` en el host (ver
`ports: ['18123:8123', '19000:9000']` en `ci.yml`). Si en el futuro
agregás otro service que use puertos hardcoded, repetí el patrón
(usá `1XXXX:XXXX` para no chocar con prod).

### Operación

| Operación              | Comando                                    |
| ---------------------- | ------------------------------------------ |
| Estado                 | `sudo /opt/actions-runner/svc.sh status`   |
| Detener                | `sudo /opt/actions-runner/svc.sh stop`     |
| Iniciar                | `sudo /opt/actions-runner/svc.sh start`    |
| Registros              | `journalctl -u 'actions.runner.*' -f`      |
| Re-registrar (token nuevo)| `sudo /opt/actions-runner/svc.sh uninstall && REG_TOKEN=... bash infra/runner-install.sh` |

### Variables de entorno del host

El runner ejecuta `docker compose -f docker-compose.prod.yml --env-file .env.prod up -d --build` dentro de `/opt/faro/`. El `.env.prod` se construye a partir de [`.env.prod.template`](../.env.prod.template) y la [referencia completa de variables](../docs/reference/environment.md) (autogenerada desde `.env.example`). No duplicamos la tabla acá — la fuente única es esa página.

### Qué hace el workflow `deploy.yml`

En cada push a `main`:

1. `rsync` del checkout hacia `/opt/faro/` (preserva `.env.prod`, backups, volúmenes)
2. Aplica todas las migraciones idempotentes de `clickhouse/migrations/*.sql`
3. `docker compose build` (re-aprovecha cache de capas)
4. `docker compose up -d --remove-orphans`
5. `GET https://faro.iaportafolio.com/healthz` hasta 40 reintentos
6. `docker image prune -f` (limpia capas viejas)

`concurrency: deploy-faro` impide que dos deploys se pisen.

## 2 · Publicación de SDKs

Tags con formato `sdk-<lenguaje>-v<semver>` disparan publicación en el registry correspondiente:

| Tag                        | Registry        | Paquete                    |
| -------------------------- | --------------- | -------------------------- |
| `sdk-node-v0.1.0`          | npm             | `@iaportafolio/node`       |
| `sdk-nextjs-v0.3.0`        | npm             | `@iaportafolio/nextjs` (incluye el RUM del navegador desde v0.3.0) |
| `sdk-expo-v0.1.0`          | npm             | `@iaportafolio/expo`       |
| `sdk-python-v0.1.0`        | PyPI            | `faro-sdk`                 |
| `sdk-go-v0.1.0`            | Go modules (git)| `github.com/IA-Portafolio/faro/sdks/go` |
| `sdk-flutter-v0.1.0`       | pub.dev         | `faro_sdk`                 |
| `sdk-kotlin-v0.1.0`        | Maven Central   | `com.iaportafolio:faro`    |

> `@iaportafolio/browser` existió entre v0.2.0 y v0.2.2 como paquete
> separado y fue fusionado en `@iaportafolio/nextjs@0.3.0`. Ahora está
> deprecado y unpublished en npm — no relanzar bajo ese nombre.

Lanzar un tag:

```bash
git tag sdk-node-v0.1.0
git push origin sdk-node-v0.1.0
```

El workflow `publish-sdks.yml` parsea el tag, bumpea la versión en el manifiesto y publica. Solo se ejecuta el job correspondiente al prefijo del tag.

### Secrets de GitHub que necesitas configurar

En **Settings → Secrets and variables → Actions**:

| Secret                      | Para qué                                  | Cómo obtenerlo |
| --------------------------- | ----------------------------------------- | -------------- |
| `NPM_TOKEN`                 | Publicar a npm                            | npmjs.com → Profile → Access Tokens → Generate **Granular** token con permiso `Read and write` sobre la organización `@iaportafolio`. La org ya existe — no recrearla. |
| `PYPI_API_TOKEN` *(o configurar Trusted Publishing — recomendado)* | Publicar a PyPI | pypi.org → Account settings → API tokens. **Trusted publishing** (sin secret) es preferible: registra el proyecto en pypi.org/manage/account/publishing/ apuntando a este repo + workflow. |
| `PUB_CREDENTIALS_JSON`      | Publicar a pub.dev                        | Localmente: `dart pub token add https://pub.dev` → autoriza con Google → copia el contenido de `~/.config/dart/pub-credentials.json` |
| `OSSRH_USERNAME`            | Maven Central                             | Tu username en <https://central.sonatype.com/> (o legacy s01.oss.sonatype.org) |
| `OSSRH_PASSWORD`            | Maven Central                             | Token generado en <https://central.sonatype.com/account> |
| `MAVEN_GPG_PRIVATE_KEY`     | Firmar artefactos Maven                   | `gpg --armor --export-secret-keys <KEY_ID>` (ver más abajo) |
| `MAVEN_GPG_PASSPHRASE`      | Passphrase de la clave GPG                | La que pusiste al generar la clave |

### Setup inicial de cada registry

#### npm (@iaportafolio/node, @iaportafolio/nextjs, @iaportafolio/expo)

1. Crea cuenta en npmjs.com si no tienes.
2. La **organización** `iaportafolio` ya está creada en npm. (El scope `@faro` se descartó en su momento porque estaba tomado.)
3. **Profile → Access Tokens → Generate New Token** → tipo **Granular Access Token**. Scope: `Packages and scopes` con permiso `Read and write` para `@iaportafolio/*`. Expiración: 1 año (renovable).
4. Pega el token en `NPM_TOKEN` del repo.

#### PyPI (faro-sdk)

**Opción A — Trusted publishing (sin secret, recomendada)**:

1. Sube manualmente el primer release con tu cuenta (para reclamar el nombre `faro-sdk`):

   ```bash
   cd sdks/python && python -m build && twine upload dist/*
   ```

2. En <https://pypi.org/manage/project/faro-sdk/settings/publishing/> añade un **trusted publisher**:
   - Owner: `IA-Portafolio`
   - Repo: `faro`
   - Workflow: `publish-sdks.yml`
   - Environment: `pypi`
3. Ya no necesitas `PYPI_API_TOKEN` — los siguientes releases se autentican vía OIDC.

**Opción B — Token clásico**: pypi.org → Account settings → API tokens → Add. Pégalo como `PYPI_API_TOKEN` y descomenta esa rama en el workflow.

#### pub.dev (faro_sdk)

1. Localmente: `dart pub token add https://pub.dev` y autoriza con la cuenta Google con la que vas a publicar.
2. Lee el JSON: `cat ~/.config/dart/pub-credentials.json`.
3. Pégalo entero (multi-línea) como `PUB_CREDENTIALS_JSON`.

#### Maven Central (com.iaportafolio:faro) — el más engorroso

1. **Registra el namespace** `com.iaportafolio` en <https://central.sonatype.com/>. Requiere demostrar control del dominio iaportafolio.com (vía DNS TXT record).
2. **Genera una par de claves GPG** y publícala:

   ```bash
   gpg --generate-key                       # nombre real, email del proyecto
   gpg --list-secret-keys --keyid-format=long
   gpg --keyserver keyserver.ubuntu.com --send-keys <KEY_ID>
   gpg --keyserver keys.openpgp.org --send-keys <KEY_ID>
   ```

3. Exporta la clave privada con armor:

   ```bash
   gpg --armor --export-secret-keys <KEY_ID> > maven-gpg.key
   ```

4. Pega el contenido completo (incluyendo `-----BEGIN PGP PRIVATE KEY BLOCK-----`) como `MAVEN_GPG_PRIVATE_KEY`. Pega la passphrase como `MAVEN_GPG_PASSPHRASE`.
5. En el portal de Central genera un **user token** (Settings → User Token → Generate). Username = primera cadena, password = segunda. Pégalos como `OSSRH_USERNAME` y `OSSRH_PASSWORD`.

#### Go modules

Sin registry — los tags `sdks/go/v<ver>` que crea el workflow ya hacen disponible el módulo en `proxy.golang.org`. El nombre de import es `github.com/IA-Portafolio/faro/sdks/go`.

## 3 · Primera publicación end-to-end

Una vez los secrets están configurados:

```bash
# Lanza los 7 SDKs a sus registries
for sdk in node nextjs expo python go flutter kotlin; do
    git tag "sdk-$sdk-v0.1.0"
done
git push origin --tags
```

Mira el progreso en **Actions → publish-sdks** del repo. Cada tag dispara su job en paralelo.

## 4 · Rollback / hot fixes

- Si el deploy a `main` rompe algo, revierte el commit y empuja: el workflow se ejecuta otra vez.
- Si necesitas detener el deploy mientras investigas: `sudo /opt/actions-runner/svc.sh stop` en el server.
- Si un SDK queda mal publicado: npm/PyPI permiten *yank* (no delete), pub.dev permite *retract* en 7 días, Maven Central no permite delete (sube parche `v0.1.1`).
