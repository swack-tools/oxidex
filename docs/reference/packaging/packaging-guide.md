# Packaging and distribution

This page describes the current OxiDex packaging policy and the optional
local package helpers. It is intentionally explicit about what the
`2.0.0-beta.1` release does and does not publish.

## Beta distribution policy

The current beta release automation publishes signed GitHub release assets
only: platform binaries, the macOS DMG, checksums, and independent release
provenance. Debian/RPM, Homebrew, and crates.io packages are not published
by the current beta release automation. Do not upload a locally generated package to a beta release or
describe it as an official distribution channel.

The supported beta installation path is the signed asset on the
[GitHub Releases page](https://github.com/swack-tools/oxidex/releases). Rust
users who need the library can use the signed Git tag as a Git dependency or
build from a checkout. The root crate remains intentionally unpublished.

## Optional local experiments

The repository retains Cargo metadata for `cargo-deb` and
`cargo-generate-rpm`. These tools are useful for developers who want to
inspect a local package, but they are not part of the release workflow and
their output is not a published OxiDex beta artifact.

The helper scripts derive the product version from the root Cargo package:

```bash
./scripts/build-all-packages.sh
./scripts/test-packages.sh all
```

The package helpers may be run only on a host with the relevant tool installed
(`cargo-deb`, `cargo-generate-rpm`, `dpkg-deb`, or `rpm`). Installation tests
may require elevated privileges. The `brew` test is a documented no-op for
this beta because there is no active formula.

### Debian package (local only)

Install `cargo-deb`, then build and inspect a package from the checkout:

```bash
cargo build --release
cargo deb
ls -lh target/debian/oxidex_*.deb
dpkg-deb --info target/debian/oxidex_*.deb
```

For a local installation test, use the exact filename produced by the tool;
do not copy a version from an old example:

```bash
sudo dpkg -i target/debian/oxidex_<VERSION>_amd64.deb
oxidex --version
sudo dpkg -r oxidex
```

### RPM package (local only)

Install `cargo-generate-rpm`, build the release binary first, and inspect the
result:

```bash
cargo build --release
cargo generate-rpm
ls -lh target/generate-rpm/oxidex-*.rpm
rpm -qip target/generate-rpm/oxidex-*.rpm
```

For a local installation test, use the exact filename produced by the tool:

```bash
sudo dnf install target/generate-rpm/oxidex-<VERSION>-1.x86_64.rpm
oxidex --version
sudo dnf remove oxidex
```

The local package output must not be uploaded to the GitHub beta release. The
release workflow's own checksums and provenance cover only the assets it
builds and uploads.

## Homebrew

Homebrew is **not enabled for the beta**. There is no active formula, tap, or
bottle, and there is no release URL or checksum to copy into one. The
quarantined placeholder and the policy for any future formula are documented
in `packaging/homebrew/README.md`.

For a macOS developer build, use Cargo directly:

```bash
cargo build --release
./target/release/oxidex --version
```

When Homebrew support is approved, it must be introduced together with a
published asset, a real checksum, formula tests, workflow coverage, and an
updated policy page. Until then, do not run or publish a formula from this
repository.

## Verifying local packages

The scripts are local validation helpers, not release publication commands:

```bash
./scripts/test-packages.sh deb
./scripts/test-packages.sh rpm
./scripts/test-packages.sh brew  # reports the beta policy and does no install
```

Each installed local package should report the same version as the checkout:

```bash
cargo metadata --no-deps --format-version 1 \
  | python3 -c 'import json, sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "oxidex"))'
oxidex --version
```

## Future distribution work

Crates.io, Debian/RPM repositories, and Homebrew may be considered for a
future release. Each requires a separate maintainer decision, publication
workflow, signing/checksum policy, and factual documentation update. Do not
pre-populate those surfaces with guessed URLs, tags, checksums, or artifacts.

For the current release workflow and its signed macOS assets, see
[`docs/RELEASE-2.0.0-beta.1.md`](../../RELEASE-2.0.0-beta.1.md) and
`.github/workflows/release.yml`.
