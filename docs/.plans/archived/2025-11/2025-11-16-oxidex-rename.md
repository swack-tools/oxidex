# OxiDex Rename Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Rename the entire project from "oxidex" to "oxidex" including all crates, documentation, CI/CD, and package metadata.

**Architecture:** Big Bang approach - all changes in one PR for clean transition. Rename 8 crates (main + 7 tag crates), update all imports, documentation, and infrastructure. Preserve git history and GitHub URLs.

**Tech Stack:** Rust workspace with 8 crates, GitHub Actions CI/CD, Debian/RPM/Homebrew packaging, C FFI bindings.

---

## Task 1: Rename Crate Directories

**Files:**
- Rename: `exiftool-tags/` → `oxidex-tags/`
- Rename: `exiftool-tags-core/` → `oxidex-tags-core/`
- Rename: `exiftool-tags-camera/` → `oxidex-tags-camera/`
- Rename: `exiftool-tags-media/` → `oxidex-tags-media/`
- Rename: `exiftool-tags-image/` → `oxidex-tags-image/`
- Rename: `exiftool-tags-document/` → `oxidex-tags-document/`
- Rename: `exiftool-tags-specialty/` → `oxidex-tags-specialty/`

**Step 1: Rename exiftool-tags directory**

```bash
git mv exiftool-tags oxidex-tags
```

Expected: Directory renamed successfully

**Step 2: Rename exiftool-tags-core directory**

```bash
git mv exiftool-tags-core oxidex-tags-core
```

Expected: Directory renamed successfully

**Step 3: Rename exiftool-tags-camera directory**

```bash
git mv exiftool-tags-camera oxidex-tags-camera
```

Expected: Directory renamed successfully

**Step 4: Rename exiftool-tags-media directory**

```bash
git mv exiftool-tags-media oxidex-tags-media
```

Expected: Directory renamed successfully

**Step 5: Rename exiftool-tags-image directory**

```bash
git mv exiftool-tags-image oxidex-tags-image
```

Expected: Directory renamed successfully

**Step 6: Rename exiftool-tags-document directory**

```bash
git mv exiftool-tags-document oxidex-tags-document
```

Expected: Directory renamed successfully

**Step 7: Rename exiftool-tags-specialty directory**

```bash
git mv exiftool-tags-specialty oxidex-tags-specialty
```

Expected: Directory renamed successfully

**Step 8: Verify renames**

```bash
ls -d oxidex-tags*
```

Expected: All 7 directories with oxidex-tags prefix exist

**Step 9: Commit directory renames**

```bash
git add -A
git commit -m "refactor: rename crate directories from exiftool-tags to oxidex-tags"
```

---

## Task 2: Update Workspace Cargo.toml

**Files:**
- Modify: `Cargo.toml:1-192`

**Step 1: Update workspace members**

In `Cargo.toml`, change line 3:

```toml
members = [".", "oxidex-tags", "oxidex-tags-core", "oxidex-tags-camera", "oxidex-tags-media", "oxidex-tags-image", "oxidex-tags-document", "oxidex-tags-specialty"]
```

**Step 2: Update package name**

Change line 7:

```toml
name = "oxidex"
```

**Step 3: Update package description**

Change line 11:

```toml
description = "A modern, high-performance Rust implementation of ExifTool for reading, writing, and editing metadata in 300+ file formats"
```

**Step 4: Update repository URL**

Keep line 13 as-is (GitHub URL stays the same):

```toml
repository = "https://github.com/oxidex/oxidex"
```

**Step 5: Update authors**

Change line 10:

```toml
authors = ["OxiDex Contributors"]
```

**Step 6: Update library name**

Change line 19:

```toml
name = "oxidex"
```

**Step 7: Update binary name**

Change line 24:

```toml
name = "oxidex"
```

**Step 8: Update workspace dependency**

Change line 41:

```toml
oxidex-tags = { path = "oxidex-tags" }
```

**Step 9: Update profile package references**

Change lines 116, 121, 125, 129, 133, 137, 141:

```toml
[profile.dev.package.oxidex-tags]
opt-level = 2
codegen-units = 16

[profile.dev.package.oxidex-tags-core]
opt-level = 2
codegen-units = 16

[profile.dev.package.oxidex-tags-camera]
opt-level = 2
codegen-units = 16

[profile.dev.package.oxidex-tags-media]
opt-level = 2
codegen-units = 16

[profile.dev.package.oxidex-tags-image]
opt-level = 2
codegen-units = 16

[profile.dev.package.oxidex-tags-document]
opt-level = 2
codegen-units = 16

[profile.dev.package.oxidex-tags-specialty]
opt-level = 2
codegen-units = 16
```

