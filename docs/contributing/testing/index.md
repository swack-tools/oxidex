# Testing Guide

OxiDex has a comprehensive testing strategy including unit tests, integration tests, and ExifTool comparison tests.

## Testing Overview

- [Integration Test Plan](./integration_test_plan.md) - Comprehensive integration testing strategy
- [Test Failure Triage](./TEST_FAILURE_TRIAGE.md) - How to handle test failures

## Comparison Testing

The `comparison/` directory contains resources for validating OxiDex against ExifTool:

- [README](./comparison/README.md) - Overview of comparison testing
- [Parity Report](./comparison/PARITY_REPORT.md) - Current parity status
- [Field Naming Guide](./comparison/FIELD_NAMING_GUIDE.md) - Tag naming conventions
- [Test Coverage](./comparison/TEST_COVERAGE.md) - Coverage analysis
- [Raw Outputs](./comparison/raw_outputs.md) - Raw comparison data

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
└── comparison/        # ExifTool comparison infrastructure
```
