# Codacy Complexity Reduction Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Restructure `src/core`, `src/parsers`, and `tests/integration` to lower Codacy’s cyclomatic/cognitive complexity scores without regressing behavior.

**Architecture:** Enforce complexity thresholds via a `cargo clippy` gate, extract the procedural core logic into smaller command modules, introduce parser strategy traits so format-specific logic lives in tiny structs, and replace repetitious integration tests with macro-generated suites.

**Tech Stack:** Rust 1.81+, Cargo workspace, Clippy (`cargo clippy -- -W cognitive_complexity`), serde/bincode (existing), macros for tests.

---

### Task 1: Add complexity gate + baseline script

**Files:**
- Create: `tools/codacy_complexity.rs`
- Modify: `justfile`
- Modify: `.github/workflows/ci.yml` (if present) or `scripts/ci.sh`

**Step 1: Implement metrics helper**
```rust
// tools/codacy_complexity.rs
use std::{process::Command, path::PathBuf};

fn main() {
    let status = Command::new("cargo")
        .args(["clippy", "--workspace", "--all-targets", "--", "-W", "cognitive_complexity", "-Aclippy::nursery", "-Aclippy::pedantic"])
        .status()
        .expect("failed to run clippy complexity gate");
    if !status.success() {
        std::process::exit(1);
    }
}
```

**Step 2: Wire just recipe**
Add to `justfile`:
```make
codacy-complexity:
    cargo run --package tools --bin codacy_complexity
```
(If `tools` workspace doesn’t exist, add `[[bin]]` entry pointing to `tools/codacy_complexity.rs`).

**Step 3: Call in CI**
Append to CI workflow before tests:
```yaml
- name: Codacy Complexity Gate
  run: just codacy-complexity
```

**Step 4: Verify**
Run `just codacy-complexity`. Expect FAIL due to current complexity; confirms guard works.

**Step 5: Commit**
`git add tools/codacy_complexity.rs justfile .github/workflows/ci.yml`
`git commit -m "chore: add Codacy complexity gate"`

---

### Task 2: Split `src/core/operations.rs` into command modules

**Files:**
- Modify: `src/core/operations.rs`
- Create: `src/core/operations/mod.rs`
- Create: `src/core/operations/reader.rs`
- Create: `src/core/operations/writer.rs`
- Create: `src/core/operations/transform.rs`
- Tests: `tests/integration/operations_tests.rs`

**Step 1: Write failing unit tests**
Augment `tests/integration/operations_tests.rs` with focused tests for `read_metadata`, `write_metadata`, `apply_date_shift` using smaller fixtures (add to `tests/fixtures/core/`).

**Step 2: Run new tests (expect fail)**
`cargo test -p oxidex --test integration operations_tests::read_metadata_smoke`
Expect panic because helper modules not exported yet.

**Step 3: Create module skeleton**
```rust
// src/core/operations/mod.rs
pub mod reader;
pub mod writer;
pub mod transform;
```

**Step 4: Move logic**
- `reader.rs` contains `fn read_metadata(path: &Path, opts: &ReaderOptions) -> Result<FileMetadata>`.
- `writer.rs` contains `fn write_metadata(...) -> Result<()>`.
- `transform.rs` contains date shifting + batch transformations.
Preserve existing public API by exporting wrappers from `operations.rs`:
```rust
pub use reader::read_metadata;
pub use writer::write_metadata;
pub use transform::{apply_date_shift, batch_apply};
```

**Step 5: Simplify functions**
Within each module, replace multi-hundred-line `match` with helper structs:
```rust
trait OperationStep {
    fn execute(&self, ctx: &mut OperationContext) -> Result<()>;
}
struct ValidateInput;
struct DetectFormat;
struct ParseTags;
```
Use `once_cell::sync::Lazy` for shared regex/state moved into module-level constants.

**Step 6: Run tests**
`cargo test --package oxidex --lib core::operations`
`cargo test --test integration operations_tests`
Expect pass.

**Step 7: Commit**
`git add src/core/operations* tests/integration/operations_tests.rs`
`git commit -m "refactor(core): split operations into reader/writer/transform"`

---

### Task 3: Introduce parser strategies for TIFF/RAW decoders

