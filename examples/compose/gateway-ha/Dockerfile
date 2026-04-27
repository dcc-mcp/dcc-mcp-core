# syntax=docker/dockerfile:1.23
#
# Multi-stage image for `dcc-mcp-server`.
#
# Build context: the dcc-mcp-core repo root (i.e. run `docker build` with
# `-f examples/compose/gateway-ha/Dockerfile .` from the repo root).

# ─── Stage 1: build ──────────────────────────────────────────────────────────
FROM rust:1.95-slim AS builder

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --bin dcc-mcp-server \
 && cp /src/target/release/dcc-mcp-server /usr/local/bin/dcc-mcp-server

# ─── Stage 2: runtime ────────────────────────────────────────────────────────
FROM debian:13-slim

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --home-dir /var/lib/dcc-mcp dcc \
 && mkdir -p /var/lib/dcc-mcp /var/log/dcc-mcp \
 && chown -R dcc:dcc /var/lib/dcc-mcp /var/log/dcc-mcp

COPY --from=builder /usr/local/bin/dcc-mcp-server /usr/local/bin/dcc-mcp-server

USER dcc
ENV DCC_MCP_REGISTRY_DIR=/var/lib/dcc-mcp/registry \
    DCC_MCP_LOG_DIR=/var/log/dcc-mcp \
    DCC_MCP_GATEWAY_PORT=9765

EXPOSE 9765

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -sf http://127.0.0.1:9765/health || exit 1

ENTRYPOINT ["/usr/local/bin/dcc-mcp-server"]
CMD ["--host", "0.0.0.0", "--dcc", "generic"]
