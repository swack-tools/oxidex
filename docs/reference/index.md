# Reference Documentation

Welcome to the OxiDex reference documentation. This section provides detailed technical information about the library architecture, APIs, and supported formats.

## Contents

### [Architecture](/reference/architecture)
Learn about OxiDex's internal design, including the hexagonal architecture, parser system, and core abstractions.

### [API Reference](/reference/api-reference)
Comprehensive Rust library API documentation with examples and usage patterns.

### [FFI API](/reference/ffi-api)
C-compatible Foreign Function Interface for integrating OxiDex with other programming languages (Python, Node.js, Go, etc.).

### [Tag Database](/reference/tag-database)
Information about the metadata tag database, including supported tag families and auto-generation from ExifTool source.

### [Tag Coverage Analysis](/reference/tag-coverage-analysis)
Detailed analysis of the gap between defined tags and extracted tags, with recommendations for improving coverage.

### [ExifTool Compatibility](/reference/comparison/)
Empirical comparison against ExifTool, including the [JPEG Tag Support mapping](/reference/jpeg-tag-support) (every ExifTool tag OxiDex reads/writes, with working keys and example values) and the [full JPEG Tag Matrix](/reference/jpeg-tag-matrix) (per-tag classification and known-bug inventory, regression-gated in CI).

### [Supported Formats](/reference/formats/)
Complete list of supported file formats with implementation details and coverage information.

## Quick Links

- **Getting Started**: See the [Guide section](/guide/) for installation and usage instructions
- **Performance**: Check out [Performance benchmarks](/performance/) for speed comparisons
- **Contributing**: Read the [Contributing guide](/contributing/) to get involved