**Step 10: Update Debian package metadata**

Change lines 164-179:

```toml
[package.metadata.deb]
maintainer = "OxiDex Contributors <oxidex@example.com>"
copyright = "2025, OxiDex Contributors"
license-file = ["LICENSE", "4"]
extended-description = """\
A modern, high-performance Rust implementation of ExifTool for reading, \
writing, and editing metadata in over 300 file formats including JPEG, PNG, \
TIFF, RAW, PDF, and many others. Provides both a command-line interface and \
library for metadata extraction and manipulation."""
depends = "$auto"
section = "utils"
priority = "optional"
assets = [
    ["target/release/oxidex", "usr/bin/", "755"],
    ["README.md", "usr/share/doc/oxidex/", "644"],
    ["LICENSE", "usr/share/doc/oxidex/", "644"],
]
```

**Step 11: Update RPM package metadata**

Change lines 184-192:

```toml
[package.metadata.generate-rpm]
assets = [
    { source = "target/release/oxidex", dest = "/usr/bin/oxidex", mode = "755" },
    { source = "README.md", dest = "/usr/share/doc/oxidex/README.md", mode = "644" },
    { source = "LICENSE", dest = "/usr/share/doc/oxidex/LICENSE", mode = "644" },
]
[package.metadata.generate-rpm.requires]
# No runtime dependencies - statically linked binary
```

**Step 12: Verify Cargo.toml syntax**

```bash
cargo metadata --format-version 1 > /dev/null
```

Expected: No errors

**Step 13: Commit workspace Cargo.toml changes**

```bash
git add Cargo.toml
git commit -m "refactor: update workspace Cargo.toml for oxidex rename"
```

---

## Task 3: Update oxidex-tags Cargo.toml

**Files:**
- Modify: `oxidex-tags/Cargo.toml:1-20`

**Step 1: Read current content**

```bash
cat oxidex-tags/Cargo.toml
```

**Step 2: Update package name**

Change the `name` field to:

```toml
name = "oxidex-tags"
```

**Step 3: Update description**

Change the `description` field to:

```toml
description = "Tag database for OxiDex metadata extraction library"
```

**Step 4: Update workspace dependencies**

Update all dependency references from `exiftool-tags-*` to `oxidex-tags-*`:

```toml
oxidex-tags-core = { path = "../oxidex-tags-core" }
oxidex-tags-camera = { path = "../oxidex-tags-camera" }
oxidex-tags-media = { path = "../oxidex-tags-media" }
oxidex-tags-image = { path = "../oxidex-tags-image" }
oxidex-tags-document = { path = "../oxidex-tags-document" }
oxidex-tags-specialty = { path = "../oxidex-tags-specialty" }
```

**Step 5: Verify changes**

```bash
cargo metadata --manifest-path oxidex-tags/Cargo.toml --format-version 1 > /dev/null
```

Expected: No errors

**Step 6: Commit**

```bash
git add oxidex-tags/Cargo.toml
git commit -m "refactor: update oxidex-tags Cargo.toml"
```

---

## Task 4: Update oxidex-tags-core Cargo.toml

**Files:**
- Modify: `oxidex-tags-core/Cargo.toml`

**Step 1: Update package name**

```toml
name = "oxidex-tags-core"
```

**Step 2: Update description**

```toml
description = "Core tag definitions for OxiDex metadata extraction"
```

**Step 3: Verify**

```bash
cargo metadata --manifest-path oxidex-tags-core/Cargo.toml --format-version 1 > /dev/null
```

**Step 4: Commit**

```bash
git add oxidex-tags-core/Cargo.toml
git commit -m "refactor: update oxidex-tags-core Cargo.toml"
```

---

## Task 5: Update oxidex-tags-camera Cargo.toml

**Files:**
- Modify: `oxidex-tags-camera/Cargo.toml`

**Step 1: Update package name**

```toml
name = "oxidex-tags-camera"
```

**Step 2: Update description**

```toml
description = "Camera-specific tag definitions for OxiDex"
```

**Step 3: Verify**

```bash
cargo metadata --manifest-path oxidex-tags-camera/Cargo.toml --format-version 1 > /dev/null
```

**Step 4: Commit**

```bash
git add oxidex-tags-camera/Cargo.toml
git commit -m "refactor: update oxidex-tags-camera Cargo.toml"
```

