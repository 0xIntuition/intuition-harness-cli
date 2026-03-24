# ---------------------------------------------------------------------------
# Stage 1: Build the static musl binary
# ---------------------------------------------------------------------------
FROM rust:1.85-bookworm AS builder

RUN apt-get update && apt-get install -y musl-tools && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src/ src/

RUN cargo build --release --target x86_64-unknown-linux-musl --locked \
    && strip /build/target/x86_64-unknown-linux-musl/release/meta

# ---------------------------------------------------------------------------
# Stage 2: Minimal runtime image
# ---------------------------------------------------------------------------
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

RUN groupadd --gid 1000 meta \
    && useradd --uid 1000 --gid meta --create-home meta

COPY --from=builder /build/target/x86_64-unknown-linux-musl/release/meta /usr/local/bin/meta

USER meta
WORKDIR /home/meta

# Cloud Run injects PORT; the service must listen on it.
ENV PORT=8080
EXPOSE 8080

ENTRYPOINT ["meta"]
