//! Shared filesystem transaction regressions: public APIs must refuse before
//! replacing a protected inode, and preserve its filesystem metadata on updates.
#![cfg(unix)]
use oxidex::core::operations::{clear_all_metadata, modify_tag, write_metadata};
use oxidex::core::{MetadataMap, TagValue, WriteOutcome};
use oxidex::ffi::*;
use std::ffi::CString;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;

fn fixture(path: &Path) {
    fs::copy("tests/fixtures/jpeg/simple/synthetic_001.jpg", path).unwrap();
}

fn write_api(path: &Path, api: &str) -> bool {
    match api {
        "map" => {
            let mut map = MetadataMap::new();
            map.insert("IFD0:Artist", TagValue::new_string("filesystem repair"));
            write_metadata(path, &map).is_ok()
        }
        "modify" => modify_tag(
            path,
            "IFD0:Artist",
            TagValue::new_string("filesystem repair"),
        )
        .is_ok(),
        "clear" => clear_all_metadata(path).is_ok(),
        "ffi" => {
            let handle = exiftool_create();
            assert!(!handle.is_null());
            assert_eq!(
                exiftool_set_tag_string(
                    handle,
                    c"IFD0:Artist".as_ptr(),
                    c"filesystem repair".as_ptr()
                ),
                EXIFTOOL_OK
            );
            let path = CString::new(path.to_str().unwrap()).unwrap();
            let mut outcome = -77;
            let code = exiftool_write_file_with_outcome(handle, path.as_ptr(), &mut outcome);
            exiftool_destroy(handle);
            if code != EXIFTOOL_OK {
                assert_eq!(outcome, -77);
            }
            code == EXIFTOOL_OK
        }
        "cli" => Command::new(env!("CARGO_BIN_EXE_oxidex"))
            .args(["--backup", "-IFD0:Artist=filesystem repair"])
            .arg(path)
            .output()
            .unwrap()
            .status
            .success(),
        _ => unreachable!(),
    }
}

fn identity(path: &Path) -> (u64, u32, u32, u32) {
    let m = fs::symlink_metadata(path).unwrap();
    (m.ino(), m.mode(), m.uid(), m.gid())
}

#[test]
fn symlink_targets_are_refused_by_every_entry_point() {
    for api in ["map", "modify", "clear", "ffi", "cli"] {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("target.jpg");
        fixture(&target);
        let path = dir.path().join("link.jpg");
        symlink(&target, &path).unwrap();
        let before = fs::read(&target).unwrap();
        let link_identity = identity(&path);
        let target_identity = identity(&target);
        let backup = path.with_extension("jpg.bak");
        fs::write(&backup, b"existing backup").unwrap();
        assert!(!write_api(&path, api), "{api} accepted a symlink");
        assert_eq!(fs::read(&backup).unwrap(), b"existing backup");
        assert_eq!(identity(&path), link_identity, "{api}: link replaced");
        assert_eq!(identity(&target), target_identity);
        assert_eq!(fs::read(&target).unwrap(), before);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            3,
            "{api}: scratch/backup left"
        );
    }
}

#[test]
fn readonly_targets_are_refused_by_every_entry_point() {
    for api in ["map", "modify", "clear", "ffi", "cli"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("readonly.jpg");
        fixture(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o444)).unwrap();
        let before = fs::read(&path).unwrap();
        let metadata = identity(&path);
        assert!(!write_api(&path, api), "{api} accepted a readonly file");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(identity(&path), metadata);
        assert_eq!(
            fs::read_dir(dir.path()).unwrap().count(),
            1,
            "{api}: scratch/backup left"
        );
    }
}