---

## Task 6: Update oxidex-tags-media Cargo.toml

**Files:**
- Modify: `oxidex-tags-media/Cargo.toml`

**Step 1: Update package name**

```toml
name = "oxidex-tags-media"
```

**Step 2: Update description**

```toml
description = "Media format tag definitions for OxiDex"
```

**Step 3: Verify**

```bash
cargo metadata --manifest-path oxidex-tags-media/Cargo.toml --format-version 1 > /dev/null
```

**Step 4: Commit**

```bash
git add oxidex-tags-media/Cargo.toml
git commit -m "refactor: update oxidex-tags-media Cargo.toml"
```

---

## Task 7: Update oxidex-tags-image Cargo.toml

**Files:**
- Modify: `oxidex-tags-image/Cargo.toml`

**Step 1: Update package name**

```toml
name = "oxidex-tags-image"
```

**Step 2: Update description**

```toml
description = "Image format tag definitions for OxiDex"
```

**Step 3: Verify**

```bash
cargo metadata --manifest-path oxidex-tags-image/Cargo.toml --format-version 1 > /dev/null
```

**Step 4: Commit**

```bash
git add oxidex-tags-image/Cargo.toml
git commit -m "refactor: update oxidex-tags-image Cargo.toml"
```

---

## Task 8: Update oxidex-tags-document Cargo.toml

**Files:**
- Modify: `oxidex-tags-document/Cargo.toml`

**Step 1: Update package name**

```toml
name = "oxidex-tags-document"
```

**Step 2: Update description**

```toml
description = "Document format tag definitions for OxiDex"
```

**Step 3: Verify**

```bash
cargo metadata --manifest-path oxidex-tags-document/Cargo.toml --format-version 1 > /dev/null
```

**Step 4: Commit**

```bash
git add oxidex-tags-document/Cargo.toml
git commit -m "refactor: update oxidex-tags-document Cargo.toml"
```

---

## Task 9: Update oxidex-tags-specialty Cargo.toml

**Files:**
- Modify: `oxidex-tags-specialty/Cargo.toml`

**Step 1: Update package name**

```toml
name = "oxidex-tags-specialty"
```

**Step 2: Update description**

```toml
description = "Specialty format tag definitions for OxiDex"
```

**Step 3: Verify**

```bash
cargo metadata --manifest-path oxidex-tags-specialty/Cargo.toml --format-version 1 > /dev/null
```

**Step 4: Commit**

```bash
git add oxidex-tags-specialty/Cargo.toml
git commit -m "refactor: update oxidex-tags-specialty Cargo.toml"
```

---

## Task 10: Update Rust Source Code Imports

**Files:**
- Modify: All `.rs` files in `src/` and subdirectories

**Step 1: Find all oxidex references in source code**

```bash
rg "oxidex" --type rust src/
```

Expected: List of files with references

**Step 2: Replace oxidex with oxidex in src/**

```bash
find src/ -name "*.rs" -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
```

**Step 3: Find all exiftool_tags references**

```bash
rg "exiftool_tags" --type rust src/
```

**Step 4: Replace exiftool_tags with oxidex_tags**

```bash
find src/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 5: Verify no oxidex references remain**

```bash
rg "exiftool_(rs|tags)" --type rust src/
```

Expected: No matches (only "exiftool" without underscore should remain for ExifTool compatibility references)

**Step 6: Commit source code changes**

```bash
git add src/
git commit -m "refactor: update Rust imports from oxidex to oxidex"
```

---

## Task 11: Update Tag Crate Source Code

**Files:**
- Modify: All `.rs` files in `oxidex-tags*/`

**Step 1: Update oxidex-tags crate references**

```bash
find oxidex-tags/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 2: Update oxidex-tags-core references**

```bash
find oxidex-tags-core/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 3: Update oxidex-tags-camera references**

```bash
find oxidex-tags-camera/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 4: Update oxidex-tags-media references**

```bash
find oxidex-tags-media/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 5: Update oxidex-tags-image references**

```bash
find oxidex-tags-image/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 6: Update oxidex-tags-document references**

```bash
find oxidex-tags-document/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 7: Update oxidex-tags-specialty references**

```bash
find oxidex-tags-specialty/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 8: Verify**

```bash
rg "exiftool_tags" --type rust oxidex-tags*/
```

Expected: No matches

**Step 9: Commit**

```bash
git add oxidex-tags*/
git commit -m "refactor: update tag crate imports to oxidex_tags"
```

