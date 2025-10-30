# v1.0.0 Release Checklist

This document outlines the remaining manual steps to complete the v1.0.0 release.

## ✅ Completed Steps

- [x] Audit all TODOs and FIXMEs (3 found, documented as known limitations)
- [x] Create comprehensive CHANGELOG.md with v1.0.0 features
- [x] Bump version to 1.0.0 in Cargo.toml
- [x] Update README.md for v1.0.0 release
- [x] Update documentation book intro (docs/book/src/intro.md)
- [x] Verify cargo publish dry-run succeeds
- [x] Fix build.rs to skip generation if generated_tags.rs exists
- [x] Commit generated_tags.rs for crates.io publishing
- [x] Draft comprehensive release announcement (RELEASE_ANNOUNCEMENT.md)
- [x] All changes committed to git (3 commits total)

## 🚀 Remaining Manual Steps

### Step 1: Publish to crates.io

**IMPORTANT**: You need a crates.io account and API token to publish.

1. Create account at https://crates.io if you don't have one
2. Get your API token from https://crates.io/me
3. Login to cargo:
   ```bash
   cargo login <your-api-token>
   ```

4. Publish the crate:
   ```bash
   cargo publish --allow-dirty
   ```

   Note: Use `--allow-dirty` because .codemachine files are modified but not part of the package.

5. Verify successful publish at https://crates.io/crates/exiftool-rs

6. Test installation:
   ```bash
   cargo install exiftool-rs
   exiftool-rs --version  # Should show 1.0.0
   ```

### Step 2: Create and Push v1.0.0 Git Tag

After successful crates.io publish:

```bash
# Create annotated tag with release notes
git tag -a v1.0.0 -m "ExifTool-RS v1.0.0 Stable Release

See CHANGELOG.md for full details.

Highlights:
- 50+ format support (JPEG, PNG, TIFF, PDF, MP4, RAW)
- 700+ metadata tags
- 16-65x performance vs Perl ExifTool
- Memory safety via Rust
- Cross-platform binaries
"

# Push tag to origin (triggers GitHub Actions release workflow)
git push origin v1.0.0
```

### Step 3: Monitor GitHub Actions Release Workflow

After pushing the tag:

1. Go to https://github.com/exiftool-rs/exiftool-rs/actions
2. Watch the "Release" workflow run
3. Verify all binary builds succeed:
   - Linux x86_64 (musl)
   - Linux ARM64 (musl)
   - macOS Intel (x86_64)
   - macOS Apple Silicon (aarch64)
   - Windows x86_64

Expected artifacts:
- exiftool-rs-x86_64-linux-musl.tar.gz
- exiftool-rs-aarch64-linux-musl.tar.gz
- exiftool-rs-x86_64-macos.tar.gz
- exiftool-rs-aarch64-macos.tar.gz
- exiftool-rs-x86_64-windows.zip
- SHA256SUMS

### Step 4: Create GitHub Release

The release workflow should auto-create a draft release. If not, create manually:

1. Go to https://github.com/exiftool-rs/exiftool-rs/releases/new
2. Select tag: v1.0.0
3. Title: "ExifTool-RS v1.0.0 - Stable Release"
4. Description: Copy content from RELEASE_ANNOUNCEMENT.md or use this shorter version:

```markdown
# ExifTool-RS v1.0.0: Stable Release

🎉 First stable release of ExifTool-RS - a modern, high-performance Rust reimplementation of ExifTool!

## Highlights

- **Performance**: 13-65x faster than Perl ExifTool
- **Format Support**: 50+ formats (JPEG, PNG, TIFF, PDF, MP4, RAW)
- **Tag Database**: 700+ metadata tags from ExifTool source
- **Memory Safety**: Zero buffer overflows via Rust
- **Cross-Platform**: Linux, macOS, Windows binaries
- **No Dependencies**: Static binaries with zero runtime dependencies

## Installation

```bash
# From crates.io
cargo install exiftool-rs

# Or download pre-built binaries below
```

## Performance Benchmarks

| Operation | Perl ExifTool | ExifTool-RS | Speedup |
|-----------|---------------|-------------|---------|
| Single file | 37.5ms | 2.3ms | **16.1x** |
| Batch (1000 files) | 916.4ms | 14.1ms | **64.9x** |
| Write operation | 96.8ms | 7.3ms | **13.3x** |
| Format detection | 39.3ms | 2.8ms | **14.2x** |

*Apple M4, 10-core, 32GB RAM*

## What's New

See [CHANGELOG.md](https://github.com/exiftool-rs/exiftool-rs/blob/main/CHANGELOG.md) for full details.

**Core Features:**
- Full CLI with 90% Perl ExifTool compatibility
- Rust library API with hexagonal architecture
- C FFI bindings for Python, Node.js, Go, etc.
- Batch processing with parallel execution
- JSON and CSV output formats
- Comprehensive documentation and user guide

**Known Limitations** (planned for v1.1+):
- Array type validation not yet implemented
- TIFF writer: Float/Struct/Array types not supported
- See CHANGELOG.md for workarounds

## Links

- 📦 [crates.io](https://crates.io/crates/exiftool-rs)
- 📖 [User Guide](https://exiftool-rs.github.io/exiftool-rs/)
- 💻 [GitHub](https://github.com/exiftool-rs/exiftool-rs)
- 📝 [CHANGELOG](https://github.com/exiftool-rs/exiftool-rs/blob/main/CHANGELOG.md)

---

**Full Changelog**: https://github.com/exiftool-rs/exiftool-rs/commits/v1.0.0
```

