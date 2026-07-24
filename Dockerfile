# Multi-stage build: compile guff, ship with a Go toolchain (needed for `go list`).
FROM rust:1-bookworm AS builder

WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --locked -p guff-lint

FROM golang:1.24-bookworm

RUN apt-get update \
  && apt-get install -y --no-install-recommends git ca-certificates \
  && rm -rf /var/lib/apt/lists/* \
  && git config --global --add safe.directory '*'

ENV GOTOOLCHAIN=auto

COPY --from=builder /src/target/release/guff /usr/local/bin/guff

WORKDIR /app
ENTRYPOINT ["guff"]
CMD ["run", "./..."]
