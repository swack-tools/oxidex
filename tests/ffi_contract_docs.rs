use std::fs;
use std::path::Path;

const WRITE_CONTRACT: &str = "Not thread-safe with respect to the handle. Do not call concurrently with";
const WRITE_CONTRACT_TAIL: &str = "any other operation on the same handle, including getters, mutations, or";

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
        let contents = fs::read_to_string(&path).unwrap_or_else(|error| {
            panic!("failed to read {}: {error}", path.display())
        });
        let block = write_doc_block(&contents);
        assert!(block.contains(WRITE_CONTRACT), "{relative} omits write contract");
        assert!(block.contains(WRITE_CONTRACT_TAIL), "{relative} omits write contract tail");
        assert!(block.contains("destruction."), "{relative} omits destruction restriction");
        assert!(!block.contains("Thread-safe for read-only access to handle"),
            "{relative} retains the contradictory write wording");
    }
}
