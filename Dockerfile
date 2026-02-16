# syntax=docker/dockerfile:1

# ── Stage 1: Build ────────────────────────────────────────────
FROM rust:1.93-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

# 1. Copy manifests to cache dependencies
COPY Cargo.toml Cargo.lock ./
# Create dummy main.rs to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release --locked
RUN rm -rf src

# 2. Copy source code
COPY . .
# Touch main.rs to force rebuild
RUN touch src/main.rs
RUN cargo build --release --locked && \
    strip target/release/asteroniris

# ── Stage 2: Permissions & Config Prep ───────────────────────
FROM busybox:latest AS permissions
# Create directory structure (simplified workspace path)
RUN mkdir -p /asteroniris-data/.asteroniris /asteroniris-data/workspace

# Create minimal config for PRODUCTION (allows binding to public interfaces)
# NOTE: Provider configuration must be done via environment variables at runtime
RUN cat > /asteroniris-data/.asteroniris/config.toml << 'EOF'
workspace_dir = "/asteroniris-data/workspace"
config_path = "/asteroniris-data/.asteroniris/config.toml"
api_key = ""
default_provider = "openrouter"
default_model = "anthropic/claude-sonnet-4-20250514"
default_temperature = 0.7

[gateway]
port = 3000
host = "[::]"
allow_public_bind = true
EOF

RUN chown -R 65534:65534 /asteroniris-data

# ── Stage 3: Development Runtime (Debian) ────────────────────
FROM debian:bookworm-slim AS dev

# Install runtime dependencies + basic debug tools
RUN apt-get update && apt-get install -y \
    ca-certificates \
    openssl \
    curl \
    git \
    iputils-ping \
    vim \
    && rm -rf /var/lib/apt/lists/*

COPY --from=permissions /asteroniris-data /asteroniris-data
COPY --from=builder /app/target/release/asteroniris /usr/local/bin/asteroniris

# Overwrite minimal config with DEV template (Ollama defaults)
COPY dev/config.template.toml /asteroniris-data/.asteroniris/config.toml
RUN chown 65534:65534 /asteroniris-data/.asteroniris/config.toml

# Environment setup
# Use consistent workspace path
ENV ASTERONIRIS_WORKSPACE=/asteroniris-data/workspace
ENV HOME=/asteroniris-data
# Defaults for local dev (Ollama) - matches config.template.toml
ENV PROVIDER="ollama"
ENV ASTERONIRIS_MODEL="llama3.2"
ENV ASTERONIRIS_GATEWAY_PORT=3000

# Note: API_KEY is intentionally NOT set here to avoid confusion.
# It is set in config.toml as the Ollama URL.

WORKDIR /asteroniris-data
USER 65534:65534
EXPOSE 3000
ENTRYPOINT ["asteroniris"]
CMD ["gateway", "--port", "3000", "--host", "[::]"]

# ── Stage 4: Production Runtime (Distroless) ─────────────────
FROM gcr.io/distroless/cc-debian12:nonroot AS release

COPY --from=builder /app/target/release/asteroniris /usr/local/bin/asteroniris
COPY --from=permissions /asteroniris-data /asteroniris-data

# Environment setup
ENV ASTERONIRIS_WORKSPACE=/asteroniris-data/workspace
ENV HOME=/asteroniris-data
# Defaults for prod (OpenRouter)
ENV PROVIDER="openrouter"
ENV ASTERONIRIS_MODEL="anthropic/claude-sonnet-4-20250514"
ENV ASTERONIRIS_GATEWAY_PORT=3000

# API_KEY must be provided at runtime!

WORKDIR /asteroniris-data
USER 65534:65534
EXPOSE 3000
ENTRYPOINT ["asteroniris"]
CMD ["gateway", "--port", "3000", "--host", "[::]"]
