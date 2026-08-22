# syntax=docker/dockerfile:1

FROM debian:bookworm-slim AS watchexec

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl xz-utils \
    && rm -rf /var/lib/apt/lists/*

ARG TARGETARCH
ARG WATCHEXEC_VERSION=2.5.1

RUN set -eu; \
    case "$TARGETARCH" in \
        amd64) \
            target=x86_64-unknown-linux-gnu; \
            checksum=cafc381f74e95f8e93e796ef590c7cbbf3409dda6d56cf3dee6109c10e5188ee \
            ;; \
        arm64) \
            target=aarch64-unknown-linux-gnu; \
            checksum=217e564946fec9911279c455e174e938d497480792a342c28712e50346cc0140 \
            ;; \
        *) \
            echo "unsupported Watchexec architecture: $TARGETARCH" >&2; \
            exit 1 \
            ;; \
    esac; \
    archive="watchexec-${WATCHEXEC_VERSION}-${target}.tar.xz"; \
    curl --fail --silent --show-error --location \
        "https://github.com/watchexec/watchexec/releases/download/v${WATCHEXEC_VERSION}/${archive}" \
        --output "/tmp/${archive}"; \
    echo "${checksum}  /tmp/${archive}" | sha256sum --check -; \
    tar --extract --xz --file "/tmp/${archive}" --directory /tmp; \
    install --mode 0755 "/tmp/${archive%.tar.xz}/watchexec" /usr/local/bin/watchexec

FROM rust:1.97-slim-bookworm

RUN \
    --mount=type=cache,target=/var/cache/apt,sharing=locked \
    --mount=type=cache,target=/var/lib/apt,sharing=locked \
    apt-get update && \
    apt-get install --yes --no-install-recommends \
        libusb-1.0-0 \
        python3 \
        python3-dev \
        python3-venv && \
    apt-get clean && \
    rm -rf /var/lib/apt/lists/*

RUN rustup component add clippy rustfmt

RUN \
    --mount=type=cache,target=/usr/local/cargo/registry,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/git,sharing=locked \
    --mount=type=cache,target=/tmp/maturin-target \
    CARGO_TARGET_DIR=/tmp/maturin-target \
    cargo install maturin --version 1.14.1 --locked

COPY --from=watchexec /usr/local/bin/watchexec /usr/local/bin/watchexec

ARG USER_ID=1000
ARG GROUP_ID=1000

RUN groupadd --gid "${GROUP_ID}" developer \
    && useradd \
        --uid "${USER_ID}" \
        --gid "${GROUP_ID}" \
        --create-home \
        developer \
    && mkdir -p \
        /home/developer/.cargo \
        /home/developer/.config/escpost \
        /home/developer/target \
        /workspace/.venv \
    && chown -R developer:developer /home/developer /workspace

USER developer

ENV CARGO_HOME=/home/developer/.cargo
ENV CARGO_TARGET_DIR=/home/developer/target

WORKDIR /workspace

CMD ["cargo", "test", "--workspace"]
