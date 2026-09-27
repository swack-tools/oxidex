//! Public write behavior across read snapshots, saves and file identities.
use oxidex::core::operations::{read_metadata, remove_tag, write_metadata};
use oxidex::core::{MetadataMap, TagValue, WriteOutcome};
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

fn fixture() -> (TempDir, PathBuf) {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("image.jpg");
    fs::copy("tests/fixtures/jpeg/simple/synthetic_001.jpg", &path).unwrap();
    remove_tag(&path, "IFD0:Artist").unwrap();
    (dir, path)
}

fn artist(map: &mut MetadataMap) {
    map.insert("IFD0:Artist", TagValue::new_string("added by caller"));
}

#[test]
fn repeated_save_deletes_caller_added_row() {
    let (_dir, path) = fixture();
    let mut map = read_metadata(&path).unwrap();
    artist(&mut map);
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert!(map.remove("IFD0:Artist").is_some());
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
}

#[test]
fn successful_save_refreshes_clones_and_projections_without_hidden_deletions() {
    let (_dir, path) = fixture();
    let mut map = read_metadata(&path).unwrap();
    artist(&mut map);
    map.insert("ExifIFD:UserComment", TagValue::new_string("new directory"));
    let before_save = map.clone();
    write_metadata(&path, &map).unwrap();
    for mut view in [
        map.clone(),
        map.without_print_conv(),
        oxidex::core::exiftool_compat::format_for_exiftool(&map),
        oxidex::core::tag_normalization::normalize_metadata_map(&map),
        before_save,
    ] {
        view.remove("IFD0:Artist");
        write_metadata(&path, &view).unwrap();
        let read = read_metadata(&path).unwrap();
        assert!(read.get("IFD0:Artist").is_none());
        assert!(
            read.get("ExifIFD:ExifVersion").is_some(),
            "writer seed survives"
        );
        artist(&mut view);
        write_metadata(&path, &view).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn hardlink_alias_uses_read_deletion_intent() {
    let (dir, path) = fixture();
    let mut seed = MetadataMap::new();
    artist(&mut seed);
    write_metadata(&path, &seed).unwrap();
    let mut map = read_metadata(&path).unwrap();
    let alias = dir.path().join("alias.jpg");
    fs::hard_link(&path, &alias).unwrap();
    map.remove("IFD0:Artist");
    assert_eq!(write_metadata(&alias, &map).unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&alias).unwrap().get("IFD0:Artist").is_none());
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_some());
}

#[test]
fn pathname_replacement_does_not_inherit_read_deletion_intent() {
    let (dir, path) = fixture();
    let mut seed = MetadataMap::new();
    artist(&mut seed);
    write_metadata(&path, &seed).unwrap();
    let mut stale = read_metadata(&path).unwrap();
    stale.remove("IFD0:Artist");
    let replacement = dir.path().join("replacement.jpg");
    fs::copy(&path, &replacement).unwrap();
    fs::rename(replacement, &path).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert_eq!(
        write_metadata(&path, &stale).unwrap(),
        WriteOutcome::Unchanged
    );
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_some());
}

#[test]
fn failure_and_no_op_do_not_advance_source_rows() {
    let (_dir, path) = fixture();
    let mut map = read_metadata(&path).unwrap();
    let original = fs::read(&path).unwrap();
    assert_eq!(
        write_metadata(&path, &map).unwrap(),
        WriteOutcome::Unchanged
    );
    artist(&mut map);
    map.insert("NotAGroup:NotATag", TagValue::new_string("refused"));
    assert!(write_metadata(&path, &map).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    map.remove("NotAGroup:NotATag");
    write_metadata(&path, &map).unwrap();
    map.remove("IFD0:Artist");
    let after_add = fs::read(&path).unwrap();
    map.insert("NotAGroup:NotATag", TagValue::new_string("refused again"));
    assert!(write_metadata(&path, &map).is_err());
    assert_eq!(fs::read(&path).unwrap(), after_add);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_some());
    map.remove("NotAGroup:NotATag");
    write_metadata(&path, &map).unwrap();
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
}

#[test]
fn metadata_save_refreshes_identity_without_mutable_save() {
    let (_dir, path) = fixture();
    let mut metadata = oxidex::Metadata::from_path(&path).unwrap();
    metadata.insert("IFD0:Artist", "saved through Metadata");
    assert_eq!(metadata.save().unwrap(), WriteOutcome::Updated);
    metadata.remove("IFD0:Artist");
    assert_eq!(metadata.save().unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
}

#[test]
fn unsaved_assignment_removal_does_not_delete_an_external_addition() {
    let (_dir, path) = fixture();
    let mut map = read_metadata(&path).unwrap();
    artist(&mut map);
    map.insert("NotAGroup:NotATag", TagValue::new_string("refused"));
    assert!(write_metadata(&path, &map).is_err());
    map.remove("NotAGroup:NotATag");
    map.remove("IFD0:Artist");
    let mut external = MetadataMap::new();
    artist(&mut external);
    write_metadata(&path, &external).unwrap();
    assert_eq!(
        write_metadata(&path, &map).unwrap(),
        WriteOutcome::Unchanged
    );
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_some());
}

