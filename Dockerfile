# syntax=docker/dockerfile:1
# The service image (ADR 0002): one program, run the same way on the hosted
# service, self-hosted and on Enterprise plan instances. The image also holds
# the web app's built files (ADR 0009), which the service serves.

# The web app: a static build of apps/web against the committed OpenAPI file.
FROM node:22-bookworm-slim AS web
WORKDIR /src
RUN npm install --global pnpm@10.28.0
COPY . .
RUN --mount=type=cache,target=/root/.local/share/pnpm/store \
    pnpm install --frozen-lockfile --filter @tokencontroller/web... \
 && pnpm --filter @tokencontroller/web build

FROM rust:1.97.0-slim-bookworm AS build
WORKDIR /src
# Queries are checked against the committed .sqlx cache, so the build needs
# no database (ADR 0005). The image's Rust is the version rust-toolchain.toml
# pins, so the build uses it as it is.
ENV SQLX_OFFLINE=true \
    RUSTUP_TOOLCHAIN=1.97.0
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p tokencontroller-server \
 && cp target/release/tokencontroller /tokencontroller

FROM gcr.io/distroless/cc-debian12:nonroot
LABEL org.opencontainers.image.title="Token Controller" \
      org.opencontainers.image.description="Cost accounting and management control for AI agents" \
      org.opencontainers.image.source="https://github.com/tokencontroller/tokencontroller" \
      org.opencontainers.image.url="https://docs.tokencontroller.com/self-hosting/docker" \
      org.opencontainers.image.licenses="AGPL-3.0-only"
COPY --from=build /tokencontroller /usr/local/bin/tokencontroller
COPY --from=web /src/apps/web/dist /usr/share/tokencontroller/web
EXPOSE 8080
USER nonroot
ENTRYPOINT ["/usr/local/bin/tokencontroller"]
