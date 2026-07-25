# OxiDex Rename Design

**Date**: 2025-11-16
**Status**: Approved
**Approach**: Big Bang Rename

## Overview & Scope

### Objective
Complete rebrand from "oxidex" to "oxidex" while maintaining ExifTool compatibility messaging and preserving git history.

### Scope of Changes

**Crate Renaming**:
- Main crate: `oxidex` → `oxidex`
- Library name: `oxidex` → `oxidex`
- Binary name: `oxidex` → `oxidex`
- Tag crates: `exiftool-tags*` → `oxidex-tags*`

**What Changes**:
- All `Cargo.toml` files (package names, dependencies)
- All Rust source code (module names, imports, crate references)
- Documentation (README, docs/, CHANGELOG, etc.)
- CI/CD workflows and scripts
- Package metadata (Debian, RPM, Homebrew formulas)
- Build scripts and configuration files

**What Stays the Same**:
- GitHub repository URL: `oxidex/oxidex` (no change)
- Git history (fully preserved)
- License (GPL-3.0)
- Core functionality and API design

**Strategy**: Execute all changes in a single PR on the `rename-to-oxidex` branch, test thoroughly, then merge to main. After merge, publish new crates to crates.io and deprecate old ones.

## Crate Renaming Strategy

### Workspace Crate Mapping

The project has 8 crates total that need renaming:

**Main Crate**:
- `oxidex` → `oxidex`
  - Package name: `oxidex`
  - Library name: `oxidex` (was `oxidex`)
  - Binary name: `oxidex` (was `oxidex`)

**Tag Database Crates** (7 crates):
- `exiftool-tags` → `oxidex-tags`
- `exiftool-tags-core` → `oxidex-tags-core`
- `exiftool-tags-camera` → `oxidex-tags-camera`
- `exiftool-tags-media` → `oxidex-tags-media`
- `exiftool-tags-image` → `oxidex-tags-image`
- `exiftool-tags-document` → `oxidex-tags-document`
- `exiftool-tags-specialty` → `oxidex-tags-specialty`

### Dependency Updates

All internal workspace dependencies must be updated. For example:
```toml
# Before
exiftool-tags = { path = "exiftool-tags" }

# After
oxidex-tags = { path = "oxidex-tags" }
```

### Directory Renaming

The subdirectory names should match the crate names:
- `exiftool-tags/` → `oxidex-tags/`
- `exiftool-tags-core/` → `oxidex-tags-core/`
- `exiftool-tags-camera/` → `oxidex-tags-camera/`
- `exiftool-tags-media/` → `oxidex-tags-media/`
- `exiftool-tags-image/` → `oxidex-tags-image/`
- `exiftool-tags-document/` → `oxidex-tags-document/`
- `exiftool-tags-specialty/` → `oxidex-tags-specialty/`

## File & Directory Changes

### Directory Renaming

**Crate Directories** (must be renamed to match new crate names):
- `exiftool-tags/` → `oxidex-tags/`
- `exiftool-tags-core/` → `oxidex-tags-core/`
- `exiftool-tags-camera/` → `oxidex-tags-camera/`
- `exiftool-tags-media/` → `oxidex-tags-media/`
- `exiftool-tags-image/` → `oxidex-tags-image/`
- `exiftool-tags-document/` → `oxidex-tags-document/`
- `exiftool-tags-specialty/` → `oxidex-tags-specialty/`

### Key Files to Update (Content Changes)

**Workspace Configuration**:
- Root `Cargo.toml` - workspace members, package metadata, all references
- All 8 `Cargo.toml` files in subdirectories

**Build & Configuration**:
- `build.rs` - any hardcoded crate name references
- `Cargo.lock` - will auto-update on rebuild
- `cbindgen.toml` - C binding configuration
- `Cross.toml` - cross-compilation config
- `rustfmt.toml`, `.clippy.toml` - should be fine as-is

**Documentation & Packaging**:
- `README.md`, `CHANGELOG.md`, `PACKAGING.md`, `RELEASE_CHECKLIST.md`
- All files in `docs/` directory
- `packaging/` - Debian, RPM, Homebrew formulas
- `justfile` - build automation recipes

**Infrastructure**:
- `.github/workflows/*.yml` - all CI/CD workflows
- `scripts/*` - any shell scripts with hardcoded names

## Code Changes

### Rust Source Code Updates

**Module and Crate Names**:
- All `use oxidex::*` → `use oxidex::*`
- All `use exiftool_tags::*` → `use oxidex_tags::*`
- Any `extern crate` declarations (if any exist)
- Doc comments that reference the crate name

**FFI/C Bindings** (`bindings/` directory):
- Function prefixes: `oxidex_*` → `oxidex_*`
- Header file names: `oxidex.h` → `oxidex.h`
- Generated binding code and examples

**String Literals and Constants**:
- Version strings and banners in CLI output
- Error messages mentioning "oxidex"
- User-facing help text and usage information
- Any hardcoded paths or identifiers

**Comments and Documentation**:
- Inline code comments mentioning "oxidex"
- Doc comments (`///` and `//!`)
- Module-level documentation
- Example code in documentation

**Test Code**:
- Test module names and test data
- Integration test references
- Benchmark names and descriptions

**Important Note**: We keep references to "ExifTool" (the original Perl tool) for compatibility messaging, but remove references to "oxidex" (our old name).

## Documentation Updates

### User-Facing Documentation

**README.md** (comprehensive rewrite):
- Title: "OxiDex" → "OxiDex"
- Badges: Update GitHub workflow badge URLs (keep repo URL as-is)
- Installation instructions: `cargo install oxidex` → `cargo install oxidex`
- CLI examples: `oxidex photo.jpg` → `oxidex photo.jpg`
- Library examples: `use oxidex::*` → `use oxidex::*`
- Download URLs: Update binary artifact names
- Project description: Maintain ExifTool compatibility messaging

