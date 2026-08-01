# Multi-arch Docker images via buildx — design

**Date:** 2026-08-01
**Status:** Approved, pending implementation
**Branch:** `claude/multi-arch-docker-buildx-535caa`

## Goal

Publish `swackhamer/oxidex` to Docker Hub as a single multi-architecture image
covering `linux/amd64` and `linux/arm64`:

- pushes to `main` → `swackhamer/oxidex:latest`
- pushes of a `v*` git tag → `swackhamer/oxidex:v1.2.1` **and** `swackhamer/oxidex:1.2.1`

Both architectures are built natively on WarpBuild runners and joined into one
OCI manifest list, so `docker pull swackhamer/oxidex:latest` resolves to the
correct architecture automatically.

## Context discovered during design

These findings shaped the decisions below and are recorded so the plan does not
have to re-derive them.

1. **No Dockerfile exists anywhere in the repository.** `packaging/` contains
   only a Homebrew formula. This work therefore delivers a Dockerfile, not just
   a workflow.
2. **WarpBuild is already integrated.** `.github/workflows/release.yml` already
   runs on `warp-ubuntu-latest-arm64-2x` and `warp-macos-15-arm64-6x`, so the
   runner labels resolve with no new account setup.
3. **Static musl builds are proven for both architectures.** `release.yml`
   builds `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` natively.
4. **No OpenSSL anywhere.** The only network dependency is `ureq` with `rustls`
   (`Cargo.toml:149`), so there is no system TLS to link — but rustls still
   needs a CA trust store at runtime for `tag_sync`, which rules out a bare
   `scratch` image.
5. **Tag-crate build scripts are hermetic.** `oxidex-tags-*/build.rs` read local
   YAML and serialize with bincode. No network, no subprocesses. The Docker
   build needs the network only to fetch crates.
6. **`target/` is 3.5 GB.** A `.dockerignore` is mandatory, not a nicety.
7. **No Docker Hub credentials exist.** `gh secret list` shows only the Apple
   signing and Codacy secrets.
8. **Current toolchain:** Rust 1.97.1 stable, Alpine 3.24. There is no
   `rust-toolchain.toml`; CI uses `dtolnay/rust-toolchain@stable`.

## Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Build strategy | Multi-stage Dockerfile that runs `cargo build` itself | `docker build .` works standalone for any contributor; the image is reproducible from the repo alone |
| Base image | `alpine` + `ca-certificates` | ~8 MB; has a shell for debugging; provides the trust store rustls needs |
| Runner size | 8x on both architectures | The workspace has 8 codegen-heavy `oxidex-tags-*` crates; 8 vCPU is where cost/time flattens |
| Tag policy | `v1.2.1` and `1.2.1` on tags; `latest` only on `main` | Publishes both the literal git tag and the form users try first, with no race over `latest` |
| PR runs | Only when the Dockerfile itself changes; amd64, build-only | Catches a broken Dockerfile pre-merge without paying for every PR |
| Container user | `root` | oxidex writes and edits metadata; a non-root default breaks `-v $PWD:/data ... -overwrite_original` with EACCES for most users |
| Smoke test | `--version` plus parsing a real fixture | Proves the binary parses on that architecture, not merely that it links |

### Why native runners rather than QEMU

The conventional single-runner approach emulates `linux/arm64` instruction by
instruction. For a Rust workspace this size an emulated release build typically
runs 5–10× slower. Building each architecture on its own native Warp runner
avoids this entirely.

### Why `rust:alpine` removes the cross-compilation problem

On an Alpine base the host triple already *is* `x86_64-unknown-linux-musl` or
`aarch64-unknown-linux-musl`. A plain `cargo build --release` therefore produces
the static binary that `release.yml` currently needs `--target` plus
`musl-tools` to obtain. No `--target` flag, no linker configuration, no cross
toolchain.

## Architecture

### Files

Three new files. No existing file is modified.

