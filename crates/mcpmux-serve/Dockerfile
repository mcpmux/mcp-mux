# Headless McpMux gateway (`mcpmux serve`).
#
# Multi-stage: build the Rust binary, then ship it on a slim Debian runtime that
# also carries node/npx and uv/uvx so most registry MCP servers (which spawn via
# npx/uvx) run out of the box. For an HTTP-servers-only deployment, strip the
# runtime tools to shrink the image.
#
# Build context is the workspace root (mcp-mux/):
#   docker build -f crates/mcpmux-serve/Dockerfile -t mcpmux/gateway .

# ---- web UI build stage ----
# Build the desktop React app for the web (base /app/), so the Rust build can
# embed it (feature `embed-ui`) and serve the full admin UI at /app.
FROM node:20-bookworm AS webbuild
WORKDIR /src
RUN corepack enable
COPY . .
RUN pnpm install --frozen-lockfile=false \
    && cd apps/desktop \
    && MCPMUX_WEB_BASE=/app/ pnpm build:web

# ---- Rust build stage ----
# Needs a recent toolchain: some transitive deps require edition2024 (Rust 1.85+).
FROM rust:1-bookworm AS build
WORKDIR /src

# Build deps: the storage crate's keychain uses the `keyring` crate, whose Linux
# Secret Service backend links libdbus. pkg-config + libssl for other native deps.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        pkg-config libdbus-1-dev libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy the whole workspace (the serve crate depends on the sibling crates).
COPY . .
# Bring in the built web UI so `embed-ui` can embed it.
COPY --from=webbuild /src/apps/desktop/dist ./apps/desktop/dist

# Build the serve binary with the embedded web admin.
RUN cargo build --release -p mcpmux-serve --features embed-ui

# ---- runtime stage ----
FROM debian:bookworm-slim AS runtime

# Runtime deps: CA certs (registry/HTTPS), plus node + uv for stdio MCP servers.
# tini is PID 1 so stdio child processes are reaped (no zombies).
# libdbus-1-3: the binary links libdbus (keyring). There's no Secret Service in
# the container, so the key provider falls back to file/env — but the shared lib
# must be present for the binary to load.
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates curl tini nodejs npm libdbus-1-3 \
    && curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin sh \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Non-root runtime user; /data is the persistent volume (db, keys, logs, spaces).
RUN useradd --system --uid 10001 --home-dir /data --create-home mcpmux
COPY --from=build /src/target/release/mcpmux /usr/local/bin/mcpmux

ENV MCPMUX_DATA_DIR=/data \
    MCPMUX_HOST=0.0.0.0 \
    MCPMUX_PORT=45818
EXPOSE 45818
VOLUME ["/data"]
USER mcpmux
WORKDIR /data

# Healthcheck hits the gateway's /health.
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s --retries=3 \
    CMD curl -fsS http://127.0.0.1:45818/health || exit 1

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/mcpmux"]