// Independent OS tools set and inspect extended attributes. The transaction's
// implementation is deliberately not called to manufacture expected values.
#[cfg(target_os = "macos")]
fn set_attributes(path: &Path) {
    assert!(
        Command::new("/usr/bin/chflags")
            .arg("nodump")
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
    for (name, value) in [
        ("org.oxidex.regression", "custom attribute"),
        ("com.apple.ResourceFork", "resource fork bytes"),
    ] {
        assert!(
            Command::new("/usr/bin/xattr")
                .args(["-w", name, value])
                .arg(path)
                .status()
                .unwrap()
                .success()
        );
    }
    assert!(
        Command::new("/usr/bin/xattr")
            .args([
                "-wx",
                "com.apple.FinderInfo",
                "0000000000000000000e00000000000000000000000000000000000000000000"
            ])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
    assert!(
        Command::new("/bin/chmod")
            .args([
                "+a",
                "everyone allow read,readattr,readextattr,readsecurity"
            ])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}
#[cfg(target_os = "macos")]
fn attributes(path: &Path) -> Vec<u8> {
    let output = Command::new("/usr/bin/xattr")
        .args(["-lx"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let acl = Command::new("/bin/ls")
        .arg("-lde")
        .arg(path)
        .output()
        .unwrap();
    assert!(acl.status.success());
    let flags = Command::new("/usr/bin/stat")
        .args(["-f", "%f"])
        .arg(path)
        .output()
        .unwrap();
    assert!(flags.status.success());
    let mut result = flags.stdout;
    result.extend(output.stdout);
    // First ls line contains path/size/time; only the numbered ACL rows matter.
    result.extend(
        String::from_utf8(acl.stdout)
            .unwrap()
            .lines()
            .skip(1)
            .collect::<Vec<_>>()
            .join("\n")
            .as_bytes(),
    );
    result
}
#[cfg(target_os = "linux")]
fn set_attributes(path: &Path) {
    // libacl constructs a real named-user POSIX ACL; Python's OS API supplies
    // an independent xattr oracle instead of calling our preservation helper.
    let output = Command::new("python3")
        .args([
            "-c",
            r#"
import ctypes, os, sys
p = os.fsencode(sys.argv[1])
os.setxattr(p, 'user.oxidex.regression', b'custom attribute')
a = ctypes.CDLL('libacl.so.1', use_errno=True)
a.acl_from_text.argtypes = [ctypes.c_char_p]
a.acl_from_text.restype = ctypes.c_void_p
a.acl_set_file.argtypes = [ctypes.c_char_p, ctypes.c_int, ctypes.c_void_p]
a.acl_free.argtypes = [ctypes.c_void_p]
acl = a.acl_from_text(b'u::rw-,u:65534:r--,g::r--,m::r--,o::---')
assert acl, ctypes.get_errno()
try:
    assert a.acl_set_file(p, 0x8000, acl) == 0, ctypes.get_errno()
finally:
    a.acl_free(acl)
"#,
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(target_os = "linux")]
fn attributes(path: &Path) -> Vec<u8> {
    let output = Command::new("python3")
        .args([
            "-c",
            r#"
import os, sys
p = sys.argv[1]
print([(n, os.getxattr(p, n).hex()) for n in sorted(os.listxattr(p))])
"#,
        ])
        .arg(path)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

#[test]
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn atomic_update_preserves_actual_owner_mode_acl_and_xattrs() {
    for api in ["map", "modify", "clear", "ffi", "cli"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("metadata.jpg");
        fixture(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        // Pick a supplementary group different from the scratch file's default.
        // This independently exercises fchown rather than checking a same-group copy.
        let output = Command::new("id").arg("-G").output().unwrap();
        assert!(output.status.success());
        let default_gid = fs::metadata(&path).unwrap().gid();
        if let Some(gid) = String::from_utf8(output.stdout)
            .unwrap()
            .split_whitespace()
            .map(|g| g.parse::<u32>().unwrap())
            .find(|gid| *gid != default_gid)
        {
            assert!(
                Command::new("chgrp")
                    .arg(gid.to_string())
                    .arg(&path)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        set_attributes(&path);
        let before = attributes(&path);
        let (ino, mode, uid, gid) = identity(&path);
        assert!(write_api(&path, api), "{api}: update refused");
        let (new_ino, new_mode, new_uid, new_gid) = identity(&path);
        assert_ne!(ino, new_ino, "{api}: expected atomic replacement");
        assert_eq!((new_mode, new_uid, new_gid), (mode, uid, gid), "{api}");
        assert_eq!(attributes(&path), before, "{api}: filesystem metadata lost");
    }
}

#[test]
fn no_op_keeps_the_original_inode() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("noop.jpg");
    fixture(&path);
    let before = identity(&path);
    assert_eq!(
        modify_tag(
            &path,
            "IFD0:Artist",
            TagValue::new_string("Synthetic Artist 1")
        )
        .unwrap(),
        WriteOutcome::Unchanged
    );
    assert_eq!(identity(&path), before);
}

#[test]
fn privileged_mode_is_refused_without_touching_existing_backup() {
    for api in ["map", "modify", "clear", "ffi", "cli"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("privileged.jpg");
        fixture(&path);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o2640)).unwrap();
        let backup = path.with_extension("jpg.bak");
        fs::write(&backup, b"previous backup").unwrap();
        let before = fs::read(&path).unwrap();
        let metadata = identity(&path);
        assert!(!write_api(&path, api), "{api} accepted privileged mode");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(identity(&path), metadata);
        assert_eq!(fs::read(&backup).unwrap(), b"previous backup");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn acl_denied_write_is_refused_without_backup_or_replacement() {
    for api in ["map", "modify", "clear", "ffi", "cli"] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("acl-readonly.jpg");
        fixture(&path);
        assert!(
            Command::new("/bin/chmod")
                .args(["+a", "everyone deny write"])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let before = fs::read(&path).unwrap();
        let metadata = identity(&path);
        let security = attributes(&path);
        assert!(!write_api(&path, api), "{api} bypassed write-denying ACL");
        assert_eq!(fs::read(&path).unwrap(), before);
        assert_eq!(identity(&path), metadata);
        assert_eq!(attributes(&path), security);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
