# Multi-arch Docker buildx Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Publish `swackhamer/oxidex` to Docker Hub as one multi-architecture image covering `linux/amd64` and `linux/arm64`, built natively on WarpBuild runners.

**Architecture:** A multi-stage Dockerfile builds the CLI on `rust:alpine` (whose host triple is already `*-unknown-linux-musl`, so no cross-compilation is needed) and copies the static binary onto a minimal `alpine` runtime. A GitHub Actions workflow builds each architecture on its own native Warp runner, pushes each by digest, smoke-tests it on that same native runner, and only then joins the digests into a single OCI manifest list.

**Tech Stack:** Docker Buildx, GitHub Actions, WarpBuild runners, Rust 1.97 / Alpine 3.24.

**Spec:** `docs/plans/specs/2026-08-01-multi-arch-docker-buildx-design.md`

## Global Constraints

- Registry and image name: `swackhamer/oxidex` (Docker Hub).
- `main` → `:latest`. Tag `v1.2.1` → `:v1.2.1` **and** `:1.2.1`. `:latest` is **never** published from a tag.
- Base images: builder `rust:1.97-alpine3.24`, runtime `alpine:3.24`. Both exposed as `ARG`s.
- Runners: `warp-ubuntu-latest-x64-8x` (amd64), `warp-ubuntu-latest-arm64-8x` (arm64).
- Every GitHub Action must be pinned by full commit SHA with a trailing `# vX.Y.Z` comment. This matches the existing convention in `.github/workflows/ci.yml`.
- The image runs as **root** by design. Do not add a `USER` instruction.
- No QEMU. `docker/setup-qemu-action` must not appear anywhere — each architecture builds natively.
- Do not modify any existing file. This change is purely additive: three new files.

## File Structure

| File | Responsibility |
|---|---|
| `.dockerignore` | Keeps `target/` (3.5 GB) out of the build context and prevents unrelated file changes from invalidating the cached `cargo build` layer |
| `Dockerfile` | Two-stage build: compile the CLI, then assemble a minimal runtime image |
| `.github/workflows/docker.yml` | Per-arch native build, smoke test, digest publish, manifest assembly |

Tasks 1 and 2 are independent — Task 2 needs only the image contract (name, entrypoint, fixture assertion), which is fully specified below. They may be implemented in parallel.

---

### Task 1: Dockerfile and .dockerignore

**Files:**
- Create: `.dockerignore`
- Create: `Dockerfile`
- Test: a real local `docker buildx build`, verified by running the image

**Interfaces:**
- Consumes: nothing.
- Produces: an image whose `ENTRYPOINT` is `/usr/local/bin/oxidex`, whose `WORKDIR` is `/data`, which runs as root, and which accepts build args `VERSION` and `REVISION` (both default to a placeholder). `docker run <image> --version` prints `oxidex <semver>`. `docker run -v <dir>:/data <image> sample_with_exif.jpg` prints a line exactly matching `IFD0:Make: TestCamera`.

- [ ] **Step 1: Create `.dockerignore`**

The dominant reason for this file is not context size but **cache invalidation**: the Dockerfile does `COPY . .`, so any file left in the context busts the cached `cargo build` layer when it changes. Excluding `docs/`, `tests/`, and `scripts/` means documentation- and test-only commits to `main` reuse the cached build entirely.

`benches/` is deliberately **kept**. `Cargo.toml` declares three explicit `[[bench]]` targets (lines 49-58), and Cargo errors at manifest-parse time if a declared target's source file is missing — even for a `--bin` build. `tests/` is safe to exclude because there are no explicit `[[test]]` declarations; those targets are auto-discovered, and auto-discovery tolerates a missing directory.

