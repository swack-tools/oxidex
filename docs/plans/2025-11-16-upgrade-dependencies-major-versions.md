# Upgrade Dependencies to Latest Major Versions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Upgrade all dependencies to their latest major versions including nom 8.0, quick-xml 0.38, indicatif 0.18, criterion 0.7, and bincode 2.0 across the entire workspace

**Architecture:** Incremental upgrade approach - update one major version dependency at a time, test after each upgrade, commit working changes. This minimizes debugging surface area and creates atomic, revertible commits.

**Tech Stack:** Rust workspace with 9 crates (main + 7 exiftool-tags-* + fuzz), cargo for dependency management, bincode for serialization

---

## Pre-Upgrade Analysis

**Current Workspace Structure:**
- Root: `./Cargo.toml` (main crate)
- Tags: `./exiftool-tags/Cargo.toml`
- Tag modules: 6 crates in `./exiftool-tags-*/Cargo.toml`
- Fuzz: `./fuzz/Cargo.toml`

**Dependencies to Upgrade:**
1. **criterion** 0.5 → 0.7 (dev-dependency, lowest risk)
2. **indicatif** 0.17 → 0.18 (production, UI library)
3. **nom** 7.1 → 8.0 (production, parser combinator - moderate breaking changes expected)
4. **quick-xml** 0.31 → 0.38 (production, XML parser - API changes expected)
5. **bincode** 1.3 → 2.0 (build-dependency across 6 workspace crates - major API overhaul)

---

## Task 1: Upgrade criterion (Lowest Risk - Dev Dependency)

**Files:**
- Modify: `./Cargo.toml:96`

**Step 1: Update Cargo.toml**

In `./Cargo.toml`, update the criterion version:

```toml
[dev-dependencies]
# Testing
proptest = "1.9"
criterion = "0.7"  # Changed from 0.5
cc = "1.2"
```

**Step 2: Regenerate Cargo.lock**

```bash
cargo update -p criterion
```

Expected: Updates criterion and its dependencies

**Step 3: Check for breaking changes**

Check criterion changelog: https://github.com/bheisler/criterion.rs/blob/master/CHANGELOG.md

Common breaking changes in 0.7:
- Removed deprecated APIs
- Changed measurement API
- Updated benchmark macro syntax (if used)

**Step 4: Run benchmarks to verify**

```bash
cargo bench --no-fail-fast
```

Expected: All benchmarks compile and run successfully

If failures occur, check benchmark files in `benches/` directory and update API calls per criterion 0.7 migration guide.

**Step 5: Run full test suite**

```bash
cargo test --release --all-features
```