#[test]
fn clear_removes_saved_caller_rows() {
    let (_dir, path) = fixture();
    // JFIF rows are intentionally not writable. Use a JPEG without APP0 so
    // clear tests the writable rows, rather than a JFIF refusal.
    let mut bytes = fs::read(&path).unwrap();
    assert_eq!(&bytes[..4], &[0xff, 0xd8, 0xff, 0xe0]);
    let app0_length = usize::from(u16::from_be_bytes([bytes[4], bytes[5]]));
    bytes.drain(2..4 + app0_length);
    fs::write(&path, bytes).unwrap();
    let mut map = read_metadata(&path).unwrap();
    artist(&mut map);
    map.insert("ExifIFD:UserComment", TagValue::new_string("new directory"));
    write_metadata(&path, &map).unwrap();
    map.clear();
    write_metadata(&path, &map).unwrap();
    let saved = read_metadata(&path).unwrap();
    assert!(saved.get("IFD0:Artist").is_none());
    assert!(saved.get("ExifIFD:UserComment").is_none());
}

#[cfg(unix)]
#[test]
fn reading_through_symlink_authorizes_the_actual_file() {
    let (dir, path) = fixture();
    let mut seed = MetadataMap::new();
    artist(&mut seed);
    write_metadata(&path, &seed).unwrap();
    let alias = dir.path().join("alias.jpg");
    std::os::unix::fs::symlink(&path, &alias).unwrap();
    let mut map = read_metadata(&alias).unwrap();
    map.remove("IFD0:Artist");
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&alias).unwrap().get("IFD0:Artist").is_none());
}

#[test]
fn clone_that_never_contained_a_saved_addition_does_not_delete_it() {
    let (_dir, path) = fixture();
    let mut map = read_metadata(&path).unwrap();
    let mut blind = map.clone();
    artist(&mut map);
    write_metadata(&path, &map).unwrap();
    assert!(blind.remove("IFD0:Artist").is_none());
    assert_eq!(
        write_metadata(&path, &blind).unwrap(),
        WriteOutcome::Unchanged
    );
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_some());
}

#[cfg(unix)]
#[test]
fn successful_alias_save_keeps_other_live_hardlinks_authorized() {
    let (dir, path) = fixture();
    let mut seed = MetadataMap::new();
    artist(&mut seed);
    write_metadata(&path, &seed).unwrap();
    let mut map = read_metadata(&path).unwrap();
    let alias = dir.path().join("alias.jpg");
    fs::hard_link(&path, &alias).unwrap();
    map.remove("IFD0:Artist");
    write_metadata(&alias, &map).unwrap();
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
    artist(&mut map);
    write_metadata(&alias, &map).unwrap();
    write_metadata(&path, &map).unwrap();
    map.remove("IFD0:Artist");
    write_metadata(&alias, &map).unwrap();
    write_metadata(&path, &map).unwrap();
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
    assert!(read_metadata(&alias).unwrap().get("IFD0:Artist").is_none());
}

