// The harness shells out to `cc` with `-Wl,-rpath` and unix library search
// paths, none of which exist on Windows toolchains.
#![cfg(unix)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every optional feature `Cargo.toml` declares, paired with whether the cargo
/// invocation that built this test binary enabled it. Integration tests are
/// compiled with the package's own feature set, so `cfg!` here reports the
/// OUTER feature graph -- which is the only way this test can learn it.
///
/// `forwarded_features_cover_every_declared_feature` keeps this list complete;
/// see `nested_build_args` for what an incomplete one breaks.
const OPTIONAL_FEATURES: &[(&str, bool)] = &[
    ("exiftool-comparison", cfg!(feature = "exiftool-comparison")),
    (
        "jpeg-tag-matrix-binary",
        cfg!(feature = "jpeg-tag-matrix-binary"),
    ),
    ("magika", cfg!(feature = "magika")),
    (
        "tag-comparison-binary",
        cfg!(feature = "tag-comparison-binary"),
    ),
];

/// Arguments for the nested `cargo build --lib` below, reproducing the feature
/// graph of the invocation that is running this test.
///
/// This matters far more than "build the same thing we are testing". `cargo
/// build --lib` inherits nothing from its parent invocation, so left bare it
/// builds the DEFAULT feature graph -- and that is not a harmless second
/// build. `[lib] crate-type = ["lib", "staticlib", "cdylib"]` makes cargo emit
/// the lib target with no `-C extra-filename` hash, so *every* feature graph
/// shares one set of filenames: `target/<profile>/deps/liboxidex.{rlib,a,dylib}`.
/// A mismatched nested build therefore overwrites the rlib the outer build
/// produced, cargo's fingerprints never notice, and `cargo test` compiles
/// doctests LAST -- against the overwritten rlib.
///
/// That is precisely why `cargo test --all-features` used to fail with E0432
/// ("could not find `magika_detector` in `parsers`") on the two
/// `parsers::magika_detector` examples, while the two commands CI runs both
/// passed: `cargo nextest run --all-features` runs this test but no doctests,
/// and `cargo test --doc --all-features` runs the doctests but not this test.
///
/// Matching the outer graph resolves this build to the unit cargo has already
/// compiled, so it writes nothing at all.
fn nested_build_args() -> Vec<String> {
    let mut args = vec!["build".to_string(), "--lib".to_string()];

    if !cfg!(feature = "default") {
        args.push("--no-default-features".to_string());
    }

    let enabled: Vec<&str> = OPTIONAL_FEATURES
        .iter()
        .filter(|(_, enabled)| *enabled)
        .map(|(name, _)| *name)
        .collect();
    if !enabled.is_empty() {
        args.push("--features".to_string());
        args.push(enabled.join(","));
    }

    args
}

/// The `[features]` keys declared in `Cargo.toml`, excluding `default` (which
/// `nested_build_args` handles through `--no-default-features`).
fn declared_optional_features(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .skip_while(|line| line.trim() != "[features]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter(|line| !line.trim_start().starts_with('#'))
        .filter_map(|line| line.split_once('='))
        .map(|(name, _)| name.trim())
        .filter(|name| !name.is_empty() && *name != "default")
        .collect()
}

/// A feature that `OPTIONAL_FEATURES` does not know about is invisible: the
/// nested build silently becomes a different unit again, and the doctest
/// breakage described on `nested_build_args` comes back. Fail here, loudly,
/// rather than there.
#[test]
fn forwarded_features_cover_every_declared_feature() {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let manifest = std::fs::read_to_string(&manifest_path).expect("read Cargo.toml");

    let declared = declared_optional_features(&manifest);
    assert!(
        !declared.is_empty(),
        "parsed no [features] out of {} -- the scan in declared_optional_features \
         has drifted from the manifest layout",
        manifest_path.display()
    );

    for name in declared {
        assert!(
            OPTIONAL_FEATURES.iter().any(|(known, _)| *known == name),
            "Cargo.toml declares feature `{name}`, but OPTIONAL_FEATURES in \
             tests/ffi_c_integration.rs does not forward it to the nested \
             `cargo build --lib`. Unforwarded, that build becomes a different unit \
             from the outer one and overwrites target/<profile>/deps/liboxidex.rlib, \
             which the doctests link afterwards. Add `(\"{name}\", cfg!(feature = \
             \"{name}\"))` to OPTIONAL_FEATURES."
        );
    }
}

fn prepend_env_path(command: &mut Command, key: &str, path: &Path) {
    let mut paths = vec![path.to_path_buf()];

    if let Some(existing) = env::var_os(key) {
        paths.extend(env::split_paths(&existing));
    }

    let value = env::join_paths(paths).expect("join dynamic library search paths");
    command.env(key, value);
}

#[test]
fn c_ffi_integration_test_compiles_and_runs() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("target"));
    let profile_dir = target_dir.join("debug");

    let build_args = nested_build_args();
    let build_status = Command::new("cargo")
        .args(&build_args)
        .current_dir(&manifest_dir)
        .status()
        .expect("run cargo build --lib");
    assert!(
        build_status.success(),
        "cargo {} failed",
        build_args.join(" ")
    );

    let profile_dir = profile_dir
        .canonicalize()
        .expect("canonicalize debug target directory");
    // A private temp dir avoids predictable paths in the shared temp root and
    // cleans the compiled harness up automatically.
    let out_dir = tempfile::tempdir().expect("create private temp dir for C harness");
    let out = out_dir.path().join(format!(
        "oxidex_c_integration_test{}",
        env::consts::EXE_SUFFIX
    ));

    let compile_status = Command::new("cc")
        .arg("tests/ffi/c_integration_test.c")
        .arg("-Iinclude")
        .arg("-L")
        .arg(&profile_dir)
        .arg(format!("-Wl,-rpath,{}", profile_dir.display()))
        .arg("-loxidex")
        .arg("-o")
        .arg(&out)
        .current_dir(&manifest_dir)
        .status()
        .expect("compile C FFI integration test");
    assert!(compile_status.success(), "C FFI integration compile failed");

    let mut run = Command::new(&out);
    run.current_dir(&manifest_dir);
    prepend_env_path(&mut run, "DYLD_LIBRARY_PATH", &profile_dir);
    prepend_env_path(&mut run, "LD_LIBRARY_PATH", &profile_dir);
    prepend_env_path(&mut run, "PATH", &profile_dir);

    let run_status = run.status().expect("run C FFI integration test");
    assert!(run_status.success(), "C FFI integration test failed");
}
