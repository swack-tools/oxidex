//! Regression tests for tag sync automation wiring.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
use std::process::Command;

fn sha256(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

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
        "build.rs should stay deleted — tag generation runs in Tier 1"
    );
}

#[test]
fn dump_backed_binary_targets_active_domain_crates() {
    let generator = repo_file("src/bin/gen_tag_registry.rs");

    assert!(
        generator.contains(r#"format!("oxidex-tags-{domain}/src/{domain}_tags.yaml")"#),
        "gen_tag_registry.rs should regenerate the active domain crates"
    );
    assert!(
        generator.contains("tag_records_from_dump_document") && !generator.contains("parse_listx"),
        "the generator must read source facts, not the documentation oracle"
    );
}

#[test]
fn generator_rejects_a_dump_from_a_different_source() {
    let scratch = tempfile::tempdir().unwrap();
    let source = scratch.path().join("lib/Image");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("ExifTool.pm"), "different source").unwrap();
    let pin = repo_file(".exiftool-version");
    let dump = scratch.path().join("tables.json");
    fs::write(
        &dump,
        serde_json::json!({
            "exiftool_version": pin.trim(),
            "modules_ok": 1,
            "modules_failed": 0,
            "source_provenance": {
                "library_relative_path": "Image/ExifTool.pm",
                "sha256": "0000000000000000000000000000000000000000000000000000000000000000"
            },
            "modules": {}
        })
        .to_string(),
    )
    .unwrap();

    let result = Command::new(env!("CARGO_BIN_EXE_gen_tag_registry"))
        .arg(&dump)
        .arg(scratch.path().join("lib"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr)
            .contains("selected source does not match table dump provenance"),
        "unexpected refusal: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn generator_rejects_stale_module_before_replacing_any_domain() {
    let scratch = tempfile::tempdir().unwrap();
    let source = scratch.path().join("lib/Image/ExifTool");
    fs::create_dir_all(&source).unwrap();
    let core = b"selected ExifTool.pm";
    let original_module = b"original Exif.pm";
    fs::write(scratch.path().join("lib/Image/ExifTool.pm"), core).unwrap();
    fs::write(source.join("Exif.pm"), original_module).unwrap();
    let dump = scratch.path().join("tables.json");
    fs::write(
        &dump,
        serde_json::json!({
            "exiftool_version": repo_file(".exiftool-version").trim(),
            "modules_ok": 1,
            "modules_failed": 0,
            "source_provenance": {
                "library_relative_path": "Image/ExifTool.pm",
                "sha256": sha256(core)
            },
            "native_capture_context": {"loaded_closure": {"modules": [
                {"inc": "Image/ExifTool.pm", "source_file": "Image/ExifTool.pm", "source_sha256": sha256(core)},
                {"inc": "Image/ExifTool/Exif.pm", "source_file": "Image/ExifTool/Exif.pm", "source_sha256": sha256(original_module)}
            ]}},
            "modules": {"Exif": {"tables": {"Main": {
                "full_name": "Image::ExifTool::Exif::Main",
                "meta": {},
                "tags": {"1": {"Name": "Make"}}
            }}}}
        })
        .to_string(),
    )
    .unwrap();
    fs::write(source.join("Exif.pm"), b"changed Exif.pm").unwrap();

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let domain_paths: Vec<_> = ["core", "camera", "media", "image", "document", "specialty"]
        .into_iter()
        .map(|domain| root.join(format!("oxidex-tags-{domain}/src/{domain}_tags.yaml")))
        .collect();
    let before: Vec<_> = domain_paths
        .iter()
        .map(|path| sha256(&fs::read(path).unwrap()))
        .collect();
    let result = Command::new(env!("CARGO_BIN_EXE_gen_tag_registry"))
        .arg(&dump)
        .arg(scratch.path().join("lib"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(
        String::from_utf8_lossy(&result.stderr).contains(
            "selected module source does not match table dump provenance: Image/ExifTool/Exif.pm"
        ),
        "unexpected refusal: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    let after: Vec<_> = domain_paths
        .iter()
        .map(|path| sha256(&fs::read(path).unwrap()))
        .collect();
    assert_eq!(
        before, after,
        "refusal must leave all six registries untouched"
    );
}

#[test]
fn generator_rejects_omitted_source_module_even_when_count_is_lowered() {
    let scratch = tempfile::tempdir().unwrap();
    let source = scratch.path().join("lib/Image/ExifTool");
    fs::create_dir_all(&source).unwrap();
    let core = b"selected ExifTool.pm";
    let exif = b"selected Exif.pm";
    let quicktime = b"selected QuickTime.pm";
    fs::write(scratch.path().join("lib/Image/ExifTool.pm"), core).unwrap();
    fs::write(source.join("Exif.pm"), exif).unwrap();
    fs::write(source.join("QuickTime.pm"), quicktime).unwrap();
    let mut doc = serde_json::json!({
        "exiftool_version": repo_file(".exiftool-version").trim(),
        "modules_ok": 2,
        "modules_failed": 0,
        "source_provenance": {
            "library_relative_path": "Image/ExifTool.pm", "sha256": sha256(core)
        },
        "native_capture_context": {"loaded_closure": {"modules": [
            {"inc": "Image/ExifTool.pm", "source_file": "Image/ExifTool.pm", "source_sha256": sha256(core)},
            {"inc": "Image/ExifTool/Exif.pm", "source_file": "Image/ExifTool/Exif.pm", "source_sha256": sha256(exif)},
            {"inc": "Image/ExifTool/QuickTime.pm", "source_file": "Image/ExifTool/QuickTime.pm", "source_sha256": sha256(quicktime)}
        ]}},
        "native_write_tables": {"Exif": {"Main": {}}, "QuickTime": {"Keys": {}}},
        "modules": {"Exif": {"tables": {"Main": {
            "full_name": "Image::ExifTool::Exif::Main", "meta": {},
            "tags": {"1": {"Name": "Make"}}
        }}}}
    });
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let paths: Vec<_> = ["core", "camera", "media", "image", "document", "specialty"]
        .into_iter()
        .map(|domain| root.join(format!("oxidex-tags-{domain}/src/{domain}_tags.yaml")))
        .collect();
    let before: Vec<_> = paths
        .iter()
        .map(|path| sha256(&fs::read(path).unwrap()))
        .collect();
    let dump = scratch.path().join("tables.json");
    for (claimed_count, expected_reason) in [
        (2, "table dump module count does not match captured modules"),
        (
            1,
            "independently captured source module is missing: QuickTime",
        ),
    ] {
        doc["modules_ok"] = claimed_count.into();
        fs::write(&dump, doc.to_string()).unwrap();
        let result = Command::new(env!("CARGO_BIN_EXE_gen_tag_registry"))
            .arg(&dump)
            .arg(scratch.path().join("lib"))
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert!(
            String::from_utf8_lossy(&result.stderr).contains(expected_reason),
            "unexpected refusal: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let after: Vec<_> = paths
            .iter()
            .map(|path| sha256(&fs::read(path).unwrap()))
            .collect();
        assert_eq!(
            before, after,
            "refusal must leave all six registries untouched"
        );
    }
}
