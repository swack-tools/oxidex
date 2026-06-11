//! Regression tests for tag sync automation wiring.

use std::fs;
use std::path::Path;

fn repo_file(path: &str) -> String {
    fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|error| panic!("failed to read {path}: {error}"))
}

#[test]
fn generated_tags_stub_delegates_count_to_active_registry() {
    let build_rs = repo_file("build.rs");

    assert!(
        build_rs.contains(r#"writeln!(file, "    crate::tag_db::tag_registry::tag_count()")"#),
        "build.rs should generate a compatibility facade that delegates counts to the active registry"
    );
    assert!(
        !build_rs.contains(r#"writeln!(file, "    {}", tags.len())"#),
        "build.rs must not regenerate generated_tag_count() with a stale parsed constant"
    );
}

#[test]
fn tag_sync_targets_active_domain_crates_and_counts_yaml_sources() {
    let build_rs = repo_file("build.rs");
    let workflow = repo_file(".github/workflows/sync-exiftool-tags.yml");

    assert!(
        build_rs.contains(r#"format!("oxidex-tags-{}/src/{}_tags.yaml", domain, domain)"#),
        "build.rs should regenerate YAML in the active oxidex-tags-* domain crates"
    );
    assert!(
        !build_rs.contains("exiftool-tags-{}/src/{}_tags.yaml"),
        "build.rs should not target obsolete exiftool-tags-* crate paths"
    );
    assert!(
        workflow.contains("oxidex-tags-*/src/*_tags.yaml"),
        "sync workflow should count tags from active YAML domain crates"
    );
    assert!(
        !workflow.contains("grep -A 1 \"pub fn generated_tag_count\" src/tag_db/generated_tags.rs"),
        "sync workflow must not scrape generated_tags.rs as the tag-count source"
    );
}
