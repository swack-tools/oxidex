# Architecture

OxiDex follows a hexagonal (ports and adapters) architecture that separates concerns into distinct layers for maintainability and extensibility.

## Core Documentation

- [Tag Database](./tag-database.md) - How definitions are synced from ExifTool and compiled
- [Multi-Crate Tags](./multi-crate-tags.md) - Tag organization across multiple crates
- [Parser Migration Guide](./parser-migration-guide.md) - Guide for migrating parsers
- [Parser Shared Infrastructure](./parser-shared-infrastructure.md) - Common parser infrastructure
- [OxiDex Tags Shared](./oxidex-tags-shared.md) - Shared tag definitions
- [Docs Site Deployment](./github-pages-setup.md) - How this documentation site is built and deployed

## Architecture Overview

```
┌─────────────────────────────────────────────────────┐
│                  Application Layer                   │
│              (CLI, FFI Bindings, MCP)               │
├─────────────────────────────────────────────────────┤
│                   Domain Layer                       │
│         (MetadataMap, TagValue, FileFormat)         │
├─────────────────────────────────────────────────────┤
│               Infrastructure Layer                   │
│     (Format Parsers, I/O, Tag Database)             │
└─────────────────────────────────────────────────────┘
```

See the [Reference Architecture](/reference/architecture) for detailed documentation.
