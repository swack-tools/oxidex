//! Only the verified pinned ExifTool may grade output (AGENTS.md: "Never
//! grade against an unpinned ExifTool"; "A matching `-ver` is not a working
//! oracle").
//!
//! `exiftool_oracle::available()` used to return true for any oracle that
//! *resolved*, only warning when it was not verified -- so with
//! `OXIDEX_ALLOW_EXIFTOOL_SKEW=1` a 13.55 on `$EXIFTOOL` graded every
//! byte-exact helper (`assert_validate_parity`, `oracle_edit`, ...), and an
//! oracle whose `.docx` reads as `ZIP` passed as long as `-ver` printed the
//! pin. Each case below runs in a child process (the oracle is resolved once
//! per process) against a fake `exiftool` script, with the real oracle's
//! environment scrubbed.

use oxidex::exiftool_oracle;
use std::path::Path;
use std::process::Command;

const CHILD_ENV: &str = "OXIDEX_ORACLE_GRADING_GATE_CHILD";

/// A fake ExifTool tree: `<dir>/exiftool` prints `version` for `-ver` and
/// `filetype` for anything else (the `-FileType` capability probe), beside a
/// `t/images/OOXML.docx`.
fn fake_tree(dir: &Path, version: &str, filetype: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir.join("t/images")).unwrap();
    std::fs::write(dir.join("t/images/OOXML.docx"), b"PK").unwrap();
    let script = dir.join("exiftool");
    std::fs::write(
        &script,
        format!("#!/bin/sh\nif [ \"$1\" = -ver ]; then echo {version}; else echo {filetype}; fi\n"),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    script
}

/// Runs `child_case` of this test binary against `script`, with every
/// variable that could name the real oracle replaced or removed.
fn run_child(case: &str, script: &Path, cache: &Path, require: bool) -> std::process::Output {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--exact", "child_case", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, case)
        .env(exiftool_oracle::BINARY_ENV, script)
        .env(exiftool_oracle::CACHE_DIR_ENV, cache)
        .env(exiftool_oracle::ALLOW_SKEW_ENV, "1")
        .env_remove(exiftool_oracle::PERL_ENV)
        .env_remove(exiftool_oracle::REQUIRE_ENV);
    if require {
        cmd.env(exiftool_oracle::REQUIRE_ENV, "1");
    }
    cmd.output().unwrap()
}

/// The child half: asserts no grading oracle, through every entry point.
#[test]
fn child_case() {
    if std::env::var_os(CHILD_ENV).is_none() {
        return;
    }
    // Resolution itself still succeeds (skew was explicitly allowed) ...
    assert!(
        exiftool_oracle::shared().is_ok(),
        "the fake did not resolve"
    );
    // ... but nothing may grade against it.
    assert!(exiftool_oracle::graded().is_none());
    assert!(!exiftool_oracle::available());
    assert!(exiftool_oracle::shared_command().is_none());
    println!("CHILD-OK");
}

#[test]
fn only_the_verified_pinned_oracle_grades() {
    if std::env::var_os(CHILD_ENV).is_some() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let cache = dir.path().join("no-cache");
    for (case, version, filetype) in [
        // A different release, resolved only because skew was allowed.
        ("skewed", "13.55", "DOCX"),
        // The pinned release that cannot open a ZIP container.
        ("degraded", exiftool_oracle::repo_pin(), "ZIP"),
    ] {
        let script = fake_tree(&dir.path().join(case), version, filetype);
        let out = run_child(case, &script, &cache, false);
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(out.status.success(), "{case}: {text}");
        assert!(
            text.contains("CHILD-OK"),
            "{case}: child did not run: {text}"
        );
        assert!(
            text.contains("skipping every ExifTool-graded check"),
            "{case}: the skip must be loud: {text}"
        );

        // With the oracle demanded (as CI demands it), the same is a failure.
        let out = run_child(case, &script, &cache, true);
        let text = String::from_utf8_lossy(&out.stderr).into_owned()
            + &String::from_utf8_lossy(&out.stdout);
        assert!(!out.status.success(), "{case}: required but passed: {text}");
        assert!(
            text.contains(exiftool_oracle::REQUIRE_ENV),
            "{case}: failure must name {}: {text}",
            exiftool_oracle::REQUIRE_ENV
        );
    }
}
