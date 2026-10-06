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
under `target/remote-linux/debug/<run_id>/oxidex` and
`target/remote-linux/release/<run_id>/oxidex`. Each receipt prints the exact
`artifact` path; run-specific directories keep parallel downloads separate.
They cannot run natively on macOS. Remote release builds first run the pinned
`cbindgen-check` inside the container and stop if the C header is stale. Use `just build-release-local` for the local
all-features release build.

### Repository-owned remote-build commands

The client lives in `tools/remote-build/` in this checkout. Just calls
`build.py` directly. No global Cargo
shim or infrastructure checkout is needed to submit a build. Python 3.11+
is required, and the client uses only the Python standard library.

`OXIDEX_REMOTE_PROJECT` selects the GCP project (default: the active gcloud
project). `OXIDEX_REMOTE_WORKTREE` selects a namespace for remote source and target
directories (default: a hash of the local checkout path). Each build appends a
random run suffix, so concurrent clients cannot overwrite each other's source
or target directories even if they reuse the namespace. The Cargo download
cache remains shared; compiled target output is scoped to one build. Every
successful build downloads a verified local binary (by default under
`target/remote-linux/<profile>/<run_id>/oxidex`) before removing its exact remote source
and uploaded archive. Remote target output and Cargo caches are retained. Failed builds attempt the same exact-run cleanup
while preserving local receipts and logs. An abruptly interrupted client may
leave its run on the host; inspect the `run_id` and instance in its receipt,
then dry-run the exact paths with `ls -ld` over authenticated SSH before any
manual removal. For an interrupted run, this read-only command lists exactly
the remote paths named by a receipt:

```bash
build_receipt=/path/to/remote-build.json
build_run_id=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["run_id"])' "$build_receipt")
build_instance=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["instance"])' "$build_receipt")
build_zone=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["zone"])' "$build_receipt")
build_project=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["project"])' "$build_receipt")
gcloud compute ssh "$build_instance" --zone="$build_zone" --project="$build_project" \
  --command="ls -ld -- /mnt/runner-data/remote-build/sources/$build_run_id /mnt/runner-data/remote-build/targets/$build_run_id ~/oxidex-remote-source-$build_run_id.tar.gz"
```

Inspect the output and follow the cleanup runbook before deleting any retained
run. Automatic builds choose a fresh instance and zone for every attempt. Direct `python3 tools/remote-build/direct.py` uses the configured
instance and zone. Settings may be environment variables or local
`.cargo/config.toml` `[env]` entries; no personal VM or credentials are committed.

```bash
export OXIDEX_REMOTE_PROJECT=YOUR_PROJECT
just build-debug
just build-release

# Optional: use a specific worker instead of automatic selection.
export OXIDEX_REMOTE_INSTANCE=YOUR_INSTANCE
export OXIDEX_REMOTE_ZONE=YOUR_ZONE
export OXIDEX_REMOTE_INSTANCE_ID=YOUR_NUMERIC_VM_ID
export OXIDEX_REMOTE_SSH_USER=YOUR_ENROLLED_BUILD_USER
export OXIDEX_REMOTE_SSH_KEY="$HOME/.ssh/YOUR_APPROVED_KEY"
export OXIDEX_REMOTE_SSH_KNOWN_HOSTS="$HOME/.ssh/YOUR_TRUSTED_BUILDER_HOSTS"
python3 tools/remote-build/direct.py --profile debug
```

Normal `just build`, `just test`, the standard build/test/check/lint/bench
recipes, and `just build-remote` use a dedicated Spot builder from a developer
checkout. `just build-local` is the deliberate local debug-workspace exception.
The generic recipe route snapshots the working tree, executes the **same Just
recipe** in the restricted Linux builder container, and retains the remote
source/target plus a local run receipt and hashed log. For build recipes it also
downloads a SHA-checked Linux `oxidex` binary below
`target/remote-linux/<profile>/<run-id>/`; other workspace outputs stay in the
retained remote target. The generic recipe receipt is a development result, not
the separate exact-head `just test-remote` qualification proof. Recipes that
require local corpus inputs or return source changes refuse on a developer
machine until their input/output mapping is reviewed.

