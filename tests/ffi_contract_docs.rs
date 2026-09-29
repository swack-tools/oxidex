use std::fs;
use std::path::Path;

const WRITE_CONTRACT: &str =
    "Not thread-safe with respect to the handle. Do not call concurrently with";
const WRITE_CONTRACT_TAIL: &str =
    "any other operation on the same handle, including getters, mutations, or";

fn write_doc_block(contents: &str) -> &str {
    let marker = "exiftool_write_file";
    let start = contents
        .find(marker)
        .expect("public surface must declare exiftool_write_file");
    let begin = start.saturating_sub(700);
    &contents[begin..start]
}

#[test]
fn write_contract_is_consistent_across_public_surfaces() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for relative in [
        "src/ffi/write_tags.rs",
        "api/exiftool_rs.h",
        "api/oxidex.h",
        "include/oxidex.h",
    ] {
        let path = root.join(relative);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        let block = write_doc_block(&contents);
        assert!(
            block.contains(WRITE_CONTRACT),
            "{relative} omits write contract"
        );
        assert!(
            block.contains(WRITE_CONTRACT_TAIL),
            "{relative} omits write contract tail"
        );
        assert!(
            block.contains("destruction."),
            "{relative} omits destruction restriction"
        );
        assert!(
            !block.contains("Thread-safe for read-only access to handle"),
            "{relative} retains the contradictory write wording"
        );
    }
}

const POINTER_LIFETIME_PREFIX: &str = "The returned pointer is owned by the handle.";
const POINTER_LIFETIME_GETTERS: &str =
    "It remains valid across subsequent read-only getter calls, including concurrent getters,";
const POINTER_LIFETIME_INVALIDATION: &str =
    "until the next successful `exiftool_read_file` on that handle or handle destruction.";
const POINTER_LIFETIME_SYNCHRONIZATION: &str =
    "File reads, tag mutations, file writes, and destruction must not overlap any operation";

fn getter_doc_block<'a>(relative: &str, contents: &'a str, marker: &str) -> &'a str {
    if relative == "docs/reference/ffi-api.md" {
        let heading = format!("#### `{marker}()`");
        let start = contents
            .find(&heading)
            .unwrap_or_else(|| panic!("{relative} must document {marker}"));
        let rest = &contents[start..];
        let end = rest.find("\n#### ").unwrap_or(rest.len());
        return &rest[..end];
    }

    let declaration = if relative == "src/ffi/read_tags.rs" {
        format!("pub extern \"C\" fn {marker}")
    } else {
        format!("const char *{marker}")
    };
    let start = contents
        .find(&declaration)
        .unwrap_or_else(|| panic!("{relative} must declare {marker}"));
    let begin = start.saturating_sub(1_600);
    &contents[begin..start]
}

#[test]
fn handle_owned_string_lifetime_contract_is_consistent_across_public_surfaces() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for (relative, markers) in [
        (
            "src/ffi/read_tags.rs",
            &[
                "exiftool_get_tag_name_at",
                "exiftool_get_tag_string",
                "exiftool_get_tag_string_in_channel",
            ][..],
        ),
        (
            "api/exiftool_rs.h",
            &["exiftool_get_tag_name_at", "exiftool_get_tag_string"][..],
        ),
        (
            "api/oxidex.h",
            &[
                "exiftool_get_tag_name_at",
                "exiftool_get_tag_string",
                "exiftool_get_tag_string_in_channel",
            ][..],
        ),
        (
            "include/oxidex.h",
            &[
                "exiftool_get_tag_name_at",
                "exiftool_get_tag_string",
                "exiftool_get_tag_string_in_channel",
            ][..],
        ),
        (
            "docs/reference/ffi-api.md",
            &[
                "exiftool_get_tag_name_at",
                "exiftool_get_tag_string",
                "exiftool_get_tag_string_in_channel",
            ][..],
        ),
    ] {
        let path = root.join(relative);
        let contents = fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
        for marker in markers {
            let block = getter_doc_block(relative, &contents, marker);
            assert!(
                block.contains(POINTER_LIFETIME_PREFIX),
                "{relative} {marker} omits owner"
            );
            assert!(
                block.contains(POINTER_LIFETIME_GETTERS),
                "{relative} {marker} omits getter survival"
            );
            assert!(
                block.contains(POINTER_LIFETIME_INVALIDATION),
                "{relative} {marker} omits precise invalidation"
            );
            assert!(
                block.contains(POINTER_LIFETIME_SYNCHRONIZATION),
                "{relative} {marker} omits synchronization boundary"
            );
            assert!(
                !block.contains("Next API call on same handle"),
                "{relative} {marker} retains obsolete next-call wording"
            );
        }
    }
}

/// `(name, value)` of every `pub const EXIFTOOL_*: c_int = N;` in
/// `src/ffi/error.rs`, the one place the codes are defined.
fn defined_error_codes(root: &Path) -> Vec<(String, i64)> {
    let source = fs::read_to_string(root.join("src/ffi/error.rs")).unwrap();
    source
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("pub const EXIFTOOL_")?;
            let (name, value) = rest.split_once(": c_int =")?;
            let value = value.trim().trim_end_matches(';').parse().ok()?;
            Some((format!("EXIFTOOL_{name}"), value))
        })
        .collect()
}

/// Every error code `src/ffi/error.rs` defines is re-exported at
/// `oxidex::ffi::` (as every existing one is), `#define`d with the same
/// value in each C header, and named in the Python binding: a code the C ABI
/// returns that one of those surfaces cannot name makes callers hardcode it.
#[test]
fn every_ffi_error_code_is_named_on_every_public_surface() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let codes = defined_error_codes(root);
    assert!(
        codes
            .iter()
            .any(|(name, _)| name == "EXIFTOOL_ERR_TAG_NOT_WRITTEN"),
        "src/ffi/error.rs must define EXIFTOOL_ERR_TAG_NOT_WRITTEN: {codes:?}"
    );
    let mod_rs = fs::read_to_string(root.join("src/ffi/mod.rs")).unwrap();
    let reexport = mod_rs
        .split_once("pub use error::{")
        .and_then(|(_, rest)| rest.split_once("};"))
        .map(|(list, _)| list)
        .expect("src/ffi/mod.rs must re-export the error codes");
    let reexported: Vec<&str> = reexport
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    let python = fs::read_to_string(root.join("bindings/python/oxidex.py")).unwrap();
    let mut problems = Vec::new();
    for (name, value) in &codes {
        if !reexported.contains(&name.as_str()) {
            problems.push(format!("src/ffi/mod.rs does not re-export {name}"));
        }
        for header in ["api/oxidex.h", "api/exiftool_rs.h", "include/oxidex.h"] {
            let contents = fs::read_to_string(root.join(header)).unwrap();
            if !contents
                .lines()
                .any(|line| line.trim() == format!("#define {name} {value}"))
            {
                problems.push(format!("{header} lacks `#define {name} {value}`"));
            }
        }
        let python_name = name.replacen("EXIFTOOL_", "OXIDEX_", 1);
        if !python
            .lines()
            .any(|line| line.trim() == format!("{python_name} = {value}"))
        {
            problems.push(format!(
                "bindings/python/oxidex.py lacks `{python_name} = {value}`"
            ));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
    // The re-export is the path callers name, not only a source line.
    assert_eq!(oxidex::ffi::EXIFTOOL_ERR_TAG_NOT_WRITTEN, 7);
}
