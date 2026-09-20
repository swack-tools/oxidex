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
