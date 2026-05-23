# Etapa de build
FROM rust:1-slim AS builder
WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config ca-certificates && rm -rf /var/lib/apt/lists/*

# Cachea dependencias
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && \
    cargo build --release && rm -rf src target/release/deps/faro*

COPY src ./src
RUN cargo build --release

# Etapa de runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates wget && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/faro /usr/local/bin/faro
ENV RUST_LOG=info,faro=debug
EXPOSE 8080 4318
ENTRYPOINT ["/usr/local/bin/faro"]
