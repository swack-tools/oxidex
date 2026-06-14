//! C FFI integration test wiring.
//!
//! This Cargo-visible test builds the oxidex library, compiles the C
//! integration test in `tests/ffi/c_integration_test.c` against the public
//! header in `include/`, links it against the produced oxidex library, and
//! runs the resulting executable. The C test returns a non-zero exit code if
//! any of its internal assertions fail, so a successful run proves the C ABI
//! is intact.

use std::env;
use std::path::PathBuf;
use std::process::Command;

#[test]
fn c_ffi_integration_test_compiles_and_runs() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| manifest_dir.join("target"));
    let profile_dir = target_dir.join("debug");

    // Ensure the cdylib/staticlib are built before linking the C test.
    let build_status = Command::new("cargo")
        .args(["build", "--lib"])
        .current_dir(&manifest_dir)
        .status()
        .expect("run cargo build --lib");
    assert!(build_status.success(), "cargo build --lib failed");

    let out = env::temp_dir().join("oxidex_c_integration_test");
    let compile_status = Command::new("cc")
        .arg("tests/ffi/c_integration_test.c")
        .arg("-Iinclude")
        .arg("-L")
        .arg(&profile_dir)
        .arg("-loxidex")
        .arg("-o")
        .arg(&out)
        .current_dir(&manifest_dir)
        .status()
        .expect("compile C FFI integration test");
    assert!(compile_status.success(), "C FFI integration compile failed");

    let mut run = Command::new(&out);
    run.current_dir(&manifest_dir);
    run.env("DYLD_LIBRARY_PATH", &profile_dir);
    run.env("LD_LIBRARY_PATH", &profile_dir);
    run.env(
        "PATH",
        format!(
            "{}:{}",
            profile_dir.display(),
            env::var("PATH").unwrap_or_default()
        ),
    );

    let run_status = run.status().expect("run C FFI integration test");
    assert!(run_status.success(), "C FFI integration test failed");
}
