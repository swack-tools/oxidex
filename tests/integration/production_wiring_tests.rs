use oxidex::core::operations::{read_metadata, write_metadata};
use oxidex::core::{MetadataMap, TagValue};
use std::fs;
use std::io::Write;
use tempfile::NamedTempFile;

fn temp_with_suffix(suffix: &str) -> NamedTempFile {
    tempfile::Builder::new()
        .suffix(suffix)
        .tempfile()
        .expect("create temp file")
}

fn read_temp_file(bytes: &[u8], suffix: &str) -> MetadataMap {
    let mut file = temp_with_suffix(suffix);
    file.write_all(bytes)
        .unwrap_or_else(|err| panic!("write fixture bytes for {suffix}: {err}"));
    read_metadata(file.path())
        .unwrap_or_else(|err| panic!("read through production metadata path for {suffix}: {err:?}"))
}

fn evtx_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 4096];
    data[0..8].copy_from_slice(b"ElfFile\0");
    data[16..24].copy_from_slice(&4u64.to_le_bytes());
    data[24..32].copy_from_slice(&501u64.to_le_bytes());
    data[32..36].copy_from_slice(&128u32.to_le_bytes());
    data[36..38].copy_from_slice(&1u16.to_le_bytes());
    data[38..40].copy_from_slice(&3u16.to_le_bytes());
    data[40..42].copy_from_slice(&4096u16.to_le_bytes());
    data[42..44].copy_from_slice(&5u16.to_le_bytes());
    data[120..124].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    data
}

fn prefetch_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 256];
    data[0..4].copy_from_slice(&30u32.to_le_bytes());
    data[4..8].copy_from_slice(b"SCCA");
    data[12..16].copy_from_slice(&45_000u32.to_le_bytes());
    for (i, ch) in "NOTEPAD.EXE".encode_utf16().take(30).enumerate() {
        data[16 + i * 2..18 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    data[76..80].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
    data[128..136].copy_from_slice(&133500420450000000u64.to_le_bytes());
    data[144..148].copy_from_slice(&7u32.to_le_bytes());
    data
}

fn registry_fixture() -> Vec<u8> {
    let mut data = vec![0u8; 4096];
    data[0..4].copy_from_slice(b"regf");
    data[4..8].copy_from_slice(&100u32.to_le_bytes());
    data[8..12].copy_from_slice(&100u32.to_le_bytes());
    data[12..20].copy_from_slice(&133000000000000000u64.to_le_bytes());
    data[20..24].copy_from_slice(&1u32.to_le_bytes());
    data[24..28].copy_from_slice(&5u32.to_le_bytes());
    data[36..40].copy_from_slice(&0x1000u32.to_le_bytes());
    data[40..44].copy_from_slice(&1_048_576u32.to_le_bytes());
    for (i, ch) in "SYSTEM".encode_utf16().enumerate() {
        data[48 + i * 2..50 + i * 2].copy_from_slice(&ch.to_le_bytes());
    }
    data
}

fn pcap_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&[0xd4, 0xc3, 0xb2, 0xa1]);
    data.extend_from_slice(&2u16.to_le_bytes());
    data.extend_from_slice(&4u16.to_le_bytes());
    data.extend_from_slice(&0i32.to_le_bytes());
    data.extend_from_slice(&0u32.to_le_bytes());
    data.extend_from_slice(&65535u32.to_le_bytes());
    data.extend_from_slice(&1u32.to_le_bytes());
    data
}

fn binary_plist_fixture() -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(b"bplist00");
    data.extend(vec![0u8; 100]);
    let mut trailer = vec![0u8; 32];
    trailer[6] = 2;
    trailer[7] = 1;
    trailer[8..16].copy_from_slice(&5u64.to_be_bytes());
    trailer[24..32].copy_from_slice(&108u64.to_be_bytes());
    data.extend(trailer);
    data
}