To keep personal worker details out of the checkout, put only the named remote
settings in your existing user Cargo config (`$CARGO_HOME/config.toml`, or
`$HOME/.cargo/config.toml` when `CARGO_HOME` is unset). Preserve its other
entries. An example of the `[env]` section is:

```toml
[env]
OXIDEX_REMOTE_PROJECT = "your-project"
OXIDEX_REMOTE_INSTANCE = "builder-your-worker"
OXIDEX_REMOTE_ZONE = "your-zone"
OXIDEX_REMOTE_INSTANCE_ID = "123456789"
OXIDEX_REMOTE_SSH_USER = "oxidex-uploader"
OXIDEX_REMOTE_SSH_KEY = "/absolute/path/to/approved-key"
OXIDEX_REMOTE_SSH_KNOWN_HOSTS = "/absolute/path/to/trusted-known-hosts"
```

Omit instance, zone and ID for automatic CPU/memory selection. The resolver
uses process environment first, then checkout `.cargo/config.toml` `[env]`,
then user Cargo `[env]`; empty or malformed named values refuse. Paths must be
absolute. The direct route still checks current provider ID, strict host-key
trust, installed launcher, image and CPU/memory admission before transfer.

An explicit build user uses direct SSH/SCP with the approved key and strict
host-key checking. Supply a trusted known-hosts file provisioned independently;
the client does not enroll keys in VM metadata. Explicit worker dispatch requires
the numeric VM ID and checks the installed launcher and current CPU/memory before
source transfer. Automatic selection still requires complete Monitoring windows.

`just test-remote` uses the same selected builder and restricted launcher. It
requires a clean checkout with a verified maintainer-signed HEAD, provisions the
locked Perl/ExifTool oracle and fixture corpus remotely, runs the remote-client
and version-qualification Python suites, then executes
`cargo test --workspace --all-features --locked --no-fail-fast`. No Rust tests or
oracle bootstrap run locally. The oracle installation is shared in the persistent
Cargo mount, keyed by the locked oracle specification and verified on each run;
a provisioning lock serializes concurrent installers. Test results remain in
each run’s separate target directory. The downloaded `remote-test.json` proof must match
the source SHA, compiler identity, oracle pin, fixture floor and test commands
before the client reports success. Receipts and logs live under
`$OXIDEX_OPS_DIR/evidence/auto-remote-<timestamp>` (default
`$HOME/oxidex-ops/evidence`). When a test result cannot be retrieved and verified,
the client retains the remote run and disables automatic retry; inspect the
receipt and live process before resuming. Workspace test proof does not replace
release qualification or CI.

### Credentials and VM setup

