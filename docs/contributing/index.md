# Contributing to OxiDex

Thank you for your interest in contributing to OxiDex! This guide will help you get started with development, testing, and submitting contributions.

## Getting Started

### Prerequisites

- **Rust:** 1.75+ (install via [rustup](https://rustup.rs/))
- **Git:** Version control
- **C Compiler:** GCC or Clang (for building dependencies)
- **Optional:** ExifTool Perl (for comparison tests)

### Development Setup

1. **Clone the repository:**

```bash
git clone https://github.com/swack-tools/oxidex.git
cd oxidex
```

2. **Build the project:**

```bash
cargo build --release
```

**Note:** Always use `--release` flag due to tag database memory requirements. Debug builds will OOM (>32GB RAM).

3. **Run tests:**

```bash
cargo test --release
```

4. **Install development tools:**

```bash
# Formatter
rustup component add rustfmt

# Linter
rustup component add clippy

# Benchmark runner
cargo install criterion

# Code coverage
cargo install cargo-tarpaulin
```

### Project Structure

```
oxidex/
├── src/
│   ├── bin/              # CLI binary
│   ├── core/             # Core metadata types
│   ├── parsers/          # Format-specific parsers
│   │   ├── jpeg/
│   │   ├── tiff/
│   │   ├── png/
│   │   └── ...
│   ├── ffi/              # C FFI bindings
│   └── lib.rs            # Library entry point
├── exiftool-tags/        # Tag database (separate crate)
├── tests/                # Integration tests
├── benches/              # Benchmarks
├── docs/                 # Documentation
└── examples/             # Example code
```

## Development Workflow

### 1. Create a Branch

```bash
git checkout -b feature/your-feature-name
# or
git checkout -b fix/bug-description
```

**Branch naming:**
- `feature/` - New features
- `fix/` - Bug fixes
- `perf/` - Performance improvements
- `docs/` - Documentation updates
- `refactor/` - Code refactoring

### 2. Make Changes

Follow the [Coding Standards](#coding-standards) below.

### 3. Test Your Changes

```bash
# Run all tests
cargo test --release

# Run specific test
cargo test --release test_name

# Run with output
cargo test --release -- --nocapture
```

### 4. Format and Lint

```bash
# Format code
cargo fmt

# Check formatting
cargo fmt -- --check

# Run linter
cargo clippy --release -- -D warnings
```

### 5. Commit Your Changes

Use conventional commit messages:

```bash
git commit -m "feat: add HEIC format support"
git commit -m "fix: resolve JPEG thumbnail corruption"
git commit -m "perf: optimize TIFF IFD parsing"
git commit -m "docs: update API reference"
```

**Commit message format:**
```
<type>: <description>

[optional body]

[optional footer]
```

**Types:**
- `feat` - New feature
- `fix` - Bug fix
- `perf` - Performance improvement
- `docs` - Documentation changes
- `test` - Test additions/changes
- `refactor` - Code refactoring
- `chore` - Maintenance tasks

### 6. Push and Create Pull Request

```bash
git push origin feature/your-feature-name
```

Then create a pull request on GitHub.

## Coding Standards

### Rust Style Guide

Follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/):

**Naming Conventions:**
```rust
// Types: PascalCase
struct MetadataMap { }
enum TagValue { }

// Functions: snake_case
fn parse_jpeg(data: &[u8]) -> Result<Metadata> { }

// Constants: SCREAMING_SNAKE_CASE
const MAX_TAG_SIZE: usize = 65536;

// Modules: snake_case
mod jpeg_parser;
```

**Error Handling:**
```rust
// Use Result for fallible operations
fn read_file(path: &Path) -> Result<Metadata, ExifToolError> {
    let file = File::open(path)?;
    parse_metadata(file)
}

// Provide context in errors
Err(ExifToolError::ParseError {
    message: format!("Invalid JPEG marker: {:02X}", marker),
    offset: Some(offset),
})
```

**Documentation:**
```rust
/// Parses JPEG metadata from raw bytes.
///
/// # Arguments
///
/// * `data` - Raw JPEG file data
///
/// # Returns
///
/// * `Ok(Metadata)` - Parsed metadata
/// * `Err(ExifToolError)` - Parse error with context
///
/// # Examples
///
/// ```
/// use oxidex::parsers::jpeg::parse_jpeg;
///
/// let data = std::fs::read("photo.jpg")?;
/// let metadata = parse_jpeg(&data)?;
/// ```
pub fn parse_jpeg(data: &[u8]) -> Result<Metadata, ExifToolError> {
    // Implementation
}
```

### Code Comments

**When to comment:**
- Complex algorithms
- Non-obvious optimizations
- Workarounds for external limitations
- Magic numbers (explain what they represent)

**When NOT to comment:**
- Self-explanatory code
- Obvious operations
- Every line (let code speak for itself)

**Good comments:**
```rust
// JPEG markers are big-endian 16-bit values starting with 0xFF
let marker = u16::from_be_bytes([data[0], data[1]]);

// Skip thumbnail data (already extracted in previous pass)
if tag_id == 0x0201 { continue; }

// Per EXIF spec, GPS tags use different byte order than main IFD
let gps_byte_order = detect_gps_byte_order(data);
```

### Testing Standards

**Unit Tests:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_jpeg_marker() {
        let data = [0xFF, 0xD8];  // JPEG SOI marker
        let marker = parse_marker(&data).unwrap();
        assert_eq!(marker, JpegMarker::SOI);
    }

    #[test]
    fn test_parse_invalid_marker() {
        let data = [0x00, 0x00];  // Invalid
        assert!(parse_marker(&data).is_err());
    }
}
```

**Integration Tests:**
```rust
// tests/integration_tests.rs
use oxidex::Metadata;

#[test]
fn test_read_real_jpeg() {
    let metadata = Metadata::from_path("tests/fixtures/sample.jpg")
        .expect("Failed to read test file");

    assert_eq!(metadata.get_string("EXIF:Make").unwrap(), "Canon");
    assert_eq!(metadata.get_integer("EXIF:ISO").unwrap(), 400);
}
```

### Benchmarking

Add benchmarks for performance-critical code:

```rust
// benches/parse_benchmarks.rs
use criterion::{black_box, criterion_group, criterion_main, Criterion};
use oxidex::parsers::jpeg::parse_jpeg;

fn benchmark_jpeg_parsing(c: &mut Criterion) {
    let data = std::fs::read("tests/fixtures/large.jpg").unwrap();

    c.bench_function("parse_jpeg", |b| {
        b.iter(|| parse_jpeg(black_box(&data)))
    });
}

criterion_group!(benches, benchmark_jpeg_parsing);
criterion_main!(benches);
```

Run benchmarks:
```bash
cargo bench
```

## Adding New Features

### Adding a New File Format

1. **Create parser module:**

```bash
mkdir -p src/parsers/your_format
touch src/parsers/your_format/mod.rs
```

2. **Implement format detection:**

```rust
// src/formats/mod.rs
pub fn detect_format(data: &[u8]) -> Option<FileFormat> {
    match &data[0..4] {
        // Your format magic number
        [0x89, b'Y', b'O', b'U'] => Some(FileFormat::YourFormat),
        // ... other formats
        _ => None,
    }
}
```

3. **Implement parser:**

```rust
// src/parsers/your_format/mod.rs
use crate::core::{Metadata, TagValue};
use crate::error::{Result, ExifToolError};

pub fn parse_your_format(data: &[u8]) -> Result<Metadata> {
    let mut metadata = Metadata::new();

    // Parse format-specific structures
    // Extract tags
    // Populate metadata

    Ok(metadata)
}
```

4. **Add tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_your_format() {
        let data = include_bytes!("../../../tests/fixtures/sample.your");
        let metadata = parse_your_format(data).unwrap();
        assert!(metadata.len() > 0);
    }
}
```

5. **Add integration test:**

Create test file in `tests/fixtures/` and add integration test.

6. **Update documentation:**

Add format to `docs/formats.md` with:
- File extensions
- Supported metadata types
- Tag count
- Common use cases

### Adding New Tags

1. **Tags are auto-generated from ExifTool source**

The tag database is automatically synchronized. To add custom tags:

```rust
// For custom tags not in ExifTool
metadata.insert("Custom:YourTag", TagValue::new_string("value"));
```

2. **To update from latest ExifTool:**

```bash
rm exiftool-tags/src/tag_db/generated_tags.rs
cargo build --release
```

## Testing

### Running Tests

```bash
# All tests (always use --release)
cargo test --release

# Specific module
cargo test --release parsers::jpeg

# With output
cargo test --release -- --nocapture

# Single test
cargo test --release test_parse_jpeg_marker
```

### Test Data

Place test files in `tests/fixtures/`:

```
tests/fixtures/
├── jpeg/
│   ├── sample_with_exif.jpg
│   ├── sample_no_metadata.jpg
│   └── corrupted.jpg
├── tiff/
├── png/
└── ...
```

### Code Coverage

```bash
cargo tarpaulin --release --out Html
open tarpaulin-report.html
```

## Pull Request Guidelines

### Before Submitting

- [ ] Code compiles without warnings: `cargo build --release`
- [ ] All tests pass: `cargo test --release`
- [ ] Code is formatted: `cargo fmt -- --check`
- [ ] Linter passes: `cargo clippy --release -- -D warnings`
- [ ] Documentation updated if needed
- [ ] Benchmarks added for performance-critical changes
- [ ] CHANGELOG.md updated (for notable changes)

### PR Description Template

```markdown
## Description

Brief description of changes.

## Type of Change

- [ ] Bug fix
- [ ] New feature
- [ ] Performance improvement
- [ ] Documentation update
- [ ] Refactoring

## Testing

Describe testing done:
- Unit tests added/modified
- Integration tests added
- Manual testing performed

## Checklist

- [ ] Code follows project style guidelines
- [ ] Self-review completed
- [ ] Comments added for complex code
- [ ] Documentation updated
- [ ] Tests pass locally
- [ ] No new warnings introduced
```

### Review Process

1. **Automated Checks:** CI runs tests, linting, benchmarks
2. **Code Review:** Maintainer reviews code quality, design
3. **Feedback:** Address review comments
4. **Approval:** Maintainer approves and merges

## Performance Guidelines

### Optimization Principles

1. **Measure first:** Profile before optimizing
2. **Focus on hot paths:** Optimize frequently-called code
3. **Avoid premature optimization:** Clarity > premature speed
4. **Document trade-offs:** Explain complex optimizations

### Using Profiling

```bash
# Install samply
cargo install samply

# Profile benchmark
samply record target/release/oxidex photo.jpg

# Opens Firefox Profiler with results
```

See [Profiling Guide](/performance/profiling) for details.

### Benchmarking Changes

```bash
# Baseline
cargo bench
cp -r target/criterion target/criterion-baseline

# Make changes
# ...

# Re-benchmark
cargo bench

# Compare results in target/criterion/report/index.html
```

## Documentation

### Inline Documentation

Use rustdoc for all public APIs:

```rust
/// Parses JPEG metadata.
///
/// # Arguments
///
/// * `data` - Raw file bytes
///
/// # Errors
///
/// Returns error if:
/// - File is not valid JPEG
/// - Metadata is corrupted
pub fn parse_jpeg(data: &[u8]) -> Result<Metadata> {
    // ...
}
```

### User Documentation

Update relevant documentation in `docs/`:
- `docs/formats.md` - Format support
- `docs/api.md` - API changes
- `docs/cli.md` - CLI changes

## Getting Help

- **GitHub Discussions:** Ask questions
- **GitHub Issues:** Report bugs, request features
- **Discord:** Join community chat (link in README)

## Code of Conduct

Be respectful, inclusive, and collaborative. See `CODE_OF_CONDUCT.md` for full guidelines.

## License

By contributing, you agree that your contributions will be licensed under the GNU General Public License v3.0 (GPL-3.0).

## Additional Resources

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Cargo Book](https://doc.rust-lang.org/cargo/)
- [ExifTool Documentation](https://exiftool.org/)
- [Architecture](/reference/architecture) - System design
- [Profiling Guide](/performance/profiling) - Performance optimization