5. Attach all binary artifacts (if not auto-attached)
6. Mark as "Latest release"
7. Publish release

### Step 5: Verify Downloads

Test downloading and running binaries from GitHub Releases:

```bash
# Linux
wget https://github.com/exiftool-rs/exiftool-rs/releases/download/v1.0.0/exiftool-rs-x86_64-linux-musl.tar.gz
tar xzf exiftool-rs-x86_64-linux-musl.tar.gz
./exiftool-rs --version

# macOS
wget https://github.com/exiftool-rs/exiftool-rs/releases/download/v1.0.0/exiftool-rs-aarch64-macos.tar.gz
tar xzf exiftool-rs-aarch64-macos.tar.gz
./exiftool-rs --version
```

### Step 6: Post Release Announcement

After successful release, post announcements to Rust community:

**Reddit r/rust**:
- Title: "ExifTool-RS v1.0.0: Fast, Memory-Safe Metadata Management (13-65x faster than Perl)"
- Content: Adapted from RELEASE_ANNOUNCEMENT.md (keep concise)
- Link to GitHub release

**This Week in Rust**:
- Submit at https://github.com/rust-lang/this-week-in-rust
- Category: "Crate of the Week" or "Project Updates"
- Include performance benchmarks

**users.rust-lang.org**:
- Category: "Announcements"
- Title: "[ANN] ExifTool-RS v1.0.0 - High-Performance Metadata Management"
- Content: Full announcement from RELEASE_ANNOUNCEMENT.md

**Twitter/Mastodon/Bluesky** (optional):
```
🎉 ExifTool-RS v1.0.0 is here!

A memory-safe Rust reimplementation of ExifTool with 13-65x performance improvements.

✨ 50+ formats
✨ 700+ tags
✨ Zero dependencies
✨ Cross-platform

cargo install exiftool-rs

https://github.com/exiftool-rs/exiftool-rs
#rustlang #opensource
```

### Step 7: Update Documentation Site (if applicable)

If you have GitHub Pages deployed:

```bash
# Build the documentation book
cd docs/book
mdbook build

# Deploy to GitHub Pages (if configured)
# Or push to gh-pages branch
```

### Step 8: Monitor and Respond

After release:
- Monitor GitHub Issues for bug reports
- Respond to questions on Reddit/forums
- Track crates.io download stats
- Plan v1.1 features based on feedback

## 🔍 Verification Checklist

After completing all steps, verify:

- [ ] crates.io shows v1.0.0 at https://crates.io/crates/exiftool-rs
- [ ] `cargo install exiftool-rs` works and installs v1.0.0
- [ ] GitHub Releases has v1.0.0 with all binaries attached
- [ ] All download links in README.md work
- [ ] Documentation site is up-to-date
- [ ] Release announcement posted to at least one Rust community forum
- [ ] CHANGELOG.md is accurate and complete
- [ ] Git tag v1.0.0 exists and is pushed to origin

## 📋 Post-Release Tasks (Non-Blocking)

These can be done after the release:

- [ ] Submit to awesome-rust lists
- [ ] Write blog post with detailed benchmarks
- [ ] Create demo video/GIF for README
- [ ] Submit to package managers (Homebrew tap, Chocolatey, Scoop)
- [ ] Add badges to README (crates.io version, downloads, CI status)
- [ ] Set up docs.rs documentation generation
- [ ] Plan v1.1 roadmap based on community feedback

## 🐛 Rollback Procedure (If Needed)

If critical bugs are discovered:

1. Yank the crate version:
   ```bash
   cargo yank --vers 1.0.0
   ```

2. Delete the GitHub release and tag:
   ```bash
   git tag -d v1.0.0
   git push origin :refs/tags/v1.0.0
   ```

3. Fix bugs, bump to v1.0.1, and restart process

---

## Notes

- The release workflow is automated via `.github/workflows/release.yml`
- Binary builds use cross-compilation with musl for maximum portability
- The .deb and .rpm packages can be generated locally but aren't part of the automated workflow yet
- Keep RELEASE_ANNOUNCEMENT.md in the repo for future reference

Good luck with the release! 🚀