Install Python 3.11+, Git, Cargo, just, and Google Cloud CLI (`gcloud`). Authenticate GCP locally with `gcloud auth login` and select the
project with `gcloud config set project YOUR_PROJECT`. The build account needs Compute Engine inventory/machine-type read, SSH access
and Monitoring read access. Provisioning requires additional VM/disk permissions.
VM provisioning and fleet scaling remain in
[spot-github-runners](https://github.com/swack-tools/spot-github-runners).
For new hosts, clone its `codex/resource-scaling-remote-rust-builds` branch,
install its documented dependencies (including `gh`), and configure project APIs
and quotas according to that repository's README.

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
instance name starting with `builder-` so automatic selection discovers
the host. The VM service account needs
`roles/monitoring.metricWriter` and the Monitoring write OAuth scope so the
metrics-only Ops Agent can publish memory utilization. CPU utilization uses
Compute Engine's built-in metric. Authenticated `gcloud compute ssh` must work;
GCP manages the SSH access path, with no SSH server inside the build container.

Building on an existing worker needs GCP/SSH/Monitoring access; it does not need
a GitHub token. `just build-debug-remote` and `just build-release-remote` select
running `builder-*` hosts with complete recent metrics, CPU and memory both
below 75%, the pinned executable builder launcher, and no drain marker.
The selection probe checks the launcher's exact revision and executes its
side-effect-free argument-validation path, so existing hosts must receive the
matching launcher before they are eligible. The client verifies the remote
compiler and Cargo against this checkout's rustup-resolved toolchain pin before
fetching or building; the pin must also be installed locally. It ranks available CPU
and RAM as balanced capacity (four GiB per available core); job and container
counts do not influence selection. Monitoring uses complete five-minute
windows ending two minutes ago, so selection reflects recent utilization
rather than an instantaneous reservation.

An explicitly maintainer-approved existing VM can be selected through
`${OXIDEX_OPS_DIR:-$HOME/oxidex-ops}/config/remote-build.json`. Use schema version
1 and an `instances` array of objects containing `name`, numeric string `id`,
`project`, `zone`, and boolean `enabled`. Keep the entry disabled until its
container, pinned toolchain and admission are verified. Selection requires the
exact VM ID; direct builds recheck that ID through GCP before remote work. A
replacement with the same name needs a new approval. The exception retains
all utilization, launcher, drain and compiler checks.

SSH disconnects, draining hosts, and killed containers cause another attempt
on a different VM, with fresh inventory/metrics and source resync. Three
attempts are allowed by default; use
`python3 tools/remote-build/build.py --profile debug --max-attempts 5` to change
this. Compiler and C-header validation errors stop immediately. Missing
metrics or no eligible host produce an explicit error. Existing hosts need
the updated root-owned launcher to propagate killed-container exit codes.
The availability probe does not reserve capacity for concurrent clients.
Receipts and timing logs use `$OXIDEX_OPS_DIR/evidence/` (default
`$HOME/oxidex-ops/evidence/`).

## Contributing

Contributions are welcome. Before contributing, ensure:
- Tests pass (`cargo test`)
- Code is formatted (`cargo fmt`)
- Clippy lints pass (`cargo clippy`)

## License

[GPL-3.0](LICENSE)

## Acknowledgments

Inspired by and compatible with [ExifTool](https://exiftool.org/) by Phil Harvey. OxiDex is an independent reimplementation.

### Restricted remote Task19 qualification

`just qualify-remote OUTPUT REFERENCE PROVISIONING` runs the existing three-row Task19
qualification and corpus read gate on an approved restricted Linux builder.
`OUTPUT` must be an unused directory below `OXIDEX_OPS_DIR`; `REFERENCE`
points to the approved Task19 reference evidence directory containing the
byte-frozen `read-policy-input.json` and `downloaded/inputs/provisioned`
read selections. `PROVISIONING` names the retained Task19 `provisioned` input
documents and write/native fixtures. The client
requires a clean, maintainer-signed candidate HEAD and the pre-enrolled
`OXIDEX_REMOTE_SSH_USER`, `OXIDEX_REMOTE_SSH_KEY`, and
`OXIDEX_REMOTE_SSH_KNOWN_HOSTS` settings. It records transport evidence below
`$OXIDEX_OPS_DIR/evidence/remote-qualification/<run-id>`.

The canonical launcher owns `/src`, `/target`, the build lock, and the immutable
builder image. Preparation verifies the transferred signed Git bundle and
regenerates path-bound plans, source resolutions, materializations, cases, and
fixture manifests under `/target`. It preserves the standalone floor policy
byte-for-byte and refuses a changed ordered fixture selection. The three rows
run serially; the remote source, targets, input archive and lease are retained
for audit. A transport timeout leaves the exact job handle and namespace in
`transport.json` and does not retry or terminate an unconfirmed run. Publication
requires the remote committed-result loader, corpus gate, full archive replay,
and hash-checked direct download. This recipe does not promote `main` or tag a
release.
