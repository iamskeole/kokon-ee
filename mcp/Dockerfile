# kokon-ee-mcp sidecar image: a small container that runs the Kokon MCP server in `serve`
# mode (Streamable HTTP). On the box it shares the service's network namespace (compose
# network_mode: "container:kokon-ee"): it reaches the service's loopback token door, and
# the only door it opens is its own Bearer-protected listener (nothing is published).
# Docs: mcp/README.md (serve mode), docs/service-on-prod-box.md.
#
#   docker build -f mcp/Dockerfile -t kokon-ee-mcp .

FROM ubuntu:22.04 AS build
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /mcp
COPY mcp/package.json mcp/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY mcp/ ./
# One self-contained .cjs (the SDK bundled in).
RUN npm run build

FROM ubuntu:22.04
ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
 && apt-get install -y --no-install-recommends nodejs \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /mcp/build/kokon-ee-mcp.cjs /usr/local/lib/kokon-ee-mcp/kokon-ee-mcp.cjs
COPY mcp/README.md /usr/local/lib/kokon-ee-mcp/
# The sidecar's compose sets these: the service's data folder (service.json: port +
# token), the listener's port/interface, its Bearer secret, and QLS_MCP_MODEL (the
# model this server's jobs run on).
ENV QLS_DATA_DIR=/data
ENTRYPOINT ["node", "/usr/local/lib/kokon-ee-mcp/kokon-ee-mcp.cjs", "serve"]