```
# Build output. This alone is ~3.5 GB and would otherwise be uploaded as
# build context on every single build.
target/
**/target/

# Git metadata and hooks are never build inputs.
.git/
.gitignore
.gitattributes
.githooks/

# Not build inputs. Excluding these also stops docs-only and test-only
# commits from invalidating the cached cargo build layer, because the
# Dockerfile does `COPY . .`.
docs/
tests/
scripts/
tools/
test_data/
fuzz/
dist/
.github/

# JavaScript tooling for the docs site.
node_modules/
**/node_modules/
package.json
package-lock.json
bun.lock

# Editor, OS, and agent scratch files.
.claude/
.vscode/
.idea/
.DS_Store

# Docker's own files are not needed inside the build.
Dockerfile
.dockerignore

# NOTE: benches/ is intentionally NOT ignored. Cargo.toml declares three
# [[bench]] targets by name, and Cargo fails to parse the manifest if a
# declared target's source file is absent.
```

- [ ] **Step 2: Create `Dockerfile`**

```dockerfile
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

# build-base supplies gcc, musl-dev and binutils. The C-backed codecs behind
# zip's default features need a C toolchain, and `strip` below comes from
# binutils.
RUN apk add --no-cache build-base

WORKDIR /src
COPY . .

# `--bin oxidex` restricts the build to the CLI. This skips the feature-gated
# tag-comparison and jpeg-tag-matrix binaries, and keeps the library a plain
# rlib dependency rather than also emitting the staticlib and cdylib
# crate-types declared in Cargo.toml, which do not reliably link against
# static musl.
RUN cargo build --release --bin oxidex \
    && strip target/release/oxidex

# ---------------------------------------------------------------------------
# Runtime
# ---------------------------------------------------------------------------
FROM alpine:${ALPINE_VERSION} AS runtime

# Required, not cosmetic: oxidex fetches over HTTPS through ureq/rustls, which
# carries no trust store of its own and fails with an unknown-issuer error on
# an image that has no CA bundle.
RUN apk add --no-cache ca-certificates

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
```

- [ ] **Step 3: Build the image locally and verify it compiles**

This is the step that retires the plan's highest risk: whether `crate-type = ["lib", "staticlib", "cdylib"]` (`Cargo.toml:33`) links against static musl. Build for the host architecture — the cdylib question is target-agnostic, so one architecture proves it.

Run:

```bash
docker buildx build --platform linux/arm64 --load -t oxidex:local .
```

Expected: build succeeds. If it fails during linking with an error naming `cdylib`, `staticlib`, or `-lgcc_s`, that is the anticipated risk — report it rather than working around it silently, and note the exact error.

On an amd64 host, substitute `--platform linux/amd64`. Do not use `--platform` values that require emulation; the point of this step is a fast native check.

- [ ] **Step 4: Verify the binary runs and prints a version**

Run:

```bash
docker run --rm oxidex:local --version
```

Expected: a line matching `oxidex ` followed by a semantic version, e.g. `oxidex 1.2.1`.

- [ ] **Step 5: Verify the image parses real metadata**

This proves the EXIF parser works inside the image, not merely that the binary links. `sample_with_exif.jpg` is 112 bytes and contains a real IFD0 block.

Run:

```bash
docker run --rm -v "$PWD/tests/fixtures/jpeg:/data:ro" oxidex:local sample_with_exif.jpg
```

Expected: output containing the exact line `IFD0:Make: TestCamera`. If that line is absent the image is not usable, regardless of exit code.

- [ ] **Step 6: Verify the image is small and correctly labelled**

Run:

```bash
docker image inspect oxidex:local --format '{{.Size}} {{.Os}}/{{.Architecture}} {{index .Config.Labels "org.opencontainers.image.licenses"}}'
```

Expected: a size under 40000000 (40 MB), an os/arch matching the platform built, and `GPL-3.0`.

- [ ] **Step 7: Commit**

