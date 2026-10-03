# OxiDex

[![CI](https://github.com/swack-tools/oxidex/workflows/CI/badge.svg)](https://github.com/swack-tools/oxidex/actions)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)

A Rust reimplementation of [ExifTool](https://exiftool.org/) for metadata extraction and manipulation.

## What is OxiDex?

OxiDex is a memory-safe Rust reimplementation of the Perl-based ExifTool. The
current development snapshot contains 32,256 generated metadata tag
definitions. Source inspection maps 131 formats for detection and 129 to a
parser; an identified format is not necessarily parsed. See [Supported
formats](https://oxidex.net/reference/formats/) for those scopes and [Tag
Coverage](https://oxidex.net/reference/tag-coverage-analysis) for separately
labelled historical measurements.

## Why OxiDex?

- **Compiled Rust with parallel directory processing** - candidate performance has not been measured for this beta; see the [performance status](https://oxidex.net/performance/)
- **Memory safe** - No buffer overflows, use-after-free, or data races
- **ExifTool-style CLI** - familiar arguments with documented differences
- **Cross-platform** - Beta release binaries for Linux, macOS, and Windows are pending
- **Library + CLI** - Use as a Rust crate or standalone binary
- **Optional Magika detection** - use the Magika model for file-type identification (`--features magika`)

## Quick Start

### Build from source

The `2.0.0-beta.1` signed tag and binary assets are pending. To use the
development snapshot, build from source on `refactor/tag-machinery` or an
explicitly chosen development commit. Beta crates are not published to crates.io
by this project. Debian/RPM and Homebrew packages are not published by the beta
release automation either.

## Usage

```bash
# Extract all metadata
oxidex photo.jpg

# Extract specific tags
oxidex -Make -Model -DateTimeOriginal photo.jpg

# Write metadata
oxidex -Artist="Your Name" photo.jpg

# Batch processing
oxidex -r /path/to/photos/

# JSON output
oxidex -json photo.jpg
```

### Optional: Magika AI-Powered Detection

Build with the `magika` feature to enable Google's Magika model for file-type
identification, selectable at runtime with `--detector=magika`. This page does
not make an accuracy or performance claim for Magika; no beta-bound receipt for
either measurement is published here:

```bash
cargo build --release --features magika
oxidex --detector=magika unknown_file
```

## Documentation

- [User Guide](https://oxidex.net/) - Installation, usage, and format support
- [Performance](https://oxidex.net/performance/) - benchmark status and how to reproduce measurements
- [API Reference](https://oxidex.net/reference/api-reference) - Rust library documentation (this beta is not on crates.io)
- [Autogeneration plan](docs/AUTOGENERATION-PLAN.md) - the goal, the measured state and the ordered next steps; the mechanism is in [AUTOGENERATION-V2-DESIGN.md](docs/AUTOGENERATION-V2-DESIGN.md)
- [Tag Machinery Status](docs/TAG_MACHINERY_STATUS.md) - Dated integration status, evidence limits and the documentation map
- [Automation Backlog](docs/AUTOMATION-AND-TESTER-PLAN.md) - Remaining upgrade, verification and migration work
- [AI Harness](docs/AI_HARNESS.md) - Harness architecture and historical experiments
- [GitHub Issues](https://github.com/swack-tools/oxidex/issues) - Bug reports and feature requests

## Closing the parity gap

The goal is that a change to ExifTool's tag definitions flows into OxiDex by
regeneration, without anyone retyping a tag name, byte layout, camera-selection
rule or conversion. The [autogeneration plan](docs/AUTOGENERATION-PLAN.md) owns
the goal, the measured state and the next steps; the
[v2 design](docs/AUTOGENERATION-V2-DESIGN.md) specifies the mechanism (generated
conversions over a session, a real grammar, a helper library, per-field mixed
mode); [progress](docs/AUTOGENERATION-PROGRESS.md) is the scoreboard.
[Transcription](docs/TRANSCRIPTION.md) documents the method and its history.
Generation, runtime activation and measured output are tracked separately.

[Tag Machinery Status](docs/TAG_MACHINERY_STATUS.md) records the audited integration
commit, completed migrations, unfinished work and evidence limits. The
[automation backlog](docs/AUTOMATION-AND-TESTER-PLAN.md) prioritizes a reproducible
release-upgrade workflow and measured migrations. The AI harness report remains
available as historical evidence; it is not the current coverage roadmap.

## Development

### Prerequisites

Install development tools via Homebrew:
```bash
brew install cargo-watch cargo-nextest cargo-audit cargo-outdated
```

Install additional tools via Cargo:
```bash
cargo install cargo-tarpaulin
```

### Just Commands

This project uses [just](https://github.com/casey/just) as a command runner. Run `just` to see all available commands:

```bash
just          # List all commands
just ci       # Run full CI checks
just test     # Run all tests
just lint     # Run clippy linter
just fmt      # Format code
```

## Remote Linux builds

`just build-debug` and `just build-release` select a running remote builder
using CPU and memory metrics, synchronize tracked working files over SSH, and
build in a dedicated unprivileged container. These builds are independent of
GitHub Actions jobs. The downloaded, checksum-verified Linux binaries are saved
under `target/remote-linux/debug/oxidex` and `target/remote-linux/release/oxidex`.
They cannot run natively on macOS. Remote release builds first run the pinned
`cbindgen-check` inside the container and stop if the C header is stale. Use `just build-release-local` for the local
all-features release build.

### Install the remote-build commands

The scripts live in [spot-github-runners](https://github.com/swack-tools/spot-github-runners),
not in the operations evidence directory. From the OxiDex checkout, clone the
infrastructure repository locally if you do not already have it. The remote
build tooling is currently on the feature branch shown below:

```bash
git clone --branch codex/resource-scaling-remote-rust-builds \
  git@github.com:swack-tools/spot-github-runners.git spot-github-runners
python3 spot-github-runners/src/install_remote_commands.py
```

Alternatively, install from an existing sibling checkout:

```bash
python3 ../spot-instance-gha-runners/src/install_remote_commands.py
```

Installation writes small `cargo-oxidex-auto` and `cargo-oxidex-remote` shims into
`$CARGO_HOME/bin` (default `~/.cargo/bin`). The shims execute scripts directly
from the chosen Git checkout; keep it in place and put the bin directory on
`PATH`. `.cargo/config.toml` provides the `remote-debug`, `remote-release`, and
`remote-build` aliases. Its non-secret `OXIDEX_REMOTE_PROJECT` setting selects
the GCP project (default: the active gcloud project);
`OXIDEX_REMOTE_WORKTREE` selects the persistent source/target cache identifier
(default: a unique identifier derived from the local checkout path). Give each local checkout a distinct worktree identifier.
Automatic builds override the instance and zone for that invocation. Direct
`cargo remote-build` uses the configured instance and zone. These settings may
be environment variables or local `.cargo/config.toml` `[env]` entries; no
personal VM identity or credential is checked into the command aliases.

```bash
export OXIDEX_REMOTE_PROJECT=YOUR_PROJECT
just build-debug
just build-release

# Optional: use a specific worker instead of automatic selection.
export OXIDEX_REMOTE_INSTANCE=YOUR_INSTANCE
export OXIDEX_REMOTE_ZONE=YOUR_ZONE
cargo remote-build --profile debug
```

### Credentials and VM setup

Install Python 3.11+, Git, Cargo, just, Google Cloud CLI (`gcloud`), and GitHub
CLI (`gh`). Authenticate GCP locally with `gcloud auth login` and select the
project with `gcloud config set project YOUR_PROJECT`. The account needs Compute
Engine VM/disk provisioning and SSH access plus Monitoring read access. Configure
the project APIs and quotas according to the runner repository's README.

For provisioning GitHub runners, copy `.env.example` to `.env` **inside the
runner repository** and set `GH_TOKEN` to a renewable credential with organization
self-hosted-runner management permission and access to the selected repository.
A classic PAT needs `admin:org`; a fine-grained credential needs organization
self-hosted runners read/write permission. The runner group must allow OxiDex.
The manager creates short-lived registration tokens; do not store those tokens
in `.env`, Cargo configuration, or an image. `GOOGLE_CLOUD_API_KEY` is optional
for Billing Catalog lookups; `GCP_BILLING_EXPORT_TABLE` is optional for cost
reporting. Never commit `.env`, GitHub credentials, or SSH private keys.

Provision hosts with `--remote-builder --resource-monitoring` in
`src/spot_runner_manager.py`; preview first and use `--apply` to provision.
The autoscaler enables the builder by default. For manual provisioning, use an
instance name starting with `oxidex-runners-` so automatic selection discovers
the host. The VM service account needs
`roles/monitoring.metricWriter` and the Monitoring write OAuth scope so the
metrics-only Ops Agent can publish memory utilization. CPU utilization uses
Compute Engine's built-in metric. Authenticated `gcloud compute ssh` must work;
GCP manages the SSH access path, with no SSH server inside the build container.

Building on an existing worker needs GCP/SSH/Monitoring access; it does not need
a GitHub token. Selection considers running `oxidex-runners-*` and
`oxidex-buildbench-*` hosts with complete recent metrics, CPU and memory both
below 75%, an installed builder, and no active remote build or drain marker.
It ranks by the higher utilization fraction. If none qualifies, retry later.
The availability probe does not reserve a VM for concurrent clients. Build
receipts and timing logs remain under `$HOME/oxidex-ops/evidence/`.

## Contributing

Contributions are welcome. Before contributing, ensure:
- Tests pass (`cargo test`)
- Code is formatted (`cargo fmt`)
- Clippy lints pass (`cargo clippy`)

## License

[GPL-3.0](LICENSE)

## Acknowledgments

Inspired by and compatible with [ExifTool](https://exiftool.org/) by Phil Harvey. OxiDex is an independent reimplementation.
