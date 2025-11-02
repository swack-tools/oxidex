# Test Failure Triage Process

This document provides a systematic process for investigating and resolving test failures in ExifTool-RS, particularly integration tests comparing against Perl ExifTool.

## Quick Reference

| Failure Type | First Step | Documentation |
|--------------|------------|---------------|
| Match rate < 99% | Check `KNOWN_DISCREPANCIES.md` | `tests/integration/KNOWN_DISCREPANCIES.md` |
| Benchmark regression | Check Criterion reports | `target/criterion/report/index.html` |
| Error handling test | Check logs for panics | Test output |
| CI failure | Check GitHub Actions logs | `.github/workflows/ci.yml` |

## Triage Workflow

### Step 1: Identify Failure Type

```bash
# Run integration tests locally
cargo test --release --features exiftool-comparison

# Run specific test
cargo test --release --features exiftool-comparison test_comparison_jpeg_with_exif

# Run benchmarks
cargo bench

# Run error handling tests
cargo test --test integration error_handling
```

### Step 2: Gather Context

#### For Match Rate Failures

```bash
# Get detailed comparison
exiftool -json -a -G1 tests/fixtures/path/to/failing/image.jpg > perl.json
target/release/exiftool-rs --json tests/fixtures/path/to/failing/image.jpg > rust.json

# Visual diff
diff -u perl.json rust.json

# Count tags
jq 'length' perl.json
jq 'length' rust.json

# Find specific mismatch
jq '.[0] | keys' perl.json > perl_keys.txt
jq '.[0] | keys' rust.json > rust_keys.txt
diff perl_keys.txt rust_keys.txt
```

#### For Benchmark Regressions

```bash
# View Criterion HTML reports
open target/criterion/report/index.html  # macOS
xdg-open target/criterion/report/index.html  # Linux

# Compare with baseline
cargo bench --bench integration_benchmarks -- --baseline main

# Check for >10% regression
grep -r "change:" target/criterion/
```

#### For Error Handling Failures

```bash
# Run with backtrace
RUST_BACKTRACE=full cargo test --test integration error_handling -- --nocapture

# Check for panics (should be none!)
cargo test --test integration error_handling 2>&1 | grep -i "panic"

# Verify timeout protection
cargo test --test integration test_no_panic_on_random_data -- --nocapture
```

### Step 3: Categorize the Issue

Use this decision tree:

```
Is this a match rate failure?
├─ YES → Is the tag in KNOWN_DISCREPANCIES.md?
│  ├─ YES → Check if tolerance/normalization is correct
│  │  ├─ Fixed → Update normalization logic
│  │  └─ Not Fixed → Investigate why known fix didn't work
│  └─ NO → Is this an acceptable discrepancy?
│     ├─ YES → Document in KNOWN_DISCREPANCIES.md
│     └─ NO → This is a bug → File issue, fix
│
├─ NO → Is this a benchmark regression?
│  ├─ YES → Is regression >10%?
│  │  ├─ YES → Investigate performance issue
│  │  └─ NO → Monitor, may be noise
│  └─ NO → Is this an error handling test?
│     ├─ YES → Did it panic or timeout?
│     │  ├─ Panic → Critical bug, fix immediately
│     │  └─ Timeout → Infinite loop, fix immediately
│     └─ NO → Other test failure → Check test logs
```

### Step 4: Take Action

#### Action A: Document Acceptable Discrepancy

1. Verify discrepancy is truly acceptable (formatting, namespace, known limitation)
2. Add to `tests/integration/KNOWN_DISCREPANCIES.md`:
   ```markdown
   | image_path.jpg | EXIF:TagName | "Expected" | "Actual" | Explanation or [Issue #42] |
   ```
3. If needed, update normalization in `exiftool_comparison_tests.rs`
4. Re-run tests to verify match rate improves

#### Action B: Fix Bug

1. Create GitHub issue with:
   - Test that fails
   - Expected vs actual values
   - Steps to reproduce
   - Relevant test fixture

2. Implement fix:
   ```bash
   # Create feature branch
   git checkout -b fix/tag-extraction-issue-42

   # Make changes
   # ...

   # Verify fix
   cargo test --release --features exiftool-comparison

   # Commit
   git commit -m "fix: correct EXIF tag extraction for issue #42"
   ```

3. Update baseline if match rate improved:
   ```bash
   cargo run --bin generate_baseline -- --update
   git add tests/baselines/
   git commit -m "test: update baseline after fix"
   ```

#### Action C: Performance Regression

1. Profile the slow code:
   ```bash
   cargo bench --bench integration_benchmarks -- --profile-time=10

   # Use perf on Linux
   perf record target/release/deps/integration_benchmarks-*
   perf report

   # Use Instruments on macOS
   instruments -t "Time Profiler" target/release/deps/integration_benchmarks-*
   ```

2. Identify bottleneck and optimize

