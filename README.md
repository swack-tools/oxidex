# OxiDex

[![CI](https://github.com/swack-tools/oxidex/workflows/CI/badge.svg)](https://github.com/swack-tools/oxidex/actions)
[![Crates.io](https://img.shields.io/crates/v/oxidex.svg)](https://crates.io/crates/oxidex)
[![License: GPL-3.0](https://img.shields.io/badge/License-GPL--3.0-blue.svg)](LICENSE)

A high-performance Rust implementation of [ExifTool](https://exiftool.org/) for metadata extraction and manipulation.

## What is OxiDex?

OxiDex is a memory-safe, drop-in replacement for the Perl-based ExifTool. It defines 16,684 metadata tags across 140+ formats (see [Tag Coverage](https://oxidex.net/reference/tag-coverage-analysis) for the measured extraction-conformance score), with significantly better performance through Rust's zero-cost abstractions and parallel processing.

## Why OxiDex?

- **3.7-9.7x faster** than Perl ExifTool ([see benchmarks](https://oxidex.net/performance/benchmarks))
- **Memory safe** - No buffer overflows, use-after-free, or data races
- **Drop-in compatible** - Same CLI arguments as original ExifTool
- **Cross-platform** - Static binaries for Linux, macOS, and Windows
- **Library + CLI** - Use as a Rust crate or standalone binary
- **AI-powered detection (optional)** - Magika deep learning model for enhanced file type detection (`--features magika`)

## Quick Start

### Download Binary

Pre-built binaries available on the [Releases page](https://github.com/swack-tools/oxidex/releases).

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

Build with the `magika` feature to enable Google's deep learning model for
enhanced file type identification (~99% accuracy across 200+ formats),
selectable at runtime with `--detector=magika`:

```bash
cargo build --release --features magika
oxidex --detector=magika unknown_file
```

## Documentation

- [User Guide](https://oxidex.net/) - Installation, usage, and format support
- [Benchmarks](https://oxidex.net/performance/#benchmark-results) - Performance comparison with Perl ExifTool
- [API Reference](https://docs.rs/oxidex) - Rust library documentation
- [Tag Machinery Status](docs/TAG_MACHINERY_STATUS.md) - Completed work, partial implementations and useful next steps
- [Automation Backlog](docs/AUTOMATION-AND-TESTER-PLAN.md) - Remaining upgrade, verification and migration work
- [AI Harness](docs/AI_HARNESS.md) - Harness architecture and historical experiments
- [GitHub Issues](https://github.com/swack-tools/oxidex/issues) - Bug reports and feature requests

## Closing the parity gap

Coverage work starts with [transcribing ExifTool's declarations](docs/TRANSCRIPTION.md),
extending shared decoding/conversion rules and routing real extraction through those tables.
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
