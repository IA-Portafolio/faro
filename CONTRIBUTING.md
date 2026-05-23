# Contribuir a Faro

Faro es un proyecto propietario de IA Portafolio (ver [LICENSE](LICENSE)).
Las contribuciones externas se aceptan caso por caso y requieren acuerdo
previo. Esta guía documenta el flujo interno.

## Antes de empezar

1. Lee el [README](README.md) para entender qué hace Faro.
2. Lee [docs/development.md](docs/development.md) para levantar el stack
   localmente.
3. Para cambios no triviales (>200 líneas o que afecten el esquema de
   ClickHouse, la API REST pública o el contrato OTLP) abre un issue
   antes de codear, para validar la dirección.

## Flujo de trabajo

```bash
git checkout -b <tipo>/<descripcion-corta>     # feat/, fix/, docs/, chore/, refactor/
# trabajar
cargo fmt && cargo clippy -- -D warnings && cargo test      # si tocaste backend
cd frontend && npm run check && npm run build               # si tocaste frontend
git commit -m "tipo: resumen en imperativo y minúsculas"
git push -u origin <branch>
gh pr create --fill
```

CI (`ci.yml`) corre fmt + clippy + test del backend, typecheck + build del
frontend y validación de `docker-compose`. Un PR no se mergea hasta que
todo eso pasa.

## Convenciones

### Commits

Estilo [Conventional Commits](https://www.conventionalcommits.org/es/v1.0.0/)
suelto. Tipos usados:

- `feat:` nueva capacidad visible para el usuario / API nueva.
- `fix:` bug fix observable.
- `refactor:` cambio interno sin alterar comportamiento.
- `docs:` solo documentación.
- `chore:` tooling, deps, CI.
- `perf:` mejora de performance.

Mantén el resumen ≤ 72 caracteres y en minúsculas. El cuerpo (opcional)
explica el _porqué_, no el _qué_.

### Código

- **Rust**: `cargo fmt` define el estilo, `clippy` con `-D warnings` es
  obligatorio. Prefiere `?` sobre `unwrap()` en código de runtime.
- **TypeScript / Svelte**: el `tsconfig.json` y `svelte-check` son ley.
  Sin `any` salvo en límites con APIs externas no tipadas.
- **SQL ClickHouse**: migraciones idempotentes (`IF NOT EXISTS`). Las que
  no lo sean serán rechazadas — `deploy.yml` las re-ejecuta en cada push.

### Tamaño de PR

Mantén los PRs pequeños y enfocados. Si tu cambio toca más de tres
subsistemas (backend, frontend, schema, infra, SDKs), divídelo. La
excepción son los renames mecánicos.

## SDKs

Cada SDK es independiente. Para publicar:

```bash
git tag sdk-<lang>-v<semver>
git push origin sdk-<lang>-v<semver>
```

El workflow `publish-sdks.yml` empuja el paquete al registry y crea una
release en GitHub. Bumpea la versión en el `CHANGELOG.md` del SDK (si
existe) en el mismo commit que el tag.

## Reportar bugs y vulnerabilidades

- **Bugs normales**: abre un issue con los pasos para reproducir.
- **Vulnerabilidades**: NO abrir issue público. Sigue [SECURITY.md](SECURITY.md).

## Contacto

Mantenedor: [@victalejo](https://github.com/victalejo) — victoralejocj@gmail.com.
