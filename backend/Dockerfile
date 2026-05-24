# syntax=docker/dockerfile:1.6
# Build con cargo-chef: las deps quedan en una layer cacheada separada del código.
# Cambios en src/ NO invalidan la layer de deps; sólo cambios en Cargo.toml o
# Cargo.lock fuerzan recompilar terceros. Cold build: ~10 min (igual que antes
# — hay que compilar las deps al menos una vez). Rebuild iterativo: ~30-60 s.
#
# Cómo funciona: el stage `planner` genera un `recipe.json` determinístico sobre
# Cargo.toml + Cargo.lock. Aunque cambies un .rs y el planner re-corra, el
# recipe.json es bitwise idéntico → la layer del `cook` siguiente sigue siendo
# cache-hit en BuildKit. Si tocás Cargo.lock (bump de dep, dep nueva), recipe.json
# cambia y el cook se vuelve a ejecutar — esperado.

# IMPORTANTE: el tag `-bookworm` (Debian 12, glibc 2.36) tiene que coincidir
# con el runtime (`debian:bookworm-slim`). Con `-slim` Docker resuelve a
# `rust:1-slim` que ahora es Trixie (Debian 13, glibc 2.41) y el binario falla
# al boot en Bookworm con `GLIBC_2.38 not found`.
FROM lukemathwalker/cargo-chef:latest-rust-1-bookworm AS chef
WORKDIR /app
# Deps de sistema para el build:
# - pkg-config + ca-certificates: crates que linkean libs nativas
# (Antes había `curl` aquí para que utoipa-swagger-ui descargara el bundle en
#  su build.rs. Reemplazamos Swagger UI por Scalar — HTML estático desde CDN —
#  y ya no hace falta nada externo en el build.)
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config ca-certificates && rm -rf /var/lib/apt/lists/*

# Stage 1: planner. Observa el manifiesto y emite recipe.json.
FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 2: builder. Cocina las deps según el recipe (esto es la layer cara que
# queremos cachear) y después compila sólo el bin del crate con el código real.
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release --bin faro

# Stage 3: runtime mínimo.
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/faro /usr/local/bin/faro
ENV RUST_LOG=info,faro=debug
EXPOSE 8080 4318 4317
ENTRYPOINT ["/usr/local/bin/faro"]
