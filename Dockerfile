# Use the rust build image from docker as our base
FROM rust:1.75.0 as base

# Set our working directory for the build
WORKDIR /usr/src/router

# Update our build image and install required packages
RUN apt-get update && \
    apt-get -y install  \
      clang \
      cmake \
      mold \
      nodejs \
      npm \
      protobuf-compiler && \
    rm -rf /var/lib/apt/lists/*

# Add rustfmt since build requires it
RUN rustup component add rustfmt

# Use cargo-chef for better caching
RUN cargo install cargo-chef --locked

FROM base as planner

# Copy the router source to our cache environment
COPY . .

# Prepare the recipe to build with
RUN cargo chef prepare --recipe-path recipe.json

FROM base as build

# Build dependencies
COPY --from=planner /usr/src/router/recipe.json recipe.json
RUN --mount=type=ssh cargo chef cook --release --recipe-path recipe.json

# Copy the router source to our build environment
COPY . .

# Build and install the custom binary
RUN cargo build --release

# Make directories for config and schema
RUN mkdir -p /dist/config && \
    mkdir /dist/schema && \
    mv target/release/router /dist

FROM debian:bookworm-slim

RUN apt-get update &&  \
    apt-get -y install ca-certificates && \
    rm -rf /var/lib/apt/lists/*

# Copy in the required files from our build image
COPY --from=build --chown=root:root /dist /dist

WORKDIR /dist

ENV APOLLO_ROUTER_CONFIG_PATH="/dist/config.yaml"

# Default executable is the router
ENTRYPOINT ["/dist/router"]
