//! ExifTool's `-b` separates declared List elements with line feeds.

use oxidex::exiftool_oracle;
use std::path::Path;
use std::process::Command;

fn binary_output(mut command: Command, path: &Path, tag: &str) -> Vec<u8> {
    let output = command
        .arg("-b")
        .arg(format!("-{tag}"))
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
fn declared_lists_keep_binary_line_separators() {
    let Some(oracle) = exiftool_oracle::graded() else {
        eprintln!("skipping declared-list parity: pinned oracle unavailable");
        return;
    };
    let sample = exiftool_oracle::capability_sample(oracle).unwrap();
    let images = sample.parent().unwrap();
    for (file, tag) in [
        ("MIE.mie", "Keywords"),
        ("MIE.mie", "References"),
        ("Lytro.lfp", "ImageColorCcmRgbToSrgbArray"),
        ("QuickTime.heic", "CompatibleBrands"),
        ("PLIST-bin.plist", "TestArray"),
        ("JXL2.jxl", "CompatibleBrands"),
        ("OOXML.docx", "HeadingPairs"),
        ("Kodak.jpg", "UsedExtensionNumbers"),
        ("MacOS.macos", "XAttrMDItemUserTags"),
        ("FlashPix.ppt", "TitleOfParts"),
        ("FlashPix.ppt", "HeadingPairs"),
        ("FlashPix.ppt", "Hyperlinks"),
    ] {
        let path = images.join(file);
        let theirs = binary_output(oracle.command(), &path, tag);
        assert!(!theirs.is_empty(), "{file} -{tag}: pinned source empty");
        let ours = binary_output(Command::new(env!("CARGO_BIN_EXE_oxidex")), &path, tag);
        assert_eq!(ours, theirs, "{file} -{tag}: {}", oracle.provenance());
    }
}