3. Verify improvement:
   ```bash
   cargo bench --bench integration_benchmarks -- --save-baseline after-fix
   cargo bench --bench integration_benchmarks -- --baseline before-fix
   ```

#### Action D: Error Handling Issue

**If test panicked:**
```bash
# Get full backtrace
RUST_BACKTRACE=full cargo test --test integration test_name -- --nocapture

# Identify panic location from backtrace
# Fix by returning Result instead of unwrap/expect
# Re-run to verify no panic
```

**If test timed out:**
```bash
# Indicates infinite loop - likely in IFD chain traversal
# Check for:
# - Circular IFD references (IFD points to itself)
# - Missing visited set in recursive parsing
# - Unbounded loop in segment parsing

# Fix by:
# - Adding max depth limit
# - Tracking visited IFDs
# - Adding iteration limit with error on exceeded
```

### Step 5: Verify Fix

```bash
# Run full test suite
cargo test --release --all-features

# Run specific test that was failing
cargo test --release --features exiftool-comparison test_that_was_failing

# Run benchmarks if performance-related
cargo bench

# Check CI will pass
gh workflow run ci.yml
```

### Step 6: Update Documentation

- [ ] Update `KNOWN_DISCREPANCIES.md` if documenting new discrepancy
- [ ] Update `CHANGELOG.md` if fixing bug
- [ ] Update baseline if match rate changed
- [ ] Close GitHub issue if bug was fixed
- [ ] Update this triage doc if process improved

## Common Issues and Solutions

### Issue: "Match rate 97.5% below 99% threshold"

**Investigation:**
```bash
# Find which tags are missing
jq -r '.[0] | to_entries | .[] | select(.key | startswith("EXIF:")) | .key' perl.json | sort > perl_exif_tags.txt
jq -r '.[0] | to_entries | .[] | select(.key | startswith("EXIF:")) | .key' rust.json | sort > rust_exif_tags.txt
comm -23 perl_exif_tags.txt rust_exif_tags.txt  # Tags in Perl but not Rust
```

**Common causes:**
- Maker notes not implemented → Document in KNOWN_DISCREPANCIES
- Tag parsing bug → File issue, implement tag
- Namespace mismatch → Update normalize_tag_name()

### Issue: "Benchmark 150% slower than baseline"

**Investigation:**
```bash
cargo bench --bench integration_benchmarks -- --save-baseline slow
git checkout main
cargo build --release
cargo bench --bench integration_benchmarks -- --save-baseline fast
cargo bench --bench integration_benchmarks -- --baseline fast

# Check Criterion report for specific slow function
```

**Common causes:**
- Introduced O(n²) algorithm → Optimize to O(n log n) or O(n)
- Memory allocations in hot path → Use stack allocation or buffer pool
- Missing memoization → Cache expensive computations

### Issue: "Test panicked: called `Result::unwrap()` on an `Err` value"

**Investigation:**
```bash
RUST_BACKTRACE=full cargo test failing_test -- --nocapture 2>&1 | grep -A 20 "panicked at"
```

**Fix:**
```rust
// BEFORE (causes panic)
let value = parse_tag(&data).unwrap();

// AFTER (graceful error)
let value = parse_tag(&data)?;  // Propagate error
// or
let value = parse_tag(&data).unwrap_or_else(|e| {
    eprintln!("Warning: Failed to parse tag: {}", e);
    default_value()
});
```

### Issue: "Test timed out after 5 seconds"

**Common location:** TIFF IFD parsing with circular reference

**Fix:**
```rust
// Add visited set to detect cycles
let mut visited = HashSet::new();

fn parse_ifd_chain(..., visited: &mut HashSet<u64>) -> Result<..> {
    if visited.contains(&offset) {
        return Err(ParseError::CircularReference);
    }
    visited.insert(offset);
    // ... parse IFD
}
```

## Escalation

If unable to resolve after following this process:

1. **Document what you tried** in GitHub issue
2. **Tag maintainers** who own the relevant component:
   - JPEG/EXIF parsing: @parser-team
   - PNG parsing: @parser-team
   - Performance: @performance-team
   - CI/Testing infrastructure: @devops-team

3. **Provide reproduction**:
   - Minimal test case
   - Test fixture (if < 1MB, attach to issue)
   - Expected vs actual behavior
   - Environment (OS, Rust version, ExifTool version)

## Continuous Improvement

After resolving a test failure, consider:

- Is this type of failure common? → Add section to this doc
- Could we detect this earlier? → Add pre-commit hook or lint
- Is our test coverage lacking? → Add more test cases
- Did we learn something? → Update team documentation

## References

- Integration Test Plan: `docs/testing/integration_test_plan.md`
- Known Discrepancies: `tests/integration/KNOWN_DISCREPANCIES.md`
- Comparison Tests: `tests/integration/exiftool_comparison_tests.rs`
- Error Handling Tests: `tests/integration/error_handling_tests.rs`
- CI Workflow: `.github/workflows/ci.yml`