---

## Task 12: Update Test Code

**Files:**
- Modify: All `.rs` files in `tests/`

**Step 1: Update test imports**

```bash
find tests/ -name "*.rs" -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
find tests/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 2: Verify**

```bash
rg "exiftool_(rs|tags)" --type rust tests/
```

Expected: No matches

**Step 3: Commit**

```bash
git add tests/
git commit -m "refactor: update test imports to oxidex"
```

---

## Task 13: Update Benchmark Code

**Files:**
- Modify: All `.rs` files in `benches/`

**Step 1: Update benchmark imports**

```bash
find benches/ -name "*.rs" -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
find benches/ -name "*.rs" -type f -exec sed -i '' 's/exiftool_tags/oxidex_tags/g' {} +
```

**Step 2: Verify**

```bash
rg "exiftool_(rs|tags)" --type rust benches/
```

Expected: No matches

**Step 3: Commit**

```bash
git add benches/
git commit -m "refactor: update benchmark imports to oxidex"
```

---

## Task 14: Update C FFI Bindings

**Files:**
- Modify: `bindings/`, `include/`, `cbindgen.toml`

**Step 1: Check for FFI references**

```bash
rg -i "exiftool.?rs" bindings/ include/ cbindgen.toml
```

**Step 2: Update cbindgen.toml**

Update crate name and include guard:

```toml
crate = "oxidex"
include_guard = "OXIDEX_H"
```

**Step 3: Update function prefixes in C bindings**

```bash
find bindings/ -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
find include/ -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
```

**Step 4: Rename header file if exists**

```bash
if [ -f include/oxidex.h ]; then git mv include/oxidex.h include/oxidex.h; fi
```

**Step 5: Verify**

```bash
rg "oxidex" bindings/ include/ cbindgen.toml
```

Expected: No matches

**Step 6: Commit**

```bash
git add bindings/ include/ cbindgen.toml
git commit -m "refactor: update C FFI bindings to oxidex"
```

---

## Task 15: Update Build Script

**Files:**
- Modify: `build.rs`

**Step 1: Check for hardcoded references**

```bash
rg -i "exiftool.?rs" build.rs
```

**Step 2: Replace any references**

```bash
sed -i '' 's/oxidex/oxidex/g' build.rs
sed -i '' 's/oxidex/oxidex/g' build.rs
```

**Step 3: Verify**

```bash
rg "exiftool.?rs" build.rs
```

Expected: No matches (or only comments about ExifTool source)

**Step 4: Commit**

```bash
git add build.rs
git commit -m "refactor: update build.rs references to oxidex"
```

---

## Task 16: Update README.md

**Files:**
- Modify: `README.md`

**Step 1: Update title**

Replace line 1:

```markdown
# OxiDex
```

**Step 2: Update badges**

Update GitHub Actions badge URLs (lines 3-4) - keep repo URL as-is:

```markdown
[![CI](https://github.com/oxidex/oxidex/workflows/CI/badge.svg)](https://github.com/oxidex/oxidex/actions)
[![Integration Tests](https://github.com/oxidex/oxidex/workflows/Integration%20Tests%20(ExifTool%20Comparison)/badge.svg)](https://github.com/oxidex/oxidex/actions)
```

**Step 3: Update project description**

Replace line 6:

```markdown
A modern, high-performance Rust implementation of the industry-standard [ExifTool](https://exiftool.org/) metadata management library and command-line application.
```

**Step 4: Update Project Vision section**

Replace lines 8-10:

```markdown
## Project Vision

OxiDex aims to provide a memory-safe, zero-cost abstraction alternative to the Perl-based ExifTool while maintaining full compatibility with its extensive metadata tag support. The goal is to deliver superior performance, native cross-compilation capabilities, and seamless integration into modern software ecosystems.
```

**Step 5: Update installation instructions**

Replace all instances of:
- `cargo install oxidex` → `cargo install oxidex`
- `oxidex` command → `oxidex`
- Binary names in download URLs

**Step 6: Update usage examples**

Replace all CLI examples:

```bash
# Extract all metadata from a file
oxidex photo.jpg

# Extract specific tags
oxidex -Make -Model -DateTimeOriginal photo.jpg

# Write metadata
oxidex -Artist="Your Name" photo.jpg

# Batch processing (recursive)
oxidex -r /path/to/photos/

# JSON output
oxidex -json photo.jpg

# CSV output for batch analysis
oxidex -csv -r /path/to/photos/ > metadata.csv

# Copy metadata between files
oxidex -TagsFromFile source.jpg target.jpg

# Date shifting (adjust all timestamps by offset)
oxidex "-DateTimeOriginal+=1:0:0 0:0:0" photo.jpg

# Extract Canon-specific metadata (for Canon cameras)
oxidex -Canon:FirmwareVersion -Canon:SerialNumber -Canon:OwnerName canon_photo.jpg
```

**Step 7: Update library API examples**

Replace Rust code example:

```rust
use oxidex::core::MetadataMap;

// Extract metadata from a file
let metadata = MetadataMap::from_file("photo.jpg")?;
println!("Camera: {}", metadata.get("Make")?);
println!("Date: {}", metadata.get("DateTimeOriginal")?);

// Extract Canon-specific tags (if applicable)
if let Ok(firmware) = metadata.get("Canon:FirmwareVersion") {
    println!("Canon Firmware: {}", firmware);
}
if let Ok(serial) = metadata.get("Canon:SerialNumber") {
    println!("Camera Serial: {}", serial);
}

// Edit and write metadata
metadata.set("Artist", "Your Name")?;
metadata.write_to_file("photo.jpg")?;
```

**Step 8: Update download URLs**

Replace binary artifact names in download section:

```markdown
- **Linux** (x86_64): `oxidex-x86_64-linux-musl.tar.gz`
- **Linux** (ARM64): `oxidex-aarch64-linux-musl.tar.gz`
- **macOS** (Intel): `oxidex-x86_64-macos.tar.gz`
- **macOS** (Apple Silicon): `oxidex-aarch64-macos.tar.gz`
- **Windows** (x86_64): `oxidex-x86_64-windows.zip`
```

And example:

```bash
# Example: Install on Linux (x86_64)
wget https://github.com/oxidex/oxidex/releases/download/v1.0.0/oxidex-x86_64-linux-musl.tar.gz
tar xzf oxidex-x86_64-linux-musl.tar.gz
sudo mv oxidex /usr/local/bin/
oxidex --version
```

**Step 9: Update Homebrew formula reference**

```bash
# Install from Homebrew formula (source build)
brew install --build-from-source https://raw.githubusercontent.com/oxidex/oxidex/main/packaging/homebrew/oxidex.rb

# Or install from local formula file
brew install --build-from-source ./packaging/homebrew/oxidex.rb

# Verify installation
oxidex --version
```

**Step 10: Update from source build instructions**

```bash
# Build the project
cargo build --release

# Run
./target/release/oxidex

# Optional: Install to system path
cargo install --path .
```

**Step 11: Update Technology Stack section**

Keep ExifTool reference but update project name context as needed.

**Step 12: Update footer**

```markdown
**Status**: Stable Release
**Current Version**: 1.0.0
**License**: GPL-3.0
**Documentation**: [User Guide](https://oxidex.github.io/oxidex/) | [API Docs](https://docs.rs/oxidex)
**Issues**: [GitHub Issues](https://github.com/oxidex/oxidex/issues)
```

**Step 13: Commit**

```bash
git add README.md
git commit -m "docs: rebrand README to OxiDex"
```

---

## Task 17: Update CHANGELOG.md

**Files:**
- Modify: `CHANGELOG.md`

**Step 1: Add rename entry at top**

Add new section after header:

```markdown
## [Unreleased]

### Changed
- **BREAKING**: Project renamed from `oxidex` to `oxidex`
  - Binary renamed: `oxidex` → `oxidex`
  - Library renamed: `oxidex` → `oxidex`
  - All crates renamed: `exiftool-tags*` → `oxidex-tags*`
  - Install with: `cargo install oxidex`
  - GitHub repository URL unchanged: `oxidex/oxidex`

---
```

**Step 2: Update historical references**

Keep historical references to "oxidex" in older changelog entries for accuracy, but add a note at the top:

```markdown
# Changelog

> **Note**: This project was renamed from `oxidex` to `oxidex`. Historical entries below use the old name.
```

**Step 3: Commit**

```bash
git add CHANGELOG.md
git commit -m "docs: add rename entry to CHANGELOG"
```

---

## Task 18: Update Other Documentation Files

**Files:**
- Modify: `PACKAGING.md`, `RELEASE_CHECKLIST.md`, `RELEASE_ANNOUNCEMENT.md`

**Step 1: Update PACKAGING.md**

```bash
sed -i '' 's/oxidex/oxidex/g' PACKAGING.md
sed -i '' 's/oxidex/oxidex/g' PACKAGING.md
sed -i '' 's/OxiDex/OxiDex/g' PACKAGING.md
```

**Step 2: Update RELEASE_CHECKLIST.md**

```bash
sed -i '' 's/oxidex/oxidex/g' RELEASE_CHECKLIST.md
sed -i '' 's/oxidex/oxidex/g' RELEASE_CHECKLIST.md
sed -i '' 's/OxiDex/OxiDex/g' RELEASE_CHECKLIST.md
```

**Step 3: Update RELEASE_ANNOUNCEMENT.md**

```bash
sed -i '' 's/oxidex/oxidex/g' RELEASE_ANNOUNCEMENT.md
sed -i '' 's/oxidex/oxidex/g' RELEASE_ANNOUNCEMENT.md
sed -i '' 's/OxiDex/OxiDex/g' RELEASE_ANNOUNCEMENT.md
```

**Step 4: Verify**

```bash
rg -i "exiftool.?rs" PACKAGING.md RELEASE_CHECKLIST.md RELEASE_ANNOUNCEMENT.md
```

Expected: No matches (only "ExifTool" references should remain)

**Step 5: Commit**

```bash
git add PACKAGING.md RELEASE_CHECKLIST.md RELEASE_ANNOUNCEMENT.md
git commit -m "docs: update packaging and release docs to OxiDex"
```

---

## Task 19: Update docs/ Directory

**Files:**
- Modify: All `.md` files in `docs/`

**Step 1: Find all markdown files in docs**

```bash
find docs/ -name "*.md" -type f
```

**Step 2: Replace oxidex references**

```bash
find docs/ -name "*.md" -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
find docs/ -name "*.md" -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
find docs/ -name "*.md" -type f -exec sed -i '' 's/OxiDex/OxiDex/g' {} +
```

**Step 3: Verify**

```bash
rg -i "exiftool.?rs" docs/ --type md
```

Expected: No matches (only "ExifTool" should remain)

**Step 4: Commit**

```bash
git add docs/
git commit -m "docs: update docs/ to OxiDex branding"
```

---

## Task 20: Update GitHub Workflows - CI

**Files:**
- Modify: `.github/workflows/ci.yml`

**Step 1: Update binary artifact names**

Find and replace all instances of `oxidex` with `oxidex` in artifact names and paths.

**Step 2: Update job names and descriptions**

Update any job display names that reference the old name.

**Step 3: Update cache keys if they include crate names**

**Step 4: Commit**

```bash
git add .github/workflows/ci.yml
git commit -m "ci: update CI workflow for oxidex rename"
```

---

## Task 21: Update GitHub Workflows - Release

**Files:**
- Modify: `.github/workflows/release.yml`

**Step 1: Update binary output names**

Replace all artifact names:
- `oxidex-x86_64-linux-musl` → `oxidex-x86_64-linux-musl`
- `oxidex-aarch64-linux-musl` → `oxidex-aarch64-linux-musl`
- `oxidex-x86_64-macos` → `oxidex-x86_64-macos`
- `oxidex-aarch64-macos` → `oxidex-aarch64-macos`
- `oxidex-x86_64-windows` → `oxidex-x86_64-windows`

**Step 2: Update release asset names**

**Step 3: Update checksum file references**

**Step 4: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: update release workflow for oxidex rename"
```

---

## Task 22: Update Other GitHub Workflows

**Files:**
- Modify: All other `.yml` files in `.github/workflows/`

**Step 1: List all workflows**

```bash
ls .github/workflows/
```

**Step 2: Update each workflow file**

```bash
find .github/workflows/ -name "*.yml" -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
```

**Step 3: Verify**

```bash
rg "oxidex" .github/workflows/
```

Expected: No matches (only repository URL should remain)

**Step 4: Commit**

```bash
git add .github/workflows/
git commit -m "ci: update all workflows for oxidex rename"
```

---

## Task 23: Update Homebrew Formula

**Files:**
- Rename: `packaging/homebrew/oxidex.rb` → `packaging/homebrew/oxidex.rb`
- Modify: `packaging/homebrew/oxidex.rb`

**Step 1: Rename formula file**

```bash
git mv packaging/homebrew/oxidex.rb packaging/homebrew/oxidex.rb
```

**Step 2: Update formula class name**

Change `class ExiftoolRs` to `class Oxidex`

**Step 3: Update description**

```ruby
desc "Modern, high-performance Rust implementation of ExifTool for metadata extraction"
```

**Step 4: Update binary installation**

Change binary name from `oxidex` to `oxidex`

**Step 5: Update homepage if needed**

Keep repository URL as-is.

**Step 6: Commit**

```bash
git add packaging/homebrew/
git commit -m "build: update Homebrew formula for oxidex"
```

---

## Task 24: Update Debian/RPM Packaging Scripts

**Files:**
- Modify: Any packaging scripts in `packaging/`

**Step 1: Check for packaging scripts**

```bash
find packaging/ -type f ! -name "*.rb"
```

**Step 2: Update references in all packaging files**

```bash
find packaging/ -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
find packaging/ -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
```

**Step 3: Verify**

```bash
rg "exiftool.?rs" packaging/
```

Expected: No matches

**Step 4: Commit if changes made**

```bash
git add packaging/
git commit -m "build: update packaging scripts for oxidex"
```

---

## Task 25: Update Scripts Directory

**Files:**
- Modify: All scripts in `scripts/`

**Step 1: List scripts**

```bash
find scripts/ -type f
```

**Step 2: Update script contents**

```bash
find scripts/ -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
find scripts/ -type f -exec sed -i '' 's/oxidex/oxidex/g' {} +
```

**Step 3: Verify**

```bash
rg "exiftool.?rs" scripts/
```

Expected: No matches

**Step 4: Commit**

```bash
git add scripts/
git commit -m "build: update scripts for oxidex rename"
```

---

## Task 26: Update Justfile

**Files:**
- Modify: `justfile`

**Step 1: Update binary references**

```bash
sed -i '' 's/oxidex/oxidex/g' justfile
sed -i '' 's/oxidex/oxidex/g' justfile
```

**Step 2: Verify**

```bash
rg "exiftool.?rs" justfile
```

Expected: No matches

**Step 3: Commit**

```bash
git add justfile
git commit -m "build: update justfile for oxidex"
```

---

## Task 27: Update Cross.toml

**Files:**
- Modify: `Cross.toml`

**Step 1: Check for references**

```bash
rg -i "exiftool" Cross.toml
```

**Step 2: Update if needed**

```bash
sed -i '' 's/oxidex/oxidex/g' Cross.toml
```

**Step 3: Commit if changed**

```bash
git add Cross.toml
git commit -m "build: update Cross.toml for oxidex"
```

---

## Task 28: Update Fuzz Targets

**Files:**
- Modify: `fuzz/Cargo.toml` and fuzz targets in `fuzz/fuzz_targets/`

**Step 1: Update fuzz Cargo.toml**

```bash
sed -i '' 's/oxidex/oxidex/g' fuzz/Cargo.toml
sed -i '' 's/oxidex/oxidex/g' fuzz/Cargo.toml
```

**Step 2: Update fuzz target sources**

```bash
find fuzz/fuzz_targets/ -name "*.rs" -exec sed -i '' 's/oxidex/oxidex/g' {} +
```

**Step 3: Verify**

```bash
rg "exiftool.?rs" fuzz/
```

Expected: No matches

**Step 4: Commit**

```bash
git add fuzz/
git commit -m "test: update fuzz targets for oxidex"
```

---

## Task 29: Clean Build Test

**Files:**
- Test: Full workspace build

**Step 1: Clean all build artifacts**

```bash
cargo clean
```

Expected: `target/` directory cleaned

**Step 2: Update Cargo.lock**

```bash
cargo generate-lockfile
```

Expected: New lockfile generated with oxidex crate names

**Step 3: Build workspace**

```bash
cargo build --all
```

Expected: All 8 crates build successfully

**Step 4: Verify binary name**

```bash
ls -lh target/debug/oxidex
```

Expected: `oxidex` binary exists

**Step 5: Check binary version output**

```bash
./target/debug/oxidex --version
```

Expected: Version output (may still show old name in version string - will fix in next task)

**Step 6: Commit Cargo.lock**

```bash
git add Cargo.lock
git commit -m "build: update Cargo.lock for oxidex rename"
```

---

## Task 30: Update Version Strings in Code

**Files:**
- Modify: Source files with hardcoded version/name strings

**Step 1: Find version string locations**

```bash
rg -i "exiftool.?rs.*version" src/ --type rust
```

**Step 2: Update CLI version output**

Find and update any hardcoded project name in version output (likely in `src/main.rs` or similar).

**Step 3: Update help text**

Search for and update help text that mentions the old name.

**Step 4: Test version output again**

```bash
./target/debug/oxidex --version
```

Expected: Shows "oxidex" not "oxidex"

**Step 5: Commit**

```bash
git add src/
git commit -m "refactor: update version strings to OxiDex"
```

---

## Task 31: Run Full Test Suite

**Files:**
- Test: All tests

**Step 1: Run all unit tests**

```bash
cargo test --all
```

Expected: All tests pass

**Step 2: Run doc tests**

```bash
cargo test --doc --all
```

Expected: All doc tests pass

**Step 3: Run integration tests**

```bash
cargo test --test '*'
```

Expected: All integration tests pass

**Step 4: Check for test failures**

If any tests fail, investigate and fix. Common issues:
- Hardcoded paths with old name
- Test assertions checking for old binary name
- Test fixtures with old names

**Step 5: Commit any test fixes**

```bash
git add tests/
git commit -m "test: fix tests after oxidex rename"
```

---

## Task 32: Build Release Binary and Test

**Files:**
- Test: Release build

**Step 1: Build release binary**

```bash
cargo build --release
```

Expected: Release build succeeds

**Step 2: Verify binary exists**

```bash
ls -lh target/release/oxidex
```

**Step 3: Test basic functionality**

```bash
# Create a test file if needed, or use existing test image
./target/release/oxidex tests/fixtures/test.jpg
```

Expected: Metadata output displayed

**Step 4: Test JSON output**

```bash
./target/release/oxidex -json tests/fixtures/test.jpg
```

Expected: Valid JSON output

**Step 5: Test help**

```bash
./target/release/oxidex --help
```

Expected: Help text displays with "oxidex" branding

---

## Task 33: Verify Documentation Builds

**Files:**
- Test: Cargo doc

**Step 1: Build all documentation**

```bash
cargo doc --all --no-deps
```

Expected: Documentation builds without errors

**Step 2: Check for broken links**

```bash
cargo doc --all --no-deps 2>&1 | grep -i "warning.*intra-doc"
```

Expected: No intra-doc link warnings

**Step 3: Open documentation to verify**

```bash
open target/doc/oxidex/index.html
```

Expected: Documentation displays correctly with OxiDex branding

---

## Task 34: Search for Remaining References

**Files:**
- Verify: All files

**Step 1: Search for oxidex in all files**

```bash
rg -i "exiftool.?rs" --type-not lock
```

**Step 2: Review results**

Check each match to ensure:
- It's not in a comment about the rename
- It's not in CHANGELOG history (which should preserve old name)
- It's not a legitimate reference to GitHub URLs (which we're keeping)
- It's not a reference to "ExifTool" (the Perl tool we're compatible with)

**Step 3: Fix any remaining issues**

Update any files that slipped through.

**Step 4: Commit fixes**

```bash
git add -A
git commit -m "refactor: fix remaining oxidex references"
```

---

## Task 35: Final Verification Commit

**Files:**
- Verify: All changes

**Step 1: Check git status**

```bash
git status
```

Expected: Clean working directory or only expected changes

**Step 2: Review all commits**

```bash
git log --oneline main..HEAD
```

Expected: Series of logical commits for the rename

**Step 3: Verify no untracked files**

```bash
git status --short
```

Expected: No untracked files with old names

**Step 4: Create summary of changes**

```bash
git diff --stat main..HEAD
```

Review the diffstat to ensure all expected files were modified.

---

## Post-Rename Checklist

After completing all tasks:

- [ ] All 8 crates renamed
- [ ] All Cargo.toml files updated
- [ ] All Rust source code updated
- [ ] All tests passing
- [ ] Documentation builds successfully
- [ ] Release binary builds and runs
- [ ] CI/CD workflows updated
- [ ] Package metadata updated
- [ ] No remaining "oxidex" or "oxidex" references (except approved locations)
- [ ] Git history clean with logical commits

## Next Steps

After merge to main:

1. Tag release: `git tag v1.1.0` (or appropriate version)
2. Push tag: `git push origin v1.1.0`
3. Verify GitHub Actions release workflow runs
4. Publish crates to crates.io in dependency order
5. Deprecate old `oxidex` crate on crates.io
6. Update project documentation/website
7. Announce rename to community

## Notes

- **IMPORTANT**: Keep all references to "ExifTool" (the Perl tool) - we're compatible with it
- **IMPORTANT**: Keep GitHub repository URL as `oxidex/oxidex`
- **IMPORTANT**: Verify each change doesn't break functionality
- Test thoroughly at each major milestone
- Commit frequently with clear messages