| File | Purpose |
|---|---|
| `Dockerfile` | Multi-stage build, repo root |
| `.dockerignore` | Keeps the 3.5 GB `target/` out of the build context |
| `.github/workflows/docker.yml` | Build, publish, and manifest assembly |

### Dockerfile

Two stages, both image versions exposed as `ARG`s so bumping is a one-line
change.

**Builder** — `rust:1.97-alpine3.24`:

- `apk add --no-cache build-base` (zip's default features pull C-backed codecs)
- `cargo build --release --bin oxidex`
  `--bin` restricts the build to the CLI, skipping the feature-gated
  `tag-comparison` and `jpeg-tag-matrix` binaries.
- `strip` the resulting binary

**Runtime** — `alpine:3.24`:

- `apk add --no-cache ca-certificates` — required or `tag_sync`'s rustls HTTPS
  fails with no trust store
- Copy the binary to `/usr/local/bin/oxidex`
- `WORKDIR /data` so `-v "$PWD:/data"` is the natural invocation
- `ENTRYPOINT ["/usr/local/bin/oxidex"]` so `docker run swackhamer/oxidex --version`
  works directly
- Runs as `root`; users wanting otherwise pass `--user $(id -u):$(id -g)`
- Standard OCI labels (`org.opencontainers.image.source`, `.version`,
  `.revision`, `.licenses`)

### .dockerignore

Excludes `target/`, `.git/`, `docs/`, `test_data/`, `node_modules/`, and editor
and CI cruft.

Deliberately **keeps** `tests/` and `benches/`: Cargo errors on manifest-declared
target paths that do not exist, and both are declared in `Cargo.toml`.

### Workflow — `.github/workflows/docker.yml`

**Triggers**

| Event | Behaviour |
|---|---|
| `push` to `main` | Build both arches, publish `:latest` |
| `push` tag `v*` | Build both arches, publish `:v1.2.1` and `:1.2.1` |
| `pull_request` with `paths:` on `Dockerfile`, `.dockerignore`, `.github/workflows/docker.yml` | Build amd64 only, no login, no push |
| `workflow_dispatch` | Manual escape hatch |

A `paths:` filter is safe in this workflow because it is not a required status
check. This is the opposite of `ci.yml`, whose header comment (lines 18–21)
explains that path filtering there would leave docs-only PRs permanently
unmergeable.

**Concurrency** follows the `ci.yml` precedent: PR runs keyed by branch so a new
push cancels the superseded run; pushes to `main` keyed by commit SHA so merges
do not cancel one another.

**Job `build`** — matrix:

| platform | runner |
|---|---|
| `linux/amd64` | `warp-ubuntu-latest-x64-8x` |
| `linux/arm64` | `warp-ubuntu-latest-arm64-8x` |

A job-level condition —
`if: github.event_name != 'pull_request' || matrix.arch == 'amd64'` — skips the
arm64 leg on pull requests without allocating a runner.

Steps: checkout → `docker/setup-buildx-action` → `docker/login-action` (skipped
on PRs) → `docker/build-push-action` → **smoke test** (see below) → export the
digest → upload it as an artifact.

The build step's output mode depends on the event:

- **push events** — `push-by-digest=true`, publishing an image that **no tag
  points at**
- **pull requests** — `load: true` with a throwaway local tag, so the image
  stays on the runner and nothing is published

`push-by-digest` is what makes publishing atomic. Because the smoke test runs
inside this job and `merge` declares `needs: build`, a smoke-test failure fails
the build job, `merge` never runs, and no tag is ever created. `:latest`
continues to point at the last good build, and the orphaned digest is unreachable.

**Caching** — `type=gha`, `mode=max`, scope `oxidex-<arch>`. Separate scopes per
architecture so the two do not evict each other. The high-value case is a `v*`
tag pointing at a tree `main` has already built, which then hits cache and
finishes in roughly a minute.

**Job `merge`** — `ubuntu-latest`, `needs: build`, skipped on pull requests:

1. Download both digest artifacts
2. `docker/metadata-action` computes the tag set
3. `docker buildx imagetools create` joins the per-arch digests into one
   manifest list under the computed tags
4. `docker buildx imagetools inspect` prints the final index for the run log,
   confirming both `linux/amd64` and `linux/arm64` are present

No build work happens here, so `ubuntu-latest` is sufficient and no Warp runner
is consumed.

**Tag rules** via `docker/metadata-action` with **`flavor: latest=false`**:

```
type=raw,value=latest,enable={{is_default_branch}}
type=ref,event=tag               # -> v1.2.1
type=semver,pattern={{version}}  # -> 1.2.1
```

`latest=false` is the load-bearing flag. Without it `metadata-action` also
stamps `latest` onto every version tag, which would put tag pushes and `main`
pushes in a race over what `:latest` means.

**Permissions:** `contents: read` only. Docker Hub authentication is by secret,
not by `GITHUB_TOKEN`, so no `packages:` scope is needed.

### Smoke test

Runs **inside the `build` job**, on the native runner for that architecture,
before any tag points at the image. Placing it here rather than in `merge` is
deliberate on two counts: `merge` runs on `ubuntu-latest` and so could not
execute the arm64 image without emulation, and testing before the manifest list
exists is what preserves the atomic-publish property.

The reference is the pushed digest on push events, or the locally loaded tag on
pull requests:

1. `docker run --rm <ref> --version` — confirms the binary is the right
   architecture and executes
2. `docker run --rm -v "$PWD/tests/fixtures/jpeg:/data:ro" <ref> sample_with_exif.jpg`
   — confirms real metadata comes out, catching architecture-specific defects
   such as endianness or unaligned-read faults. The step asserts on the output
   rather than only on the exit code, so a silent empty result still fails.

The bind mount works because these jobs run steps directly on the runner VM
alongside its Docker daemon. Bind mounts fail only when the *job itself* runs
inside a container (`jobs.<id>.container:`), where `-v /path` resolves on the
daemon's host rather than inside the job container. No job here uses `container:`.

The mount is read-only and the fixtures are world-readable after checkout, so
the root-vs-non-root choice does not affect the test.

## Prerequisites — owner action required

Two repository secrets must be added before the workflow can publish. Neither
exists today.

| Secret | Value |
|---|---|
| `DOCKERHUB_USERNAME` | Docker Hub account name |
| `DOCKERHUB_TOKEN` | Docker Hub access token, Read/Write scope |

Until these exist, pushes to `main` will fail at the login step. Pull-request
runs are unaffected because they never log in.

## Risks

1. **`crate-type = ["lib", "staticlib", "cdylib"]`** (`Cargo.toml:33`) — a
   cdylib against a static-musl target can fail to link. `cargo build --bin oxidex`
   should build the library as an rlib dependency only, avoiding the cdylib
   entirely. If it does not, the fallback is to build the binary from a context
   that does not request the other crate types. This is the most likely
   first-run failure and should be verified before anything else.
2. **`zip = "8.6"` default features** may pull C-backed codecs (bzip2, zstd).
   `build-base` in the builder stage covers this; if a specific `-sys` crate
   needs more, the error will name it.
3. **`lto = true` with `codegen-units = 1`** (`Cargo.toml:162`) — the fat-LTO
   pass is largely single-threaded. This bounds the benefit of larger runners
   and is why 8x rather than 16x was chosen. Expect several minutes of
   effectively serial link time on a cold build.
4. **GHA cache limits** — the repository-wide GitHub Actions cache is capped at
   10 GB. Two architectures at `mode=max` may approach this and cause eviction
   of older entries. If churn becomes a problem, switch `cache-to` to a registry
   backend such as `swackhamer/oxidex:buildcache-<arch>`.

## Out of scope

- Mirroring to GHCR. The requested target is Docker Hub; adding a second
  registry is a small additive change if wanted later.
- Build provenance and SBOM attestations.
- Publishing the `magika` feature variant, which pulls the ONNX runtime and
  would need a substantially larger image.
- Any change to `release.yml`. The Docker workflow is independent and does not
  consume its artifacts, so `main` pushes work without a release having run.
