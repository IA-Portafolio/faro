# farocli

CLI para Faro. Tail de logs en vivo, queries rápidas, y crear/listar monitores desde la terminal — sin ir al dashboard.

Reusa los mismos endpoints HTTP que consume el dashboard, así que cualquier filtro disponible en la UI también está aquí (y al revés: lo que añadas a la API queda automáticamente accesible).

## Instalar

```bash
cd cli
cargo install --path .
# o, sin instalar:
cargo run --release -- logs --follow
```

El binario produce `farocli` en `~/.cargo/bin`.

## Setup

```bash
# Apunta a tu instancia. Si la omites, default a http://localhost:8080.
farocli --endpoint https://faro.iaportafolio.com login
# (te pregunta email/password — y código TOTP si lo tienes activo)

# A partir de aquí cada comando reusa la sesión.
farocli logs --follow
```

El `endpoint` y la cookie se guardan en `~/.config/farocli/config.json` (Linux/macOS) o `%APPDATA%/farocli/config.json` (Windows). `farocli logout` los borra.

## Comandos

### `farocli logs`

```bash
# Últimos 200 logs del último 1h (defaults), todos los proyectos:
farocli logs

# Stream en vivo, sólo errores del servicio `api` en el proyecto `acme`:
farocli logs -p acme --service api --severity ERROR --follow

# Búsqueda de substring, últimas 24h, devuelve JSON línea-a-línea para `jq`:
farocli logs --last 24h --query "payment failed" --json | jq .body

# Limita el batch (no aplica con --follow):
farocli logs --last 6h --limit 1000
```

Flags útiles:

| Flag | Default | Notas |
| --- | --- | --- |
| `--service`, `-s` | — | Filtra por `service_name` exacto |
| `--severity` | — | `DEBUG` / `INFO` / `WARN` / `ERROR` / `FATAL`. Filtra ≥ ese nivel. |
| `--query`, `-q` | — | Substring case-insensitive sobre `body` |
| `--last` | `1h` | `5m`, `30m`, `1h`, `6h`, `24h`, `7d` (ignorado con `--follow`) |
| `--follow`, `-f` | — | SSE en vivo. `Ctrl-C` para cortar. |
| `--json` | — | Emite el row crudo en JSONL para pipear a `jq` |
| `--limit` | `200` | Máximo de filas en modo no-follow |

Output por defecto colorea por severidad. Respeta `NO_COLOR=1` o pipes (no-TTY).

### `farocli services`

Listado del último 1h con conteos de logs y errores. Útil para confirmar que un servicio está reportando antes de bucear en logs.

```bash
farocli -p acme services
```

### `farocli errors`

Issues agrupados por fingerprint, últimos 24h.

```bash
farocli -p acme errors                # todos
farocli -p acme errors --status unresolved
```

### `farocli monitor`

```bash
# Listar:
farocli -p acme monitor list

# Crear (defaults: GET, interval 60s, timeout 30s, espera 2xx):
farocli -p acme monitor create --name "API health" --url https://api.acme.com/health

# Con override:
farocli -p acme monitor create \
    --name "Slow endpoint" \
    --url https://api.acme.com/slow \
    --method POST \
    --interval 120 \
    --timeout 10
```

Las actualizaciones y borrados van por el dashboard — no se cubren acá porque rara vez son acción "de terminal".

## Globals

Cualquier comando acepta:

| Flag | Env var | Notas |
| --- | --- | --- |
| `--endpoint <URL>` | `FARO_ENDPOINT` | Sobreescribe el endpoint guardado |
| `--project, -p <SLUG>` | `FARO_PROJECT` | Filtra por proyecto |

Útil para tener varios "perfiles" sin reloguearte:

```bash
FARO_ENDPOINT=https://faro.staging.acme.com farocli logs -p acme --follow
```

## Diseño

- ~500 líneas de Rust en un único archivo. Cero capas innecesarias.
- Auth = cookie de sesión `faro_session`, la misma que el dashboard. Sin tokens dedicados, sin OAuth, sin scopes — si puedes leer en el dashboard, puedes leer aquí.
- Cookie se persiste plain en el config. El servidor la marca `HttpOnly`, pero eso solo restringe a browsers — un cliente HTTP la puede leer del `Set-Cookie` y reusarla. **Trátalo como un secreto del nivel de tu `~/.ssh/id_rsa`** — si compartes la máquina, la sesión queda accesible para otros usuarios locales con permisos.
- `--follow` consume el SSE `/api/v1/logs/live` con un parser minimalista de la spec (mensajes separados por `\n\n`, `data:` extraído por línea). Sin librería de SSE — innecesario para 30 líneas.
- Mismos endpoints que el dashboard, así que filtros y semántica son los mismos. Si una flag no aparece acá, está vivo en `/frontend/src/lib/api.ts` y es trivial añadirlo.

## Tests

```bash
cargo test
```

Cubre parseo de duración (`5m`/`1h`/`7d`), severidad (alias `WARNING`, etc.) y url-encoding.