**Files:**
- Modify: `src/parsers/tiff/mod.rs`
- Modify: `src/parsers/raw/mod.rs`
- Create: `src/parsers/common/strategy.rs`
- Tests: `tests/integration/tiff_tests.rs`, `tests/raw_metadata_parsing.rs`

**Step 1: Add strategy trait**
```rust
// src/parsers/common/strategy.rs
pub trait ParserStrategy {
    type Output;
    fn description(&self) -> &'static str;
    fn parse(&self, ctx: &mut ParseContext) -> Result<Self::Output>;
}
```

**Step 2: Refactor TIFF**
In `src/parsers/tiff/mod.rs`, replace giant `match format` with registration table:
```rust
static STRATEGIES: Lazy<Vec<Box<dyn ParserStrategy<Output = TagTable>>>> = Lazy::new(|| vec![
    Box::new(TiffIfdStrategy),
    Box::new(PentaxMakernoteStrategy),
    // ...
]);
```
Loop through strategies to parse, reducing nested branching. Each strategy lives in its own file under `src/parsers/tiff/strategies/`.

**Step 3: Apply same pattern to RAW**
`src/parsers/raw/mod.rs` registers `Cr2Strategy`, `NefStrategy`, etc.

**Step 4: Update tests**
Rewrite `tests/integration/tiff_tests.rs` to iterate over strategies using a macro:
```rust
macro_rules! tiff_strategy_test {
    ($name:ident, $strategy:expr, $fixture:expr) => {
        #[test]
        fn $name() {
            let mut ctx = ParseContext::from_fixture($fixture);
            let table = $strategy.parse(&mut ctx).unwrap();
            assert!(!table.tags.is_empty());
        }
    };
}
```
Generate tests for each strategy, reducing copy/paste cases flagged by Codacy.

**Step 5: Run parser-focused tests**
`cargo test --test raw_metadata_parsing`
`cargo test --test tiff_tests`
`cargo test --test raw_format_detection`
All should pass.

**Step 6: Commit**
`git add src/parsers/common/strategy.rs src/parsers/tiff src/parsers/raw tests/integration/tiff_tests.rs tests/raw_metadata_parsing.rs`
`git commit -m "refactor(parsers): share strategy trait to reduce complexity"`

---

### Task 4: Macro-driven integration test suites

**Files:**
- Create: `tests/integration/macros.rs`
- Modify: numerous `tests/integration/*_tests.rs`

**Step 1: Extract macro**
```rust
// tests/integration/macros.rs
#[macro_export]
macro_rules! metadata_roundtrip {
    ($name:ident, $path:expr, $checks:expr) => {
        #[test]
        fn $name() {
            let result = run_cli_on($path).expect("metadata roundtrip");
            ($checks)(result);
        }
    };
}
```

**Step 2: Replace boilerplate**
In each makernote test module, replace repetitive `#[test] fn canon_...` with `metadata_roundtrip!(canon_shot_info, "tests/fixtures/canon/canon1.cr2", |meta| { ... });`.

**Step 3: Introduce table-driven cases**
Add helper arrays to `tests/integration/format_detection.rs`:
```rust
const FORMAT_CASES: &[(&str, &str)] = &[("Canon CR3", "tests/fixtures/canon/IMG_0001.CR3"), ...];
```
Iterate with macro to generate tests.

**Step 4: Update mod import**
`tests/integration/mod.rs` should `pub mod macros;` and ensure `#[macro_use] mod macros;` for other files.

**Step 5: Run integration tests**
`cargo test --test integration`
Expect pass with fewer lines per file; Codacy sees reduced duplication/complexity.

**Step 6: Commit**
`git add tests/integration/macros.rs tests/integration/*.rs`
`git commit -m "test: macro-drive integration suites"`

---

### Task 5: Final verification + Codacy confirmation

**Files:**
- None (verification)

**Step 1: Run full suite**
`cargo fmt`
`cargo clippy --workspace --all-targets -- -D warnings`
`cargo test --workspace`

**Step 2: Run complexity gate**
`just codacy-complexity`
Expect PASS, confirming `cargo clippy` no longer flags high cognitive complexity.

**Step 3: Capture report**
Save `target/codacy-complexity.log` (redirect command output) and link it in release notes.

**Step 4: Commit**
`git add target/codacy-complexity.log` (if tracked) or attach to release artifact; final commit message `"chore: verify Codacy complexity reduction"`.
