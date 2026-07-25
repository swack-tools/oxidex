# oxidex-tags-shared Architecture

The `oxidex-tags-shared` crate centralizes metadata tag schemas and shared helpers used by every `oxidex-tags-*` domain crate.

## Responsibilities
- Define canonical `Tag`, `TagTable`, and `TagDatabase` structs used to deserialize generated YAML/bincode tag definitions.
- Provide the `find_table` helper for ergonomic lookups with consistent error handling.
- Serve as the future home for shared validation, documentation, and normalization utilities that operate on tag data regardless of domain.

## Data Flow
1. Build scripts in `oxidex-tags-*` crates convert upstream YAML into a serialized `TagDatabase` (currently via `bincode`).
2. Each crate exposes a `LazyLock<TagDatabase>` constructed from the embedded bytes.
3. Consumer code calls `oxidex_tags_shared::find_table(&DB, name)` (or higher-level helpers) to fetch a `TagTable`, then enumerates its `Tag` entries.

Because all crates share the same schema types, any new validation passes or doc generators can operate over `TagDatabase` without duplicating structs or conversion glue.

## Consuming the Crate
Add the dependency:
```toml
oxidex-tags-shared = { path = "../oxidex-tags-shared" }
```
Expose shared types from your crate’s public API if downstream callers depend on them:
```rust
pub use oxidex_tags_shared::{Tag, TagTable, TagDatabase};
```
Use `find_table` inside lookup helpers:
```rust
use oxidex_tags_shared::{find_table, TagTable};

pub fn get_tag_table(name: &str) -> Option<&'static TagTable> {
    find_table(&MY_DOMAIN_TAGS, name).ok()
}
```

Future shared utilities (validators, Markdown emitters, fixture runners) will live here as the migration plan progresses, so domain crates remain mostly declarative.

## Generating Domain Documentation
Use the `oxidex-tags` example to emit Markdown documentation for any domain:

```bash
cargo run -p oxidex-tags --example render_domain -- <domain> docs/tag-domains/<domain>.md
# domains: core, camera, media, image, document, specialty
```

The helper calls `render_domain_summary` on the selected `TagDatabase`, producing a table count overview plus previews of individual tags. Generated files are stored under `docs/tag-domains/` for inclusion in broader documentation efforts.
