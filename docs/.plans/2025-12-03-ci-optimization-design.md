# CI Pipeline Optimization Design

**Date:** 2025-12-03
**Goal:** Reduce CI build time through artifact sharing, parallel execution, and faster test runners

## Problem

The `just ci` recipe took **3:11 (191 seconds)** due to:
1. Redundant compilation - Clippy (dev profile) doesn't share artifacts with build (release profile)
2. Sequential execution - All steps run in series
3. Test overhead - Standard cargo test has slower parallel execution
4. Verbose output - `--verbose` flags add I/O overhead

### Time Breakdown (Before)

| Step | Wall Time | % of Total |
|------|-----------|------------|
| test | 88s | 46% |
| build-release | 31s | 16% |
| lint (clippy) | 25s | 13% |
| fmt-check | <1s | 0% |
| Overhead | ~47s | 25% |

## Solution

### 1. Local Optimization (justfile)

**Reorder steps for artifact sharing:**
```
Before: build-release → test → lint → fmt-check
After:  fmt-check → lint-release → build-release → test
```

**Use release profile for Clippy:**
```bash
# Before (dev profile - no shared artifacts)
cargo clippy --all-features -- -D warnings

# After (release profile - shares with build)
cargo clippy --release --all-features -- -D warnings
```

**Remove verbose flags:**
```bash
# Before
cargo build --release --verbose --all-features
cargo test --release --verbose --all-features

# After
cargo build --release --all-features
cargo test --release --all-features
```

**Add cargo-nextest support:**
```bash
cargo nextest run --release --all-features
```

### 2. GitHub Actions Optimization

**Split into parallel jobs:**
- `lint-fmt` - Format check + Clippy (fast, fails early)
- `build-test` - Build + nextest + doc tests (main work)
- `audit` - Security audit (parallel)

**Use cargo-nextest in CI:**
```yaml
- name: Install cargo-nextest
  uses: taiki-e/install-action@v2
  with:
    tool: cargo-nextest

- name: Run tests (nextest)
  run: cargo nextest run --release --all-features
```

## Results

**Local `just ci`:** 3:11 → 2:50 (**11% faster**, saved 21 seconds)

| Metric | Before | After | Change |
|--------|--------|-------|--------|
| Wall time | 191s | 170s | -21s |
| CPU utilization | 580% | 514% | - |

## New Commands

| Command | Description |
|---------|-------------|
| `just ci` | Standard CI (optimized order) |
| `just ci-fast` | CI with nextest |
| `just lint-release` | Clippy with release profile |
| `just test-nextest` | Tests with nextest |

## Implementation

1. ✅ Update justfile with optimized ci recipe
2. ✅ Add lint-release recipe
3. ✅ Update CI workflow for parallel jobs
4. ✅ Add cargo-nextest to CI
5. ✅ Test locally and verify improvement
6. ✅ Commit and push

## Future Optimizations

- **sccache**: Cache compilation across CI runs
- **cargo-chef**: Docker layer caching for dependencies
- **Split test suites**: Run unit tests in debug mode (faster compile)