#[cfg(unix)]
#[test]
fn descriptor_limited_batch_reads_do_not_retain_source_handles() {
    use std::process::Command;
    let directory = TempDir::new().unwrap();
    for index in 0..128 {
        fs::copy(
            "tests/fixtures/jpeg/simple/synthetic_001.jpg",
            directory.path().join(format!("{index:03}.jpg")),
        )
        .unwrap();
    }
    let output = Command::new("bash")
        .args([
            "-c",
            "ulimit -n 64 || exit 90; exec \"$1\" -j -r \"$2\"",
            "provenance-fd-test",
        ])
        .arg(env!("CARGO_BIN_EXE_oxidex"))
        .arg(directory.path())
        .env("RAYON_NUM_THREADS", "2")
        .output()
        .unwrap();
    let rows: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let rows = rows.as_array().unwrap();
    let errors = rows.iter().filter(|row| row.get("Error").is_some()).count();
    assert_eq!(rows.len(), 128);
    assert_eq!(errors, 0, "descriptor-limited batch returned errors");
    assert!(
        output.status.success(),
        "batch exit {:?}: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn saved_alias_removal(key: &str) {
    let (_directory, path) = fixture();
    let mut map = read_metadata(&path).unwrap();
    map.insert(key, TagValue::new_string("alias addition"));
    let before = map.clone();
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    for mut view in [map.clone(), map.without_print_conv(), before] {
        assert!(view.remove(key).is_some());
        assert_eq!(write_metadata(&path, &view).unwrap(), WriteOutcome::Updated);
        assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
        view.insert(key, TagValue::new_string("alias addition"));
        assert_eq!(write_metadata(&path, &view).unwrap(), WriteOutcome::Updated);
    }
}

#[test]
fn saved_ungrouped_alias_removal() {
    saved_alias_removal("Artist");
}
#[test]
fn saved_family_zero_alias_removal() {
    saved_alias_removal("EXIF:Artist");
}
#[test]
fn saved_lowercase_alias_removal() {
    saved_alias_removal("ifd0:artist");
}

#[cfg(target_os = "macos")]
#[test]
fn pre_epoch_birth_timestamp_keeps_deletion_authority() {
    use std::os::unix::ffi::OsStrExt;
    let (_directory, path) = fixture();
    let mut seed = MetadataMap::new();
    artist(&mut seed);
    write_metadata(&path, &seed).unwrap();
    // This is the independently validated APFS setattrlist fixture, not a
    // replacement implementation of platform identity resolution.
    let mut attributes: libc::attrlist = unsafe { std::mem::zeroed() };
    attributes.bitmapcount = 5;
    attributes.commonattr = 0x200; // ATTR_CMN_CRTIME
    let timestamp = libc::timespec {
        tv_sec: -315619200,
        tv_nsec: 0,
    };
    let name = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: attributes selects exactly one timespec, whose live buffer and
    // length match that selection; name is a NUL-terminated pathname.
    let result = unsafe {
        libc::setattrlist(
            name.as_ptr(),
            (&mut attributes as *mut libc::attrlist).cast(),
            (&timestamp as *const libc::timespec).cast_mut().cast(),
            std::mem::size_of_val(&timestamp),
            0,
        )
    };
    assert_eq!(result, 0, "{}", std::io::Error::last_os_error());
    assert!(fs::metadata(&path).unwrap().created().unwrap() < std::time::UNIX_EPOCH);
    let mut map = read_metadata(&path).unwrap();
    map.remove("IFD0:Artist");
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
}

#[test]
fn saved_alias_removal_overrides_the_untouched_old_read_spelling() {
    let (_directory, path) = fixture();
    let mut seed = MetadataMap::new();
    artist(&mut seed);
    write_metadata(&path, &seed).unwrap();
    let mut map = read_metadata(&path).unwrap();
    map.insert("EXIF:Artist", TagValue::new_string("new alias value"));
    write_metadata(&path, &map).unwrap();
    map.remove("EXIF:Artist");
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
}

#[test]
fn saved_artist_alias_does_not_own_an_unrelated_group() {
    let (_directory, path) = fixture();
    let packet = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description rdf:about="" xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Artist="unrelated group"/></rdf:RDF></x:xmpmeta>"#;
    let mut payload = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
    payload.extend_from_slice(packet);
    let bytes = fs::read(&path).unwrap();
    let mut with_xmp = bytes[..2].to_vec();
    with_xmp.extend_from_slice(&[0xff, 0xe1]);
    with_xmp.extend_from_slice(&u16::try_from(payload.len() + 2).unwrap().to_be_bytes());
    with_xmp.extend_from_slice(&payload);
    with_xmp.extend_from_slice(&bytes[2..]);
    fs::write(&path, with_xmp).unwrap();
    let mut map = read_metadata(&path).unwrap();
    assert_eq!(map.get_string("XMP-tiff:Artist"), Some("unrelated group"));
    let original = fs::read(&path).unwrap();
    map.insert("Artist", TagValue::new_string("refused broad request"));
    // The authoritative writer refuses a bare request that would also change
    // the existing XMP field. Its failed assignment grants no removal binding.
    assert!(write_metadata(&path, &map).is_err());
    assert_eq!(fs::read(&path).unwrap(), original);
    map.remove("Artist");
    map.insert("EXIF:Artist", TagValue::new_string("caller field"));
    write_metadata(&path, &map).unwrap();
    map.remove("EXIF:Artist");
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    let result = read_metadata(&path).unwrap();
    assert!(result.get("IFD0:Artist").is_none());
    assert_eq!(
        result.get_string("XMP-tiff:Artist"),
        Some("unrelated group")
    );
}

#[test]
fn switching_saved_alias_spelling_keeps_the_new_assignment() {
    let (_directory, path) = fixture();
    let mut map = read_metadata(&path).unwrap();
    map.insert("Artist", TagValue::new_string("first spelling"));
    write_metadata(&path, &map).unwrap();
    map.remove("Artist");
    map.insert("EXIF:Artist", TagValue::new_string("second spelling"));
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert_eq!(
        read_metadata(&path).unwrap().get_string("IFD0:Artist"),
        Some("second spelling")
    );
    map.remove("EXIF:Artist");
    assert_eq!(write_metadata(&path, &map).unwrap(), WriteOutcome::Updated);
    assert!(read_metadata(&path).unwrap().get("IFD0:Artist").is_none());
}
