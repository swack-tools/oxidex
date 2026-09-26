// Runs `bindings/python/test_bindings.py` against the C ABI this `cargo test`
// just built, so the ctypes binding is exercised in CI (the `cargo nextest`
// step) rather than only by hand. Unix only, like the C harness beside it.
#![cfg(unix)]

use std::env;
use std::path::PathBuf;
use std::process::Command;

/// The `oxidex` cdylib cargo emitted for this invocation: it sits beside this
/// test binary in `target/<profile>/deps` (see `tests/ffi_c_integration.rs`
/// for why the harness links those artifacts rather than building its own).
fn built_cdylib() -> PathBuf {
    let exe = env::current_exe().expect("locate this test binary");
    let dir = exe
        .parent()
        .expect("test binary sits in target/<profile>/deps")
        .canonicalize()
        .expect("canonicalize the directory holding this test binary");
    dir.join(format!(
        "{}oxidex{}",
        env::consts::DLL_PREFIX,
        env::consts::DLL_SUFFIX
    ))
}

#[test]
fn python_ctypes_bindings_pass_their_unittests() {
    let cdylib = built_cdylib();
    assert!(
        cdylib.is_file(),
        "{} is missing; run this through `cargo test` / `cargo nextest`",
        cdylib.display()
    );
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let bindings = manifest_dir.join("bindings/python");
    let out = Command::new("python3")
        .args(["-m", "unittest", "-v", "test_bindings"])
        .current_dir(&bindings)
        .env("OXIDEX_LIBRARY", &cdylib)
        .env_remove("PYTHONPATH")
        .output()
        .expect("run python3 (the CI runners and this repo's tools require it)");
    let stderr = String::from_utf8_lossy(&out.stderr);
    println!("{stderr}");
    assert!(
        out.status.success(),
        "bindings/python/test_bindings.py failed:\n{stderr}"
    );
    // A run that collected nothing is not a pass.
    assert!(
        stderr.contains("\nOK") && !stderr.contains("Ran 0 tests"),
        "no Python binding tests ran:\n{stderr}"
    );
}
