use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

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

    let build_status = Command::new("cargo")
        .args(["build", "--lib"])
        .current_dir(&manifest_dir)
        .status()
        .expect("run cargo build --lib");
    assert!(build_status.success(), "cargo build --lib failed");

    let profile_dir = profile_dir
        .canonicalize()
        .expect("canonicalize debug target directory");
    let out = env::temp_dir().join(format!(
        "oxidex_c_integration_test-{}{}",
        std::process::id(),
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