Expected: All tests pass (criterion is dev-dependency, shouldn't affect tests)

**Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: upgrade criterion from 0.5 to 0.7

Updated dev-dependency criterion to latest version 0.7.0.
All benchmarks verified working with new version.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 2: Upgrade indicatif (UI Library - Moderate Risk)

**Files:**
- Modify: `./Cargo.toml:68`

**Step 1: Update Cargo.toml**

In `./Cargo.toml`, update indicatif version:

```toml
# Progress Bar (optional but recommended)
indicatif = "0.18"  # Changed from 0.17
```

**Step 2: Regenerate Cargo.lock**

```bash
cargo update -p indicatif
```

**Step 3: Check for breaking changes**

Check release notes: https://github.com/console-rs/indicatif/releases

Common breaking changes in 0.18:
- `ProgressBar::new()` may require different parameters
- Style API changes
- `ProgressDrawTarget` changes

**Step 4: Search for indicatif usage in codebase**

```bash
rg "indicatif::" --type rust
rg "ProgressBar" --type rust
```

**Step 5: Build and check for compilation errors**

```bash
cargo build --release --all-features 2>&1 | tee /tmp/build-log.txt
```

**Step 6: Fix any compilation errors**

If errors occur, common fixes:
- Update `ProgressBar::new()` calls to `ProgressBar::new(len)`
- Update `ProgressStyle` builder methods
- Check `MultiProgress` API changes

**Step 7: Test progress bar functionality**

```bash
# Run a command that uses progress bars
cargo run --release -- /path/to/test/images/*.jpg
```

Verify: Progress bars display correctly

**Step 8: Run full test suite**

```bash
cargo test --release --all-features
```

Expected: All tests pass

**Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: upgrade indicatif from 0.17 to 0.18

Updated progress bar library to latest version.
Verified progress bar display in CLI operations.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 3: Upgrade nom (Parser Combinator - Higher Risk)

**Files:**
- Modify: `./Cargo.toml:43`
- Potentially modify: Multiple parser files in `src/parsers/**/*.rs`

**Step 1: Update Cargo.toml**

In `./Cargo.toml`, update nom version:

```toml
# Binary Parsing
nom = "8.0"  # Changed from 7.1
```

**Step 2: Regenerate Cargo.lock**

```bash
cargo update -p nom
```

**Step 3: Review nom 8.0 breaking changes**

Read migration guide: https://github.com/rust-bakery/nom/blob/main/doc/upgrading_to_nom_8.md

Major changes in nom 8.0:
- Error types changed (ParseError trait)
- Some combinators renamed
- `IResult` type signature may differ
- Streaming parsers API changes

**Step 4: Build and capture compilation errors**

```bash
cargo build --release --all-features 2>&1 | tee /tmp/nom-errors.txt
```

**Step 5: Fix compilation errors systematically**

Common fixes needed:
1. Update error types: `nom::error::Error<&str>` to new error types
2. Update combinator imports if renamed
3. Fix `IResult` type annotations
4. Update custom parser error handling

Example error fix pattern:
```rust
// OLD (nom 7)
use nom::error::{Error, ErrorKind};
fn parser(input: &[u8]) -> IResult<&[u8], Output, Error<&[u8]>> { ... }

// NEW (nom 8)
use nom::error::{Error, ErrorKind};
fn parser(input: &[u8]) -> IResult<&[u8], Output> { ... }
```

**Step 6: Run parser-specific tests**

```bash
cargo test --release parsers::
```

Expected: All parser tests pass

**Step 7: Run full test suite**

```bash
cargo test --release --all-features
```

Expected: All 800+ tests pass

**Step 8: Test with real files**

```bash
cargo run --release -- test-images/sample.jpg
cargo run --release -- test-images/sample.png
cargo run --release -- test-images/sample.pdf
```

Verify: Metadata extraction works correctly

**Step 9: Commit**

```bash
git add Cargo.toml Cargo.lock src/
git commit -m "chore: upgrade nom from 7.1 to 8.0

Updated parser combinator library to nom 8.0.
Fixed compilation errors related to error types and combinator APIs.
All parser tests verified passing.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 4: Upgrade quick-xml (XML Parser - Moderate Risk)

**Files:**
- Modify: `./Cargo.toml:46`
- Potentially modify: `src/parsers/xmp/*.rs`, `src/parsers/jpeg/xmp_parser.rs`

**Step 1: Update Cargo.toml**

In `./Cargo.toml`, update quick-xml version:

```toml
# XML Parsing (for XMP metadata)
quick-xml = "0.38"  # Changed from 0.31
```

**Step 2: Regenerate Cargo.lock**

```bash
cargo update -p quick-xml
```

**Step 3: Review quick-xml breaking changes**

Check releases: https://github.com/tafia/quick-xml/releases

Breaking changes 0.31 → 0.38:
- Reader API changes (bytes vs. text methods)
- Event enum variants changed
- Namespace handling updates
- Writer API modifications

**Step 4: Find XMP/XML parsing code**

```bash
rg "quick_xml::" --type rust
rg "Reader::from" --type rust
```

**Step 5: Build and capture errors**

```bash
cargo build --release --all-features 2>&1 | tee /tmp/xml-errors.txt
```

**Step 6: Fix compilation errors**

Common fixes:
1. Update `Reader::from_str()` to `Reader::from_reader()`
2. Change `Event::Text` handling (now uses `BytesText`)
3. Update attribute reading: `.value()` → `.decode_and_unescape_value()`
4. Fix namespace resolution APIs

Example fix:
```rust
// OLD (0.31)
use quick_xml::Reader;
let mut reader = Reader::from_str(xml_str);

// NEW (0.38)
use quick_xml::Reader;
let mut reader = Reader::from_reader(xml_str.as_bytes());
```

**Step 7: Run XMP parser tests**

```bash
cargo test --release xmp
cargo test --release xml
```

**Step 8: Test with XMP-containing files**

```bash
cargo run --release -- test-images/with-xmp.jpg
```

Verify: XMP metadata extracted correctly

**Step 9: Run full test suite**

```bash
cargo test --release --all-features
```

**Step 10: Commit**

```bash
git add Cargo.toml Cargo.lock src/parsers/
git commit -m "chore: upgrade quick-xml from 0.31 to 0.38

Updated XML parser library for XMP metadata handling.
Migrated Reader and Event APIs to 0.38 interface.
All XMP parsing tests verified passing.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 5: Upgrade bincode (Highest Risk - Major API Overhaul Across 6 Crates)

**Files:**
- Modify: `./exiftool-tags-camera/Cargo.toml:12,13`
- Modify: `./exiftool-tags-core/Cargo.toml:12,13`
- Modify: `./exiftool-tags-document/Cargo.toml:12,13`
- Modify: `./exiftool-tags-image/Cargo.toml:12,13`
- Modify: `./exiftool-tags-media/Cargo.toml:12,13`
- Modify: `./exiftool-tags-specialty/Cargo.toml:12,13`
- Potentially modify: Build scripts in `exiftool-tags-*/build.rs`

**Step 1: Review bincode 2.0 migration guide**

Read: https://github.com/bincode-org/bincode/blob/trunk/docs/migration_guide.md

Major changes:
- New API: `bincode::encode()`/`decode()` instead of `serialize()`/`deserialize()`
- Configuration system completely redesigned
- Encoding format may differ (check compatibility)
- Derive macro changes for custom types

**Step 2: Update all workspace Cargo.toml files**

Update bincode version in 6 files:

`./exiftool-tags-camera/Cargo.toml`:
```toml
[dependencies]
bincode = "2.0"  # Changed from 1.3

[build-dependencies]
bincode = "2.0"  # Changed from 1.3
```

Repeat for:
- `./exiftool-tags-core/Cargo.toml`
- `./exiftool-tags-document/Cargo.toml`
- `./exiftool-tags-image/Cargo.toml`
- `./exiftool-tags-media/Cargo.toml`
- `./exiftool-tags-specialty/Cargo.toml`

**Step 3: Regenerate Cargo.lock**

```bash
cargo update -p bincode
```

**Step 4: Find bincode usage in build scripts**

```bash
rg "bincode::" exiftool-tags-*/build.rs
rg "serialize" exiftool-tags-*/build.rs
rg "deserialize" exiftool-tags-*/build.rs
```

**Step 5: Build and capture errors**

```bash
cargo build --release --all-features 2>&1 | tee /tmp/bincode-errors.txt
```

**Step 6: Update build.rs files systematically**

For each `exiftool-tags-*/build.rs`, update serialization calls:

```rust
// OLD (bincode 1.3)
use bincode::serialize;
let encoded = serialize(&tags_data)?;

// NEW (bincode 2.0)
use bincode::{Encode, Decode};
use bincode::config::standard;
let encoded = bincode::encode_to_vec(&tags_data, standard())?;
```

**Step 7: Check data structures have Encode/Decode derives**

If custom structs are serialized, add derives:

```rust
// OLD (bincode 1.3)
#[derive(Serialize, Deserialize)]
struct TagData { ... }

// NEW (bincode 2.0)
#[derive(Encode, Decode)]
struct TagData { ... }
```

**Step 8: Rebuild to verify build scripts**

```bash
cargo clean
cargo build --release --all-features
```

Expected: Build scripts compile YAML to binary successfully

**Step 9: Verify generated tag files**

```bash
ls -lh exiftool-tags-*/generated_tags.rs
```

Expected: Files generated with correct sizes (~50KB to 1MB each)

**Step 10: Run tag database tests**

```bash
cargo test --release tag_
cargo test --release exiftool_tags
```

**Step 11: Verify binary compatibility (IMPORTANT)**

Test that existing binary tag files can still be loaded:

```bash
# Check if generated files need regeneration
cargo clean
rm exiftool-tags-*/generated_tags.rs
cargo build --release --all-features
```

**Step 12: Run full workspace test suite**

```bash
cargo test --release --all-features --workspace
```

Expected: All 800+ tests pass across all workspace crates

**Step 13: Integration test with tag database**

```bash
cargo run --release -- --help  # Verify tag database loads
cargo run --release -- test-images/sample.jpg  # Extract tags
```

Verify: All tag names/values display correctly

**Step 14: Commit**

```bash
git add */Cargo.toml Cargo.lock exiftool-tags-*/build.rs
git commit -m "chore: upgrade bincode from 1.3 to 2.0 across workspace

Updated binary serialization library to bincode 2.0 in all 6 tag crates.
Migrated build scripts to new encode/decode API.
Regenerated all tag database binary files.
All workspace tests verified passing.

Breaking changes addressed:
- Updated serialize/deserialize to encode/decode API
- Added Encode/Decode derives to tag data structures
- Updated configuration to use standard() config

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 6: Final Verification & Documentation

**Step 1: Clean build from scratch**

```bash
cargo clean
rm -rf target/
cargo build --release --all-features --workspace
```

Expected: Clean build succeeds

**Step 2: Run complete test suite**

```bash
cargo test --release --all-features --workspace -- --nocapture
```

Expected: All tests pass

**Step 3: Run benchmarks**

```bash
cargo bench --no-fail-fast
```

Expected: All benchmarks complete

**Step 4: Test CLI with multiple file formats**

```bash
cargo run --release -- test-images/*.jpg
cargo run --release -- test-images/*.png
cargo run --release -- test-images/*.pdf
cargo run --release -- test-images/*.tiff
```

Verify: Metadata extracted correctly for all formats

**Step 5: Check for deprecation warnings**

```bash
cargo build --release --all-features 2>&1 | grep -i "warning.*deprecated"
```

Address any deprecation warnings found

**Step 6: Update CHANGELOG.md**

Add entry documenting all upgraded dependencies:

```markdown
## [Unreleased]

### Changed
- Upgraded criterion from 0.5 to 0.7 for benchmarking
- Upgraded indicatif from 0.17 to 0.18 for progress bars
- Upgraded nom from 7.1 to 8.0 for binary parsing
- Upgraded quick-xml from 0.31 to 0.38 for XMP parsing
- Upgraded bincode from 1.3 to 2.0 across all tag workspace crates

### Migration Notes
- Bincode 2.0 changes binary format - regenerated all tag database files
- Nom 8.0 updates parser error types - custom parsers may need updates
- Quick-xml 0.38 changes Reader API - XMP parser updated accordingly
```

**Step 7: Update documentation if needed**

Check if any docs reference specific dependency versions:

```bash
rg "nom 7" docs/
rg "bincode 1" docs/
```

**Step 8: Final commit**

```bash
git add CHANGELOG.md docs/
git commit -m "docs: update changelog for dependency upgrades

Documented all major version dependency upgrades completed.

🤖 Generated with [Claude Code](https://claude.com/claude-code)

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Task 7: CI Verification

**Step 1: Push changes**

```bash
git log --oneline -7  # Review commits
git push origin main
```

**Step 2: Monitor GitHub Actions**

Watch CI runs for:
- ✅ Build (all platforms)
- ✅ Test Suite
- ✅ Integration Tests
- ✅ Clippy Lints
- ✅ Benchmarks

**Step 3: If CI fails**

1. Review failure logs
2. Reproduce locally: `cargo test --release --features exiftool-comparison`
3. Fix issues
4. Commit fixes
5. Push and verify

---

## Rollback Plan (If Needed)

If critical issues found after deployment:

**Option 1: Revert specific dependency**

```bash
# Revert just one problematic upgrade
git revert <commit-hash-of-upgrade>
cargo update
cargo test --release --all-features
git push
```

**Option 2: Full rollback**

```bash
# Revert all upgrades
git revert HEAD~7..HEAD  # Revert last 7 commits
cargo update
cargo test --release --all-features
git push
```

---

## Success Criteria

- [ ] All 5 major dependencies upgraded
- [ ] All workspace Cargo.toml files updated
- [ ] Cargo.lock regenerated
- [ ] All 800+ tests passing
- [ ] All benchmarks running
- [ ] CLI functionality verified with real files
- [ ] No compilation warnings
- [ ] CI passing on all platforms
- [ ] Documentation updated
- [ ] Commits follow atomic upgrade pattern

---

## Estimated Time

- Task 1 (criterion): 15 minutes
- Task 2 (indicatif): 20 minutes
- Task 3 (nom): 45-60 minutes (potential parser fixes)
- Task 4 (quick-xml): 30-45 minutes (XMP parser updates)
- Task 5 (bincode): 60-90 minutes (6 crates + build scripts)
- Task 6 (verification): 30 minutes
- Task 7 (CI): 15 minutes monitoring

**Total: 3-4 hours** (includes debugging time for breaking changes)

---

## Notes

1. **Do upgrades in order** - low risk to high risk allows early detection of issues
2. **Test after each upgrade** - isolates breaking changes to specific dependency
3. **Commit atomically** - makes rollback surgical if needed
4. **Watch for cascading failures** - one dependency may affect another
5. **Bincode is highest risk** - binary format change affects 6 workspace crates
6. **Keep clap pinned** - don't upgrade clap (already fixed at 4.5.49)
