// The harness shells out to `cc` with `-Wl,-rpath` and unix library search
// paths, none of which exist on Windows toolchains.
#![cfg(unix)]

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The directory holding the `oxidex` lib artifacts that the cargo invocation
/// running this test just built, derived from this test binary's own location
/// instead of from `CARGO_TARGET_DIR` plus a hard-coded profile name.
///
/// Deriving it is what lets the test link those artifacts directly, and
/// linking them directly is what lets it build nothing of its own. That is the
/// load-bearing part.
///
/// `[lib] crate-type = ["lib", "staticlib", "cdylib"]` makes cargo emit the lib
/// target with no `-C extra-filename` hash, so every configuration of it shares
/// one set of filenames: `target/<profile>/deps/liboxidex.{rlib,a,dylib}`. A
/// second build landing in the same profile directory overwrites the first,
/// cargo's fingerprints never notice, and `cargo test` compiles doctests LAST,
/// against whatever is on disk.
///
/// This test used to shell out to `cargo build --lib` for the artifacts, and so
/// was that second build. Left bare it built the DEFAULT feature graph, which
/// is how `cargo test --all-features` came to fail with E0432 on both
/// `parsers::magika_detector` doctests (#639). Forwarding the outer feature
/// graph fixed the E0432 but not the overwrite: a nested cargo inherits no
/// profile either, so it ran under `dev` while `cargo test` builds the lib
/// under `test` -- a different unit here, because `[profile.test]` sets
/// `opt-level = 2, codegen-units = 4` against `[profile.dev]`'s `0` and `16`.
/// Measured on `cargo test --all-features -v`: 44s into this test the nested
/// build logged "Compiling oxidex" / "Finished `dev` profile", and
/// `target/debug/deps/liboxidex.rlib` went from 119,540,352 bytes to
/// 99,273,872 -- the same library, recompiled unoptimised, underneath the
/// doctests about to link it.
///
/// A test cannot learn the profile NAME cargo invoked it under, so a nested
/// build cannot be made to match one. Not building is the fix that holds:
/// cargo builds the lib as a dependency of every integration-test target,
/// emitting all three crate types in one rustc call, so the artifacts are
/// already on disk before this test starts.
///
/// Note this is `target/<profile>/deps`, not the profile root: cargo uplifts a
/// lib target's artifacts to `target/<profile>/` only when it is a requested
/// target of the invocation, and under `cargo test` it is a dependency.
/// Verified with `cargo clean -p oxidex && cargo test --all-features --no-run`
/// -- afterwards `deps/` holds all three artifacts and the profile root holds
/// none. The old nested build was what uplifted them, which is the only reason
/// the previous `-L target/debug` resolved.
fn built_artifact_dir() -> PathBuf {
    let exe = env::current_exe().expect("locate this test binary");
    exe.parent()
        .expect("test binary sits in target/<profile>/deps")
        .canonicalize()
        .expect("canonicalize the directory holding this test binary")
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
    let lib_dir = built_artifact_dir();

    // Named rather than left to the linker so a missing artifact reports itself
    // here, by path, instead of as a bare `ld: library 'oxidex' not found`.
    let cdylib = lib_dir.join(format!(
        "{}oxidex{}",
        env::consts::DLL_PREFIX,
        env::consts::DLL_SUFFIX
    ));
    assert!(
        cdylib.is_file(),
        "{} is missing. Cargo emits it from the same lib unit as this test \
         binary's `oxidex` dependency, so the usual cause is running this \
         binary directly rather than through `cargo test` / `cargo nextest`.",
        cdylib.display()
    );

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
        .arg(&lib_dir)
        .arg(format!("-Wl,-rpath,{}", lib_dir.display()))
        .arg("-loxidex")
        .arg("-o")
        .arg(&out)
        .current_dir(&manifest_dir)
        .status()
        .expect("compile C FFI integration test");
    assert!(compile_status.success(), "C FFI integration compile failed");

    let mut run = Command::new(&out);
    run.current_dir(&manifest_dir);
    prepend_env_path(&mut run, "DYLD_LIBRARY_PATH", &lib_dir);
    prepend_env_path(&mut run, "LD_LIBRARY_PATH", &lib_dir);
    prepend_env_path(&mut run, "PATH", &lib_dir);

    let run_status = run.status().expect("run C FFI integration test");
    assert!(run_status.success(), "C FFI integration test failed");
}
