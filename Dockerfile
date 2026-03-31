# ------------------------------------------------------------------------------
# 1. Chef Stage
# WE USE 'rust:1-bookworm' to ensure we are on Debian 12
# ------------------------------------------------------------------------------
FROM rust:1-bookworm as chef
RUN cargo install cargo-chef
WORKDIR /app

# ------------------------------------------------------------------------------
# 2. Planner Stage
# ------------------------------------------------------------------------------
FROM chef as planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# ------------------------------------------------------------------------------
# 3. Builder Stage
# ------------------------------------------------------------------------------
FROM chef as builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release --bin eidolon-node

# ------------------------------------------------------------------------------
# 4. Runtime Stage
# WE USE 'debian:bookworm-slim' to match the builder OS exactly
# ------------------------------------------------------------------------------
FROM debian:bookworm-slim
WORKDIR /app

# Install SSL certificates (Critical for HTTPS RPCs)
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates openssl curl \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user
RUN groupadd -r eidolon && useradd -r -g eidolon -s /bin/false eidolon

# Copy the binary
COPY --from=builder /app/target/release/eidolon-node /usr/local/bin/eidolon-node

# Switch to non-root user
USER eidolon

EXPOSE 8545

# Health check — hit /health every 30s
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:8545/health || exit 1

CMD ["eidolon-node"]