```bash
git add Dockerfile .dockerignore
git commit --no-gpg-sign -m "feat(docker): add multi-stage Dockerfile for static musl CLI image

Builds on rust:alpine, whose host triple is already *-unknown-linux-musl,
so no cross-compilation is needed. Runtime is alpine plus ca-certificates,
which rustls requires as a trust store.

The .dockerignore excludes target/ (3.5 GB) and, just as importantly,
docs/, tests/ and scripts/ so that changes there do not invalidate the
cached cargo build layer.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

### Task 2: Docker publish workflow

**Files:**
- Create: `.github/workflows/docker.yml`
- Test: `actionlint .github/workflows/docker.yml`

**Interfaces:**
- Consumes: the image contract from Task 1 — `ENTRYPOINT` is the `oxidex` binary, `WORKDIR` is `/data`, build args `VERSION` and `REVISION` exist, `--version` prints `oxidex <semver>`, and parsing `sample_with_exif.jpg` prints `IFD0:Make: TestCamera`.
- Produces: nothing consumed by a later task.

- [ ] **Step 1: Create `.github/workflows/docker.yml`**

Three details in this file are load-bearing and must not be "simplified":

1. **The matrix is conditional via `fromJSON`, not a job-level `if`.** The `matrix` context is **not available** in `jobs.<job_id>.if` — only `github`, `needs`, `vars` and `inputs` are. A job-level `if: matrix.arch == 'amd64'` would not work. `jobs.<job_id>.strategy` *can* see `github`, so the matrix itself is what varies by event.
2. **`flavor: latest=false`** on `metadata-action`. Without it the action also stamps `latest` onto every version tag, putting tag pushes and `main` pushes in a race over what `:latest` means.
3. **`provenance: false`** on the build. Provenance attestations add `unknown/unknown` platform entries to the manifest list, which confuses `imagetools create`. Attestations are explicitly out of scope in the spec.

```yaml
name: Docker

on:
  push:
    branches: [main]
    tags: ['v*']
  pull_request:
    # Only when the image definition itself changes. A paths filter is safe
    # here because this workflow is NOT a required status check. This is the
    # opposite of ci.yml, whose header explains that filtering there would
    # leave docs-only PRs waiting forever on a check that never reports.
    paths:
      - 'Dockerfile'
      - '.dockerignore'
      - '.github/workflows/docker.yml'
  workflow_dispatch:

concurrency:
  # Same rationale as ci.yml: PR runs key by branch so a new push cancels the
  # superseded run, while pushes to main key by SHA so one merge never cancels
  # another merge's publish.
  group: ${{ github.workflow }}-${{ github.event_name == 'push' && github.sha || github.head_ref || github.ref }}
  cancel-in-progress: true

env:
  IMAGE: swackhamer/oxidex

permissions:
  contents: read

