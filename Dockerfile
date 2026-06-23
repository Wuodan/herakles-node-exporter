# Multi-stage Dockerfile for herakles-node-exporter
# Builds the x86_64 musl eBPF-enabled binary using the same musl compatibility
# approach as CI, then ships it in a small Alpine runtime image.

FROM ubuntu:24.04 AS builder

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        autoconf \
        automake \
        autopoint \
        bison \
        build-essential \
        clang \
        ca-certificates \
        curl \
        flex \
        gawk \
        git \
        libelf-dev \
        libtool \
        libtool-bin \
        linux-libc-dev \
        musl-tools \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/opt/rustup
ENV CARGO_HOME=/opt/cargo
ENV PATH=/opt/cargo/bin:${PATH}

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /app
COPY . .

RUN rustup target add x86_64-unknown-linux-musl

RUN set -euo pipefail \
    && env_file=/tmp/herakles-musl.env \
    && chmod +x ./scripts/setup-musl-ebpf-compat.sh \
    && ./scripts/setup-musl-ebpf-compat.sh x86_64-unknown-linux-musl "${env_file}" \
    && set -a \
    && source "${env_file}" \
    && set +a \
    && cargo build --release --no-default-features --features ebpf-vendored --target x86_64-unknown-linux-musl

FROM alpine:3.24 AS runtime

RUN apk add --no-cache \
    ca-certificates \
    && rm -rf /var/cache/apk/*

RUN addgroup -g 1000 herakles \
    && adduser -D -u 1000 -G herakles herakles

COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/herakles-node-exporter /opt/herakles/bin/

USER herakles

EXPOSE 9215

HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
    CMD wget -q -O /dev/null http://localhost:9215/health || exit 1

ENTRYPOINT ["/opt/herakles/bin/herakles-node-exporter"]
CMD []
