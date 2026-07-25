# Development Guide

Resources for contributing to OxiDex development.

## Getting Started

See the main [Contributing Guide](../) for setup instructions, coding standards, and workflow.

## Additional Resources

- [Archived Context](./archived-context.md) - Historical development context and notes

## Development Environment

### Prerequisites

- **Rust:** 1.75+ (install via [rustup](https://rustup.rs/))
- **Git:** Version control
- **C Compiler:** GCC or Clang (for building dependencies)
- **Optional:** ExifTool Perl (for comparison tests)

### Quick Start

```bash
# Clone and build
git clone https://github.com/swack-tools/oxidex.git
cd oxidex
cargo build --release

# Run tests
cargo test --release

# Run linter
cargo clippy --release -- -D warnings
```

### Important Notes

- **Always use `--release` flag** - Debug builds require >32GB RAM due to tag database size
- Run `cargo fmt` before committing
- Run `cargo clippy` to catch common issues
