# syntax=docker/dockerfile:1

# Both base image versions are ARGs so a bump is a one-line change.
ARG RUST_VERSION=1.97
ARG ALPINE_VERSION=3.24

# ---------------------------------------------------------------------------
# Builder
#
# rust:alpine is itself a musl image, so the host triple is already
# x86_64-unknown-linux-musl or aarch64-unknown-linux-musl. A plain
# `cargo build --release` therefore yields the fully static binary that
# release.yml needs an explicit --target plus musl-tools to obtain. Each
# architecture builds natively on its own runner, so there is no
# cross-compilation anywhere in this file and no QEMU in the workflow.
# ---------------------------------------------------------------------------
FROM rust:${RUST_VERSION}-alpine${ALPINE_VERSION} AS builder

# build-base supplies gcc, musl-dev and binutils -- the C toolchain that the
# C-backed codec behind zip's default features (zstd) needs to
# compile. `strip` below is redundant given Cargo.toml's
# [profile.release] strip = true, which already ships a stripped binary; it
# is kept anyway as an explicit, harmless safeguard in case that profile
# setting ever changes.
RUN apk add --no-cache build-base

WORKDIR /src
COPY . .

# `--bin oxidex` restricts the build to the CLI, skipping the feature-gated
# tag-comparison and jpeg-tag-matrix binaries -- that is all `--bin` does.
# It does NOT stop Cargo from attempting the library's full declared
# crate-type list (lib, staticlib, cdylib) from Cargo.toml; Cargo builds
# that list regardless of which --bin was requested. The build succeeds
# because rustc detects that cdylib is unsupported on this static-musl
# target and drops it with a warning instead of failing to link.
#
# --locked pins the build to the committed Cargo.lock, which also arrives
# via COPY . . above. Without it, a stale lockfile would be silently
# updated inside the container, so the published image could ship a
# dependency set nobody tested.
RUN cargo build --release --locked --bin oxidex \
    && strip target/release/oxidex

# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------
FROM alpine:${ALPINE_VERSION} AS runtime

# No explicit `apk add ca-certificates` on purpose. The one that stood here
# justified itself as "oxidex fetches over HTTPS through ureq/rustls" -- it
# does not. `default = []` in Cargo.toml plus the `--bin oxidex` build above
# mean neither ureq nor ort is in this binary, and nothing in the library or
# CLI opens a socket. That claim traced back to reading the root package's
# (since removed) dead [build-dependencies] as a runtime dependency.
#
# Alpine's base image still ships ca-certificates-bundle, which owns
# /etc/ssl/certs/ca-certificates.crt -- so a trust store is present anyway
# should a future network-facing feature need one. What is gone is only the
# larger ca-certificates package and its update-ca-certificates tooling.
COPY --from=builder /src/target/release/oxidex /usr/local/bin/oxidex

# /data is where callers are expected to mount their files:
#   docker run --rm -v "$PWD:/data" swackhamer/oxidex photo.jpg
WORKDIR /data

# Runs as root deliberately. oxidex writes and edits metadata in place, so a
# non-root default would make an in-place edit against a bind-mounted host
# directory fail with EACCES for most users. Callers who want to drop
# privileges can pass `--user "$(id -u):$(id -g)"`.
ENTRYPOINT ["/usr/local/bin/oxidex"]

# Declared last so that changing them invalidates only this final layer,
# leaving the expensive cargo build layer cached.
ARG VERSION=dev
ARG REVISION=unknown
LABEL org.opencontainers.image.title="oxidex" \
      org.opencontainers.image.description="High-performance Rust implementation of ExifTool for reading, writing and editing metadata in 300+ file formats" \
      org.opencontainers.image.source="https://github.com/swack-tools/oxidex" \
      org.opencontainers.image.licenses="GPL-3.0" \
      org.opencontainers.image.version="${VERSION}" \
      org.opencontainers.image.revision="${REVISION}"
