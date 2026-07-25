# README Simplification Design

**Date:** 2025-12-03
**Goal:** Transform README from 573-line comprehensive documentation into a concise ~100-150 line landing page

## Problem

Current README.md is verbose with:
- Extensive benchmark tables (auto-updated by CI)
- Detailed feature lists
- Full architecture documentation
- CI/CD pipeline documentation
- Fuzzing guides
- Technology stack details

This duplicates content already in `docs/` and clutters the repository landing page.

## Solution

### 1. Simplified README Structure

New README (~100-150 lines):
1. **Header** - Title, badges, one-line description
2. **What is OxiDex?** - 2-3 sentences
3. **Why OxiDex?** - 4-5 key value props (performance, safety, compatibility)
4. **Quick Start** - cargo install + binary link
5. **Usage** - 3-4 essential CLI examples
6. **Documentation** - Links to oxidex.net sections
7. **License & Acknowledgments**

### 2. CI/CD Changes

**Remove:**
- `.github/workflows/benchmark-comparison.yml` - No longer updates README
- `.github/scripts/update_readme_benchmarks.py` - No longer needed

**Keep:**
- `.github/workflows/ci.yml` benchmarks job - Continues publishing to GitHub Pages
- Live benchmarks at oxidex.net/benchmarks remain up-to-date

### 3. Content Migration

Removed README content already exists in docs/:
- Installation → `docs/book/src/installation.md`
- CLI Usage → `docs/book/src/cli_usage.md`
- Formats → `docs/book/src/formats.md`
- Architecture → `docs/architecture/`
- Benchmarks → oxidex.net/benchmarks (live Criterion reports)

No new docs files needed.

## Benefits

1. **Clean landing page** - Users see what matters immediately
2. **No stale content** - README doesn't need CI updates
3. **Single source of truth** - Detailed docs live in one place
4. **Cleaner git history** - No more `[skip ci]` benchmark commits

## Implementation Steps

1. Rewrite README.md with minimal structure
2. Delete `.github/workflows/benchmark-comparison.yml`
3. Delete `.github/scripts/update_readme_benchmarks.py`
4. Commit and push
