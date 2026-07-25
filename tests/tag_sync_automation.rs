//! Regression tests for tag sync automation wiring.

use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn generated_tags_stub_still_delegates_to_active_registry() {
    let generated = repo_file("src/tag_db/generated_tags.rs");

    assert!(
        generated.contains("crate::tag_db::tag_registry::get_tag_descriptor(name)"),
        "generated_tags.rs facade should delegate lookups to the active registry"
    );
    assert!(
        generated.contains("crate::tag_db::tag_registry::tag_count()"),
        "generated_tags.rs facade should delegate counts to the active registry"
    );
}

#[test]
fn build_rs_no_longer_exists() {
    let build_rs = Path::new(env!("CARGO_MANIFEST_DIR")).join("build.rs");

    assert!(
        !build_rs.exists(),
        "build.rs should stay deleted — tag generation lives in src/tag_sync/ + \
         src/bin/sync_tags.rs, run explicitly rather than as a build.rs side effect"
    );
}

#[test]
fn sync_tags_binary_targets_active_domain_crates() {
    let sync_tags = repo_file("src/bin/sync_tags.rs");

    assert!(
        sync_tags.contains(r#"format!("oxidex-tags-{domain}/src/{domain}_tags.yaml")"#),
        "sync_tags.rs should regenerate YAML in the active oxidex-tags-* domain crates"
    );
    assert!(
        !sync_tags.contains("exiftool-tags-{}/src/{}_tags.yaml"),
        "sync_tags.rs should not target obsolete exiftool-tags-* crate paths"
    );
}
