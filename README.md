# OxiDex

[![CI](https://github.com/swack-tools/oxidex/workflows/CI/badge.svg)](https://github.com/swack-tools/oxidex/actions)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)

A Rust reimplementation of [ExifTool](https://exiftool.org/) for metadata extraction and manipulation.

## What is OxiDex?

OxiDex is a memory-safe Rust reimplementation of the Perl-based ExifTool. The
current development snapshot contains 16,684 generated metadata tag
definitions. Source inspection maps 131 formats for detection and 129 to a
parser; an identified format is not necessarily parsed. See [Supported
formats](https://oxidex.net/reference/formats/) for those scopes and [Tag
Coverage](https://oxidex.net/reference/tag-coverage-analysis) for separately
labelled historical measurements.

## Why OxiDex?

- **Compiled Rust with parallel directory processing** - candidate performance has not been measured for this beta; see the [performance status](https://oxidex.net/performance/)
- **Memory safe** - No buffer overflows, use-after-free, or data races
- **ExifTool-style CLI** - familiar arguments with documented differences
- **Cross-platform** - Prebuilt binaries for Linux, macOS, and Windows
- **Library + CLI** - Use as a Rust crate or standalone binary
- **Optional Magika detection** - use the Magika model for file-type identification (`--features magika`)

## Quick Start

### Download Binary

Pre-built binaries available on the [Releases page](https://github.com/swack-tools/oxidex/releases).
The `2.0.0-beta.1` release is not published to crates.io; Rust users should
use the signed Git tag or build from a checkout. Debian/RPM and Homebrew
packages are not published by the beta release automation either.

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

## Contributing

Contributions welcome! Please ensure:
- Tests pass (`cargo test`)
- Code is formatted (`cargo fmt`)
- Clippy lints pass (`cargo clippy`)

## License

[GPL-3.0](LICENSE)

## Acknowledgments

Inspired by and compatible with [ExifTool](https://exiftool.org/) by Phil Harvey. OxiDex is an independent reimplementation.
