# Testing Guide

OxiDex has a comprehensive testing strategy including unit tests, integration tests, and ExifTool comparison tests.

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
# Run all tests (always use --release due to memory requirements)
cargo test --release

# Run specific test module
cargo test --release parsers::jpeg

# Run with output
cargo test --release -- --nocapture

# Run ExifTool comparison tests
cargo test --release --features exiftool-comparison
```

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