**Core Documentation Files**:
- `CHANGELOG.md` - Add entry for rename, update historical references
- `PACKAGING.md` - Update package names and paths
- `RELEASE_CHECKLIST.md` - Update release procedures
- `RELEASE_ANNOUNCEMENT.md` - Rewrite for oxidex branding
- `LICENSE` - Update copyright holder if needed (currently "OxiDex Contributors")

**docs/ Directory**:
- All markdown files in `docs/` referencing the project name
- Architecture documentation
- API documentation
- User guides and tutorials

### Positioning Strategy

Keep ExifTool compatibility messaging prominent:
- "OxiDex - A high-performance Rust implementation of ExifTool for metadata extraction"
- "100% ExifTool tag parity" (keep this messaging)
- "ExifTool-compatible CLI" (emphasize compatibility)

## Build & CI/CD Updates

### GitHub Workflows (`.github/workflows/`)

**CI Workflow** (`ci.yml`):
- Job names and descriptions
- Binary artifact names: `oxidex` → `oxidex`
- Cache keys (may include crate names)
- Test commands and benchmark names
- Coverage report titles

**Release Workflow** (`release.yml`):
- Binary output names: `oxidex-x86_64-linux-musl` → `oxidex-x86_64-linux-musl`
- Artifact upload names
- Release asset naming
- Checksum file names (SHA256SUMS)

**Integration Test Workflow**:
- Test binary invocations
- Comparison output file names

### Package Metadata

**Debian Package** (`Cargo.toml` metadata.deb section):
- Package name: `oxidex` → `oxidex`
- Binary path: `/usr/bin/oxidex` → `/usr/bin/oxidex`
- Documentation paths: `/usr/share/doc/oxidex/` → `/usr/share/doc/oxidex/`
- Maintainer email: `oxidex@example.com` → update as needed

**RPM Package** (metadata.generate-rpm section):
- Same changes as Debian package
- Asset paths and destinations

**Homebrew Formula** (`packaging/homebrew/`):
- Formula file: `oxidex.rb` → `oxidex.rb`
- Formula class name: `ExiftoolRs` → `Oxidex`
- Binary installation paths
- Repository URLs (keep as-is since GitHub URLs don't change)

## Publishing Strategy

### Crates.io Publication Plan

**Pre-Publication** (before merging to main):
- Verify all 8 crate names are available on crates.io
- Check `oxidex`, `oxidex-tags`, `oxidex-tags-core`, etc.
- Reserve names if possible to prevent squatting

**Publication Order** (after merge to main):
1. Publish tag crates first (they have no dependencies):
   - `oxidex-tags-core`
   - `oxidex-tags-camera`
   - `oxidex-tags-media`
   - `oxidex-tags-image`
   - `oxidex-tags-document`
   - `oxidex-tags-specialty`
2. Publish `oxidex-tags` (depends on the above)
3. Publish `oxidex` main crate (depends on `oxidex-tags`)

**Old Crate Deprecation**:
1. Publish final version of `oxidex` with deprecation notice:
   - Update README: "⚠️ This crate has been renamed to `oxidex`. Please update your dependencies."
   - Add deprecation to crate description
   - Point to new crate in all documentation
2. Yank old versions if needed (optional, not recommended)
3. Archive old crate (mark as deprecated on crates.io)

**Version Strategy**:
- Start `oxidex` at current version or bump to next major version
- Tag git commit with new version after rename

**Documentation Links**:
- Update docs.rs links in README
- Ensure API documentation publishes correctly for all 8 crates

## Testing & Validation

### Pre-Merge Validation

**Build Verification**:
1. Clean build: `cargo clean && cargo build --release`
2. Verify binary name: `target/release/oxidex --version`
3. Check library compilation: All 8 crates build successfully
4. Cross-compilation test: `cargo build --target x86_64-unknown-linux-musl`

**Test Suite**:
1. Unit tests: `cargo test --all`
2. Integration tests: `cargo test --test '*'`
3. Doc tests: `cargo test --doc`
4. Benchmarks compile: `cargo bench --no-run`

**Functional Testing**:
1. CLI smoke tests:
   - `oxidex photo.jpg` (metadata extraction)
   - `oxidex -json photo.jpg` (JSON output)
   - `oxidex -csv -r tests/` (batch processing)
2. Library API test:
   - Create small test program using `oxidex` crate
   - Verify imports and functionality work

**Documentation Verification**:
1. Run `cargo doc --all --no-deps` - verify docs build
2. Check for broken internal links
3. Search for remaining "oxidex" or "oxidex" references:
   - `rg -i "exiftool.?rs" --type md`
   - `rg "oxidex" --type rust`
   - Exclude legitimate references to "ExifTool" (the Perl tool)

**CI/CD Validation**:
1. Push branch and verify GitHub Actions pass
2. Check artifact names in workflow runs
3. Verify benchmark reports generate correctly

### Post-Merge Validation

**Crates.io Verification**:
- Confirm all 8 crates published successfully
- Test installation: `cargo install oxidex`
- Verify documentation appears on docs.rs

**Release Artifacts**:
- GitHub release has correct binary names
- All platform binaries work (Linux, macOS, Windows)
- Checksums are correct

## Decision Log

1. **GitHub URLs**: Keep repository URL as `oxidex/oxidex` to preserve links and history
2. **Crates.io**: Publish new crates with oxidex names, deprecate old ones
3. **CLI Command**: Rename to `oxidex` (no backwards compatibility alias)
4. **Documentation**: Complete rebrand to oxidex in all documentation
5. **Positioning**: Maintain ExifTool compatibility messaging and references
6. **Approach**: Big Bang rename in single PR for clean transition