jobs:
  build:
    name: Build ${{ matrix.arch }}
    runs-on: ${{ matrix.runner }}
    timeout-minutes: 45
    strategy:
      fail-fast: false
      matrix:
        # Pull requests build amd64 only. The arm64 entry is absent from the
        # matrix entirely, so no Warp arm64 runner is ever allocated -- this
        # cannot be expressed as a job-level `if`, because the matrix context
        # is unavailable there.
        include: >-
          ${{ fromJSON(github.event_name == 'pull_request'
          && '[{"arch":"amd64","platform":"linux/amd64","runner":"warp-ubuntu-latest-x64-8x"}]'
          || '[{"arch":"amd64","platform":"linux/amd64","runner":"warp-ubuntu-latest-x64-8x"},{"arch":"arm64","platform":"linux/arm64","runner":"warp-ubuntu-latest-arm64-8x"}]') }}
    steps:
      - name: Checkout repository
        uses: actions/checkout@34e114876b0b11c390a56381ad16ebd13914f8d5 # v4.3.1

      # No setup-qemu-action anywhere: each architecture builds natively on its
      # own runner. Emulating an arm64 Rust build of this workspace would take
      # 5-10x longer.
      - name: Set up Buildx
        uses: docker/setup-buildx-action@bb05f3f5519dd87d3ba754cc423b652a5edd6d2c # v4.2.0

      - name: Log in to Docker Hub
        if: github.event_name != 'pull_request'
        uses: docker/login-action@dbcb813823bdd20940b903addbd779551569679f # v4.6.0
        with:
          username: ${{ secrets.DOCKERHUB_USERNAME }}
          password: ${{ secrets.DOCKERHUB_TOKEN }}

      # Push events publish an image that NO tag points at. That is what makes
      # publishing atomic: the smoke test below runs before the manifest list
      # exists, so a failure here means no tag ever moves.
      - name: Build and push by digest
        id: build
        if: github.event_name != 'pull_request'
        uses: docker/build-push-action@53b7df96c91f9c12dcc8a07bcb9ccacbed38856a # v7.3.0
        with:
          context: .
          platforms: ${{ matrix.platform }}
          provenance: false
          build-args: |
            VERSION=${{ github.ref_name }}
            REVISION=${{ github.sha }}
          outputs: type=image,name=${{ env.IMAGE }},push-by-digest=true,name-canonical=true,push=true
          cache-from: type=gha,scope=oxidex-${{ matrix.arch }}
          cache-to: type=gha,scope=oxidex-${{ matrix.arch }},mode=max

      # Pull requests load the image into the local daemon instead of pushing.
      # cache-to is deliberately omitted: a PR branch's cache entry is not
      # readable from main, so writing it would only consume the 10 GB quota.
      - name: Build for validation
        if: github.event_name == 'pull_request'
        uses: docker/build-push-action@53b7df96c91f9c12dcc8a07bcb9ccacbed38856a # v7.3.0
        with:
          context: .
          platforms: ${{ matrix.platform }}
          provenance: false
          load: true
          tags: oxidex:ci
          build-args: |
            VERSION=pr-${{ github.event.pull_request.number }}
            REVISION=${{ github.sha }}
          cache-from: type=gha,scope=oxidex-${{ matrix.arch }}

      # Runs on the native runner for this architecture, so each image is
      # exercised on the hardware it targets. This cannot live in the merge
      # job: that runs on ubuntu-latest and could not execute the arm64 image
      # without the emulation this design avoids.
      - name: Smoke test
        env:
          DIGEST: ${{ steps.build.outputs.digest }}
        run: |
          set -euo pipefail
          if [ "${{ github.event_name }}" = "pull_request" ]; then
            ref="oxidex:ci"
          else
            ref="${IMAGE}@${DIGEST}"
          fi
          echo "Testing $ref"

          version=$(docker run --rm "$ref" --version)
          echo "version: $version"
          case "$version" in
            oxidex\ *) ;;
            *) echo "::error::unexpected --version output: $version"; exit 1 ;;
          esac

          out=$(docker run --rm -v "$PWD/tests/fixtures/jpeg:/data:ro" "$ref" sample_with_exif.jpg)
          echo "$out"
          if ! printf '%s\n' "$out" | grep -qx 'IFD0:Make: TestCamera'; then
            echo "::error::image did not parse EXIF from sample_with_exif.jpg"
            exit 1
          fi
          echo "Smoke test passed for $ref"

      - name: Export digest
        if: github.event_name != 'pull_request'
        env:
          DIGEST: ${{ steps.build.outputs.digest }}
        run: |
          set -euo pipefail
          mkdir -p "${RUNNER_TEMP}/digests"
          touch "${RUNNER_TEMP}/digests/${DIGEST#sha256:}"

      - name: Upload digest
        if: github.event_name != 'pull_request'
        uses: actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02 # v4.6.2
        with:
          name: digest-${{ matrix.arch }}
          path: ${{ runner.temp }}/digests/*
          if-no-files-found: error
          retention-days: 1

  merge:
    name: Publish manifest list
    needs: [build]
    if: github.event_name != 'pull_request'
    # No build work happens here, so no Warp runner is consumed.
    runs-on: ubuntu-latest
    timeout-minutes: 10
    steps:
      - name: Download digests
        uses: actions/download-artifact@d3f86a106a0bac45b974a628896c90dbdf5c8093 # v4.3.0
        with:
          path: ${{ runner.temp }}/digests
          pattern: digest-*
          merge-multiple: true

      - name: Set up Buildx
        uses: docker/setup-buildx-action@bb05f3f5519dd87d3ba754cc423b652a5edd6d2c # v4.2.0

      - name: Log in to Docker Hub
        uses: docker/login-action@dbcb813823bdd20940b903addbd779551569679f # v4.6.0
        with:
          username: ${{ secrets.DOCKERHUB_USERNAME }}
          password: ${{ secrets.DOCKERHUB_TOKEN }}

      # latest=false is load-bearing. Without it metadata-action also stamps
      # `latest` onto every version tag, which would put tag pushes and main
      # pushes in a race over what :latest means.
      - name: Compute tags
        id: meta
        uses: docker/metadata-action@dc802804100637a589fabce1cb79ff13a1411302 # v6.2.0
        with:
          images: ${{ env.IMAGE }}
          flavor: latest=false
          tags: |
            type=raw,value=latest,enable={{is_default_branch}}
            type=ref,event=tag
            type=semver,pattern={{version}}

      - name: Create and push manifest list
        working-directory: ${{ runner.temp }}/digests
        run: |
          set -euo pipefail
          ls -l
          docker buildx imagetools create \
            $(jq -cr '.tags | map("-t " + .) | join(" ")' <<< "$DOCKER_METADATA_OUTPUT_JSON") \
            $(printf "${IMAGE}@sha256:%s " *)

      - name: Inspect published image
        run: |
          set -euo pipefail
          docker buildx imagetools inspect "${IMAGE}:${{ steps.meta.outputs.version }}"
```

- [ ] **Step 2: Lint the workflow**

`actionlint` checks context availability, so it is a real gate on the `fromJSON` matrix decision described above — not a formality.

Run:

```bash
actionlint .github/workflows/docker.yml
```

Expected: no output and exit code 0. If it reports that a context is unavailable in a given key, that is a genuine error in the workflow — fix it rather than suppressing it.

- [ ] **Step 3: Verify the tag logic by inspection**

There is no way to execute `metadata-action` locally, so confirm by reading that all three statements below hold. Any that does not is a bug to fix before committing.

1. On a push to `main`, `type=raw,value=latest,enable={{is_default_branch}}` produces `latest`, and neither `type=ref,event=tag` nor `type=semver` produces anything (there is no tag).
2. On a push of tag `v1.2.1`, `type=ref,event=tag` produces `v1.2.1` and `type=semver,pattern={{version}}` produces `1.2.1`. `type=raw` produces nothing because `is_default_branch` is false for a tag ref.
3. `flavor: latest=false` is present, so no tag push can produce `latest`.

- [ ] **Step 4: Verify every action is SHA-pinned**

Run:

```bash
grep -n 'uses:' .github/workflows/docker.yml
```

Expected: every line references a 40-character hex SHA followed by a `# vX.Y.Z` comment. No line may use a bare tag such as `@v4`.

- [ ] **Step 5: Confirm no QEMU crept in**

Run:

```bash
grep -c 'setup-qemu' .github/workflows/docker.yml || true
```

Expected: `0`. Any occurrence means the build would silently emulate rather than build natively, defeating the purpose of the Warp arm64 runner.

- [ ] **Step 6: Commit**

```bash
git add .github/workflows/docker.yml
git commit --no-gpg-sign -m "ci(docker): publish multi-arch images to Docker Hub

Builds linux/amd64 and linux/arm64 natively on WarpBuild runners, pushes
each by digest, smoke-tests each on its own native runner, then joins the
digests into one manifest list. main publishes :latest; a v* tag publishes
both :v1.2.1 and :1.2.1, never :latest.

Requires DOCKERHUB_USERNAME and DOCKERHUB_TOKEN repository secrets.

Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>"
```

---

## Post-implementation

Neither task can verify end-to-end publishing, because that needs Docker Hub credentials that do not exist in the repository yet. Before the first push to `main` succeeds, the repository owner must add:

| Secret | Value |
|---|---|
| `DOCKERHUB_USERNAME` | Docker Hub account name |
| `DOCKERHUB_TOKEN` | Docker Hub access token with Read/Write scope |

Pull-request runs work without these, because they never log in.
