# Release Checklist

Use this checklist before tagging a new release.

## Pre-Release Steps

### 1. Run CI Checks

```bash
just ci
```

This runs the full CI-equivalent checks:
- Build (debug and release)
- All tests
- Clippy lints
- Code formatting

### 2. Regenerate Tag Documentation

```bash
just docs-generate-tags
```

This updates `docs/tag-domains/*.md` with the latest tag data from the database.

Review the diffs for accuracy before committing.

### 3. Update CHANGELOG

Edit `CHANGELOG.md`:
- Move items from `[Unreleased]` to new version section
- Add release date
- Ensure all notable changes are documented
- Follow [Keep a Changelog](https://keepachangelog.com/) format

### 4. Update Version Numbers

Check these locations:
- `Cargo.toml` (main crate)
- `oxidex-tags-*/Cargo.toml` (tag crates)
- Homebrew formula
- RPM/Deb packaging manifests

### 5. Verify Release Automation

```bash
just release-check
```

This verifies release-specific automation succeeds.

## Tagging the Release

```bash
# Create annotated tag
git tag -a v1.x.x -m "Release v1.x.x"

# Push tag to trigger release workflow
git push origin v1.x.x
```

## Post-Release

1. Monitor GitHub Actions release workflow
2. Verify artifacts on GitHub Releases page
3. Test installation from release artifacts
4. Announce release (if applicable)