fn der_x509_fixture() -> Vec<u8> {
    let mut der = vec![0x30, 0x10, 0x30, 0x0E];
    der.extend_from_slice(&[0x02, 0x01, 0x01]);
    der.extend_from_slice(&[0x02, 0x04, 0x12, 0x34, 0x56, 0x78]);
    der.extend_from_slice(&[0x30, 0x03, 0x06, 0x01, 0x00]);
    der
}

fn write_zip_fixture(entries: &[(&str, &[u8])], suffix: &str) -> tempfile::NamedTempFile {
    let file = temp_with_suffix(suffix);
    {
        let output = fs::File::create(file.path()).expect("open temp zip");
        let mut zip = zip::ZipWriter::new(output);
        let options = zip::write::FileOptions::default();
        for (name, data) in entries {
            zip.start_file(*name, options).expect("start zip entry");
            zip.write_all(data).expect("write zip entry");
        }
        zip.finish().expect("finish zip");
    }
    file
}

#[test]
fn read_metadata_routes_evtx() {
    let _write_metadata = write_metadata;

    assert_eq!(
        read_temp_file(&evtx_fixture(), ".evtx").get("FileType"),
        Some(&TagValue::String("Windows Event Log".to_string()))
    );
}

#[test]
fn read_metadata_routes_prefetch() {
    assert_eq!(
        read_temp_file(&prefetch_fixture(), ".pf").get("Prefetch:FileType"),
        Some(&TagValue::String("Windows Prefetch".to_string()))
    );
}

#[test]
fn read_metadata_routes_registry() {
    assert_eq!(
        read_temp_file(&registry_fixture(), ".dat").get("Registry:SequenceValid"),
        Some(&TagValue::String("Yes".to_string()))
    );
}

#[test]
fn read_metadata_routes_pcap() {
    assert!(read_temp_file(&pcap_fixture(), ".pcap").contains_key("PCAP:Version"));
}

#[test]
fn read_metadata_routes_binary_plist() {
    assert_eq!(
        read_temp_file(&binary_plist_fixture(), ".plist").get("Plist:Format"),
        Some(&TagValue::String("Binary".to_string()))
    );
}

#[test]
fn read_metadata_routes_ics_before_txt() {
    let ics = b"BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nSUMMARY:Planning\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    assert!(read_temp_file(ics, ".ics").contains_key("ICS:EventCount"));
}

#[test]
fn read_metadata_routes_eml_before_txt() {
    let eml = b"From: a@example.com\r\nTo: b@example.com\r\nSubject: Wiring\r\nDate: Wed, 10 Jun 2026 12:00:00 +0000\r\n\r\nBody";

    assert!(read_temp_file(eml, ".eml").contains_key("EML:Subject"));
}

#[test]
fn read_metadata_routes_der_x509_detection() {
    let metadata = read_temp_file(&der_x509_fixture(), ".der");
    assert!(metadata.contains_key("X509:SHA256Fingerprint"));
}

#[test]
fn read_metadata_routes_pages_to_iwork_parser() {
    let metadata_plist = br#"<plist><dict><key>Author</key><string>Ada</string></dict></plist>"#;
    let pages = write_zip_fixture(
        &[
            ("Index/Document.iwa", b""),
            ("Index/Metadata.plist", metadata_plist),
        ],
        ".pages",
    );

    assert_eq!(
        read_metadata(pages.path())
            .unwrap()
            .get("iWork:Application"),
        Some(&TagValue::String("Pages".to_string()))
    );
}

#[test]
fn read_metadata_routes_numbers_to_iwork_parser() {
    let numbers = write_zip_fixture(
        &[("Index/Document.iwa", b""), ("Index/Tables/table.iwa", b"")],
        ".numbers",
    );

    assert_eq!(
        read_metadata(numbers.path())
            .unwrap()
            .get("iWork:Application"),
        Some(&TagValue::String("Numbers".to_string()))
    );
}

#[test]
fn read_metadata_routes_keynote_to_iwork_parser() {
    let keynote = write_zip_fixture(&[("Index/Presentation.iwa", b"")], ".key");

    assert_eq!(
        read_metadata(keynote.path())
            .unwrap()
            .get("iWork:Application"),
        Some(&TagValue::String("Keynote".to_string()))
    );
}
