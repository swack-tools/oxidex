# Code Refinement Task

The previous code submission did not pass verification. You must fix the following issues and resubmit your work.

---

## Original Task Description

**Task ID:** I1.T1

**Description:** Create Rust project with cargo, set up directory structure per Section 3 of the plan, configure Cargo.toml with initial dependencies (clap, nom, serde_json, chrono, memmap2, rayon, quick-xml, encoding_rs), add rustfmt and clippy configuration, create basic README.md and LICENSE (GPL-3.0), set up .gitignore.

**Target Files:**
- Cargo.toml
- Cargo.lock
- src/main.rs
- src/lib.rs
- README.md
- LICENSE
- .gitignore
- rustfmt.toml
- .clippy.toml

**Acceptance Criteria:**
- cargo build completes without errors ✅
- cargo clippy runs without warnings ✅
- cargo fmt --check passes ❌ **FAILED**
- directory structure matches Section 3 exactly ✅
- README.md contains project description and basic usage placeholder ✅

---

## Issues Detected

### Critical Issues

*   **Missing LICENSE File:** The LICENSE file is completely missing from the project root. The task explicitly requires creating a LICENSE file with GPL-3.0 text. This is a mandatory deliverable listed in `target_files`.

*   **Formatting Errors:** The command `cargo fmt --check` fails with multiple formatting violations. Several module files have incorrectly ordered `pub mod` declarations that need to be alphabetically sorted.

### Specific Formatting Violations

The following files have import ordering issues (rustfmt requires alphabetical ordering of module declarations):

1. **src/cli/mod.rs:6** - `output_formatter` and `batch_processor` are in wrong order
2. **src/core/mod.rs:6** - Multiple modules not in alphabetical order
3. **src/io/mod.rs:4** - `buffered_reader` should come before `file_reader`
4. **src/parsers/common/mod.rs:4** - `encoding` and `exif_types` in wrong order
5. **src/parsers/jpeg/mod.rs:4** - Multiple modules not in alphabetical order
6. **src/parsers/mod.rs:5** - `common` and `format_detector` in wrong order
7. **src/parsers/tiff/mod.rs:5** - Module ordering issue
8. **src/parsers/xmp/mod.rs:4** - Module ordering issue
9. **src/tag_db/mod.rs:4** - Module ordering issue
10. **src/writers/mod.rs:4** - Multiple modules not in alphabetical order

---

## Best Approach to Fix

You MUST complete the following two actions:

### 1. Create the LICENSE File

Create a file named `LICENSE` in the project root directory containing the full text of the GNU General Public License version 3.0 (GPL-3.0). You can obtain the standard GPL-3.0 license text from https://www.gnu.org/licenses/gpl-3.0.txt or use a template. The file must include:
- The full GPL-3.0 license text
- A copyright notice at the top with the current year (2025) and "ExifTool-RS Contributors"

### 2. Fix All Formatting Issues

Run `cargo fmt` to automatically fix all the module ordering issues in the files listed above. This will reorder the `pub mod` declarations alphabetically as required by rustfmt.

After making these changes, verify that:
- `cargo build` still succeeds
- `cargo clippy` runs without warnings
- `cargo fmt --check` passes without any diff output

---

## Verification Commands

After fixing the issues, run these commands to verify:

```bash
# Fix formatting automatically
cargo fmt

# Verify formatting is correct
cargo fmt --check

# Verify build still works
cargo build

# Verify no clippy warnings
cargo clippy

# Verify LICENSE file exists
ls -la LICENSE
```

All commands must complete successfully with no errors or warnings.
