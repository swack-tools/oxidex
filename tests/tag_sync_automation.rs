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
         src/bin/gen_tag_registry.rs, run explicitly (via tools/exiftool-tables/regen.sh) \
         rather than as a build.rs side effect"
    );
}

/// `sync_tags.rs` (which shelled out to `exiftool -f -listx`, the
/// documentation view — see `AGENTS.md`, "Tag knowledge is not tag
/// coverage") was retired in Step 30 of the tag-machinery overhaul.
/// `gen_tag_registry.rs` replaced it, generating the same YAML shape from
/// `dump_tables.pl`'s dump of ExifTool's real Perl tag tables instead.
#[test]
fn sync_tags_binary_was_retired_in_favor_of_gen_tag_registry() {
    let sync_tags = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/bin/sync_tags.rs");
    assert!(
        !sync_tags.exists(),
        "sync_tags.rs should stay deleted — replaced by gen_tag_registry.rs"
    );

    let gen_tag_registry = repo_file("src/bin/gen_tag_registry.rs");
    assert!(
        gen_tag_registry.contains(r#"format!("oxidex-tags-{domain}/src/{domain}_tags.yaml")"#),
        "gen_tag_registry.rs should regenerate YAML in the active oxidex-tags-* domain crates"
    );
    assert!(
        !gen_tag_registry.contains("exiftool-tags-{}/src/{}_tags.yaml"),
        "gen_tag_registry.rs should not target obsolete exiftool-tags-* crate paths"
    );
}
