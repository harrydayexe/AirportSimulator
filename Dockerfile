# syntax=docker/dockerfile:1
#
# The production image: `airport-server` (one replica of the airport group)
# and the `airport-sim` command-line runner, on a distroless base.
#
#   docker build -t airport-simulator .
#   docker compose up            # three replicas, see docker-compose.yml

# Keep in step with `channel` in rust-toolchain.toml.
ARG RUST_VERSION=1.97.0

FROM rust:${RUST_VERSION}-slim-bookworm AS build
ARG RUST_VERSION
# The image already has this toolchain. Naming it stops rustup reading
# rust-toolchain.toml and fetching clippy, rustfmt and the wasm target.
ENV RUSTUP_TOOLCHAIN=${RUST_VERSION}
# tini is PID 1 in the final image, so that SIGTERM stops the server.
RUN apt-get update \
    && apt-get install -y --no-install-recommends tini \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
# The release this build is for, shown by `airport-server --version`.
ARG VERSION
ENV AIRPORT_SIM_VERSION=${VERSION}
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,sharing=locked \
    cargo build --release --locked -p airport-server -p airport-sim \
    && mkdir -p /out/bin /out/data \
    && cp target/release/airport-server target/release/airport-sim /out/bin/

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=build /usr/bin/tini-static /usr/bin/tini
COPY --from=build /out/bin/ /usr/local/bin/
# An empty directory the unprivileged user owns; a volume mounted here
# starts out with the same owner.
COPY --from=build --chown=nonroot:nonroot /out/data /data
ENV AIRPORT_DATA_DIR=/data \
    AIRPORT_LISTEN_PEER=0.0.0.0:7000 \
    AIRPORT_LISTEN_CLIENT=0.0.0.0:8080
VOLUME /data
# 7000: other replicas (keep it on a private network). 8080: clients.
EXPOSE 7000 8080
USER nonroot
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s \
    CMD ["/usr/local/bin/airport-server", "healthcheck"]
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/airport-server"]
