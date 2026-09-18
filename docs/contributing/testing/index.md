# Testing Guide

OxiDex is tested at three levels: unit and integration tests (`cargo test`), comparison against the pinned ExifTool (the instruments below), and the CI gates described in the [contributing guide](/contributing/#what-ci-enforces).

## Testing Overview

- [Test Failure Triage](./TEST_FAILURE_TRIAGE.md) - How to handle test failures
- [Integration test layout](https://github.com/swack-tools/oxidex/blob/refactor/tag-machinery/tests/integration/README.md) - the `tests/integration/` README, kept next to the tests

## Comparison Testing

OxiDex is validated against the pinned ExifTool release (`.exiftool-version`)
by measurement, not by a hand-maintained parity report:

- [Measuring Coverage](../measuring-coverage.md) - how extraction coverage is measured and how to add a file type
- [ExifTool Coverage](/reference/tag-coverage-analysis) - the generated, CI-refreshed conformance report
- [JPEG Tag Support](/reference/jpeg-tag-support) and [JPEG Tag Matrix](/reference/jpeg-tag-matrix) - generated per-tag reports
- [Corpus Read Observations](/reference/catalog-corpus-observed) - authenticated read receipts against the source catalog
- `tests/integration/KNOWN_DISCREPANCIES.md` - documented, accepted differences (in the repository, next to the comparison tests)

## Running Tests

```bash
# The whole workspace (what the local checks in the contributing guide run)
cargo test --workspace

# The library tests with the release profile
cargo test --lib --release

# One module, with output
cargo test --lib parsers::jpeg -- --nocapture

# ExifTool comparison tests (need the pinned ExifTool; see the contributing guide)
cargo test --features exiftool-comparison

# Ignored tests are not run by default; sweep them per target after a rename
cargo test --workspace -- --ignored
```

`cargo test --workspace --release` can fail with bogus `panic strategy` or
duplicate-`chrono` errors after `cargo clippy --all-features` has shared the
target directory. This is an output filename collision, not a test failure;
`cargo clean --release -p chrono -p oxidex` clears it.

## Test Organization

```
tests/
├── fixtures/           # Test images and files
│   ├── jpeg/          # JPEG test files
│   ├── tiff/          # TIFF test files
│   └── ...
├── integration/       # Integration tests
└── integration/exiftool_comparison_tests.rs  # ExifTool comparison tests (feature `exiftool-comparison`)
```
