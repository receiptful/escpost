# The Bun image plus `zip`, which the Chrome upload package needs.
FROM oven/bun:1.4.0

RUN apt-get update \
    && apt-get install --yes --no-install-recommends zip \
    && rm -rf /var/lib/apt/lists/*
