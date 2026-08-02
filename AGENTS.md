# OxiDex Development Guide

## Overview
Rust implementation of ExifTool - high-performance metadata parsing for 140+ formats.

## Use rust uutils coreutils when you can like ripgrep and LSP's as well as claude-mem
to decrease the amount of time grepping. We also have things like hyperfine.

We have: coreutils bat eza fd ripgrep hyperfine dust bottom tokei procs sd zoxide starship gitui git-delta lsd tealdeer broot bandwhich grex xh just watchexec typos-cli nushell yazi atuin mprocs hurl

## Where you can utilize sccache to speed up builds especially when dealing with multiple worktrees
building the same thing basically

## Commands
```bash
cargo build                    # Build debug
cargo build --release          # Build release
cargo test --workspace         # Run all tests
just test                      # Run tests (CI config)
just check                     # Quick check without build
cargo clippy                   # Lint
just build-bin-release         # Build release binary
```

## Structure
- `src/` - Core library and CLI
- `src/exiftool_tables/` - Binary tag layouts transcribed from ExifTool's Perl tables (generated)
- `oxidex-tags-*` - Tag definition crates (auto-generated from ExifTool)
- `tests/` - Integration tests
- `benches/` - Performance benchmarks
- `bindings/` - C FFI bindings
- `docs/` - Documentation

## Closing an ExifTool coverage gap

**Check whether the answer is already transcribed before writing parser code.**
`src/exiftool_tables::find_table(module, table)` carries ExifTool's real byte
layout — `FORMAT`, `FIRST_ENTRY`, per-field `Format`, `Mask`, `SubDirectory`
edges, and enum `PrintConv` maps. Re-deriving by hand a binary record ExifTool
already declares is the expensive way to close a gap. See `docs/TRANSCRIPTION.md`.

**Tag knowledge is not tag coverage.** `src/tag_sync` ingests `exiftool -f -listx`,
which is the *documentation* view: it carries `count encoding id index lang name
type version writable` and nothing else. It has no `SubDirectory`, `FORMAT`,
`FIRST_ENTRY`, `ValueConv`, `Condition` or `DataMember` — that is, no layout. So
it can tell you a tag exists but never how to read one, and a rising
`oxidex-tags-*` count is **not** evidence of rising extraction coverage. Only a
comparison run measures coverage.

**Measure the gap by kind before costing the work.**
`python3 tools/exiftool-tables/conformance.py <corpus> --exiftool-dir <src> --oxidex <bin>`
classifies each difference as RENAME / MISSING / VALUE / EXTRA and prints a
`ceiling` column. A wide score-to-ceiling spread means free coverage (renames),
not parsing work.

**Never grade against an unpinned ExifTool.** `.exiftool-version` at the repo
root names the release the transcriptions come from, and it is the only source
of truth — the Rust oracle (`src/exiftool_oracle.rs`) compiles it in, the Python
one (`scripts/exiftool_oracle.py`) reads it, and CI and the justfile both fetch
that exact tag. Never invoke a bare `exiftool`: `PATH` resolved to 13.55 while
the tables were transcribed from 13.59, and the two disagree about which
sub-table a given byte count selects, so sixteen correct Canon R6 Mark III tags
were reported as regressions. The failure is symmetric — the same skew
manufactures phantom *fixes* — and neither is distinguishable from the real
thing afterwards.

**A matching `-ver` is not a working oracle.** The pinned tree's `exiftool`
starts `#!/usr/bin/env perl`, which finds a Homebrew perl with no
`Archive::Zip`; ExifTool then reports `FileType: ZIP` for a `.docx` and every
container format degrades at once, *while `-ver` still prints the right
release*. The oracle therefore also probes capability, and any corpus sweep
should assert a file-count and tag-count floor — a degraded run does not crash,
it reports a confident, precisely-formatted, completely wrong number.

**Never approximate a conversion.** A plausible-but-wrong value under a real
ExifTool tag name is worse than an absent tag: it does not crash, and nothing
downstream can tell. Omit and count it instead — that is the rule the generator
follows, and `just verify-tables` (also a CI job) enforces it for the generated
tables against an independent oracle.

## Architecture
Hexagonal (ports/adapters) with three layers:
- **Application**: CLI, C FFI bindings
- **Domain**: Format-agnostic metadata models
- **Infrastructure**: Format-specific parsers, I/O

## Style
- Run `cargo clippy` before commits
- Use `cargo fmt` for formatting
