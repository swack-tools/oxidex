# Oxidex Tags Shared Crate Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Create the foundational `oxidex-tags-shared` crate and migrate `oxidex-tags-core` to consume its shared tag types and table lookup helper, proving the new architecture works end-to-end.

**Architecture:** Add a workspace crate exporting `Tag`, `TagTable`, and `TagDatabase` types plus `find_table` helper built around lazy-static databases. Consumers (starting with `oxidex-tags-core`) import the crate instead of defining duplicate structs, enabling future validators/documentation helpers to live centrally.

**Tech Stack:** Rust 1.81+, Cargo workspace, serde for serialization, bincode for core tags deserialization, `once_cell` for `LazyLock`.

---

### Task 1: Create `oxidex-tags-shared` crate skeleton

**Files:**
- Create: `oxidex-tags-shared/Cargo.toml`
- Create: `oxidex-tags-shared/src/lib.rs`

**Step 1: Scaffold crate manifest**
```toml
[package]
name = "oxidex-tags-shared"
version = "0.1.0"
edition = "2021"

[dependencies]
once_cell = { version = "1", features = ["parking_lot"] }
serde = { version = "1", features = ["derive"] }
```

**Step 2: Wire crate into workspace**
Add to root `Cargo.toml` workspace members list:
```toml
members = [
    "oxidex-tags",
    "oxidex-tags-core",
    # ...existing entries...
    "oxidex-tags-shared"
]
```
Also add path dependency to any `oxidex-tags-*` crate you migrate later.

**Step 3: Create minimal lib**
```rust
// oxidex-tags-shared/src/lib.rs
pub mod types;

pub use types::{Tag, TagDatabase, TagTable};
```

**Step 4: Commit scaffold**
```bash
git add oxidex-tags-shared/Cargo.toml oxidex-tags-shared/src/lib.rs Cargo.toml
git commit -m "feat: add oxidex-tags-shared crate skeleton"
```

### Task 2: Define shared tag schema types and lookup helper

**Files:**
- Create: `oxidex-tags-shared/src/types.rs`
- Modify: `oxidex-tags-shared/src/lib.rs`
- Create tests: `oxidex-tags-shared/tests/find_table.rs`

**Step 1: Move Tag schema structs**
Copy structs from `oxidex-tags-core/src/types.rs` into shared crate, adjusting visibility:
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Tag { /* fields same as core */ }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TagTable {
    pub name: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TagDatabase {
    pub tables: Vec<TagTable>,
}
```

**Step 2: Add lookup helper + error type**
Append to `types.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    #[error("tag table '{0}' not found")]
    NotFound(String),
}

pub fn find_table<'a>(db: &'a TagDatabase, name: &str) -> Result<&'a TagTable, LookupError> {
    db.tables
        .iter()
        .find(|table| table.name == name)
        .ok_or_else(|| LookupError::NotFound(name.to_string()))
}
```
Add `thiserror = "1"` to crate dependencies.

**Step 3: Export helper**
Update `src/lib.rs`:
```rust
mod types;

pub use types::{find_table, LookupError, Tag, TagDatabase, TagTable};
```

**Step 4: Write integration test**
`oxidex-tags-shared/tests/find_table.rs`:
```rust
use oxidex_tags_shared::{find_table, Tag, TagDatabase, TagTable};

fn sample_db() -> TagDatabase {
    TagDatabase {
        tables: vec![TagTable {
            name: "Exif::Main".into(),
            tags: vec![Tag {
                id: "0x0001".into(),
                name: "InteropIndex".into(),
                writable: false,
                type_name: Some("string".into()),
                description: Some("Indicates the identification of interoperability rule".into()),
            }],
        }],
    }
}

#[test]
fn finds_existing_table() {
    let db = sample_db();
    let table = find_table(&db, "Exif::Main").unwrap();
    assert_eq!(table.name, "Exif::Main");
}

#[test]
fn errors_on_missing_table() {
    let db = sample_db();
    let err = find_table(&db, "GPS::Main").unwrap_err();
    assert!(matches!(err, oxidex_tags_shared::LookupError::NotFound(name) if name == "GPS::Main"));
}
```

**Step 5: Run crate tests**
```bash
cargo test -p oxidex-tags-shared
```
Expect PASS.

**Step 6: Commit**
```bash
git add oxidex-tags-shared
git commit -m "feat: add shared tag schema and lookup helper"
```

### Task 3: Consume shared crate in `oxidex-tags-core`

**Files:**
- Modify: `oxidex-tags-core/Cargo.toml`
- Modify: `oxidex-tags-core/src/lib.rs`
- Modify: `oxidex-tags-core/src/types.rs`

**Step 1: Add dependency**
`oxidex-tags-core/Cargo.toml`:
```toml
[dependencies]
oxidex-tags-shared = { path = "../oxidex-tags-shared" }
# existing deps...
```

**Step 2: Re-export shared types**
In `oxidex-tags-core/src/lib.rs`, remove local `pub mod types; pub use types::*;` definitions tied to schema structs. Instead:
```rust
pub use oxidex_tags_shared::{Tag, TagDatabase, TagTable};
```
Retain backward-compatibility structs by moving them to `types.rs` under a new module `compat` (keep them referencing shared `Tag` if needed).

**Step 3: Update `CORE_TAGS` type**
Ensure `LazyLock<TagDatabase>` uses shared type; update `get_tag_table` to call `oxidex_tags_shared::find_table(&CORE_TAGS, name).ok()` or fallback to manual iteration if you prefer no error handling.

**Step 4: Slim `types.rs`**
Delete duplicate `Tag`, `TagTable`, `TagDatabase` definitions, keep only backward-compatibility types (e.g., `TagDescriptor`). Update references to use `oxidex_tags_shared::Tag` where appropriate.

**Step 5: Run focused tests**
```bash
cargo test -p oxidex-tags-core
```
Expect PASS.

**Step 6: Commit**
```bash
git add oxidex-tags-core/Cargo.toml oxidex-tags-core/src/lib.rs oxidex-tags-core/src/types.rs
git commit -m "refactor: consume shared tag schema"
```

### Task 4: Document shared crate usage

**Files:**
- Create: `docs/architecture/oxidex-tags-shared.md`
- Modify: `README.md` (optional pointer)

**Step 1: Document architecture**
Write summary covering responsibilities, key APIs (`Tag`, `find_table`), and how other crates should depend on it.

**Step 2: Link from README**
Add short note in existing README "Tag architecture" section referencing the detailed doc.

**Step 3: Proof + commit**
```bash
git add docs/architecture/oxidex-tags-shared.md README.md
git commit -m "docs: describe shared tag schema crate"
```

---

## Verification Checklist
- `cargo test -p oxidex-tags-shared`
- `cargo test -p oxidex-tags-core`
- `cargo fmt && cargo clippy --workspace --all-targets`
- Review docs for clarity (spellcheck / markdown lint if available)
