# Use the rust build image from docker as our base
FROM rust:1.76-bookworm as base

# Set our working directory for the build
WORKDIR /usr/src/router

# Update our build image and install required packages
RUN set -eux; \
    apt-get update; \
    apt-get -y install  \
      clang \
      libclang-dev \
      cmake \
      protobuf-compiler

RUN set -eux; \
    mkdir -p ~/.ssh/; \
    ssh-keyscan ssh.shipyard.rs >> ~/.ssh/known_hosts \
    ssh-keyscan github.com >> ~/.ssh/known_hosts

ARG MOLD_VERSION=2.31.0
RUN set -eux; \
    wget -qO /tmp/mold.tar.gz https://github.com/rui314/mold/releases/download/v${MOLD_VERSION}/mold-${MOLD_VERSION}-x86_64-linux.tar.gz; \
    tar -xf /tmp/mold.tar.gz -C /usr/local --strip-components 1; \
    rm /tmp/mold.tar.gz

# Use cargo-chef for better caching
ARG CARGO_CHEF_VERSION=0.1.66
RUN cargo install --locked cargo-chef@${CARGO_CHEF_VERSION}

# Copy the router source to our cache environment and prepare the recipe to build with
FROM base as planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM base as build

# Build dependencies
COPY --from=planner /usr/src/router/recipe.json recipe.json
RUN --mount=type=cache,target=/root/.rustup \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/usr/src/router/target \
    --mount=type=ssh \
    --mount=type=secret,id=shipyard-token \
    set -eux; \
    export CARGO_REGISTRIES_WAFFLEHACKS_TOKEN=$(cat /run/secrets/shipyard-token); \
    export CARGO_REGISTRIES_WAFFLEHACKS_CREDENTIAL_PROVIDER=cargo:token; \
    export CARGO_NET_GIT_FETCH_WITH_CLI=true; \
    cargo chef cook --release --recipe-path recipe.json

# Copy the router source to our build environment and build
COPY . .
RUN --mount=type=cache,target=/root/.rustup \
    --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    --mount=type=cache,target=/usr/src/router/target \
    --mount=type=ssh \
    set -eux; \
    cargo build --release --bin router; \
    mkdir -p /dist/config; \
    mkdir /dist/schema; \
    objcopy --compress-debug-sections ./target/release/router /dist/router

FROM debian:bookworm-slim

RUN set -eux; \
    apt-get update;  \
    apt-get -y install ca-certificates; \
    rm -rf /var/lib/apt/lists/*

# Copy in the required files from our build image
COPY --from=build --chown=root:root /dist /dist

WORKDIR /dist

ENV APOLLO_ROUTER_CONFIG_PATH="/dist/config.yaml"

# Default executable is the router
ENTRYPOINT ["/dist/router"]
